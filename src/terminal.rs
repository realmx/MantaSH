//! Shared ANSI terminal state, input translation and authenticated Shell event parsing.
use crate::encoding::{Encoding, TerminalDecoder};
use alacritty_terminal::{
    Term,
    event::{Event, EventListener},
    grid::{Dimensions, Scroll},
    index::{Column, Line, Point, Side},
    selection::{Selection, SelectionRange, SelectionType},
    term::{
        Config, TermDamage, TermMode,
        cell::{Cell, Flags},
    },
    vte::ansi,
};
use base64::Engine;
use parking_lot::Mutex;
use std::{collections::VecDeque, sync::Arc};

#[derive(Clone, Default)]
pub struct TerminalEvents(pub Arc<Mutex<VecDeque<Event>>>);
impl EventListener for TerminalEvents {
    fn send_event(&self, event: Event) {
        self.0.lock().push_back(event);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GridSize {
    pub cols: usize,
    pub rows: usize,
}
impl Default for GridSize {
    fn default() -> Self {
        Self { cols: 80, rows: 24 }
    }
}
impl Dimensions for GridSize {
    fn total_lines(&self) -> usize {
        self.rows
    }
    fn screen_lines(&self) -> usize {
        self.rows
    }
    fn columns(&self) -> usize {
        self.cols
    }
}

#[derive(Default)]
struct ClearSequence {
    saved: bool,
    purge: bool,
    erase_viewport: bool,
}

impl alacritty_terminal::vte::Perform for ClearSequence {
    fn print(&mut self, _: char) {
        self.saved = false;
    }

    fn execute(&mut self, _: u8) {
        self.saved = false;
    }

    fn csi_dispatch(
        &mut self,
        params: &alacritty_terminal::vte::Params,
        intermediates: &[u8],
        ignore: bool,
        action: char,
    ) {
        let mut values = params.iter();
        let value = values
            .next()
            .filter(|value| value.len() == 1)
            .map(|value| value[0]);
        let simple = !ignore && intermediates.is_empty() && values.next().is_none();
        match (action, value) {
            ('J', Some(3)) if simple => self.saved = true,
            ('J', Some(2)) if simple => {
                self.erase_viewport = true;
                self.purge = self.saved;
                self.saved = false;
            }
            ('H', _) if simple => (),
            _ => self.saved = false,
        }
    }

    fn osc_dispatch(&mut self, _: &[&[u8]], _: bool) {
        self.saved = false;
    }

    fn esc_dispatch(&mut self, _: &[u8], _: bool, _: u8) {
        self.saved = false;
    }

    fn terminated(&self) -> bool {
        self.purge || self.erase_viewport
    }
}

pub struct TerminalBuffer {
    pub term: Term<TerminalEvents>,
    parser: ansi::Processor,
    clear_parser: alacritty_terminal::vte::Parser,
    clear_sequence: ClearSequence,
    decoder: TerminalDecoder,
    pub events: TerminalEvents,
    pub size: GridSize,
    pub encoding: Encoding,
    pub revision: u64,
    pub search_hits: Vec<(Point, Point)>,
    pub search_index: usize,
    overlay_dirty: bool,
    last_selection: Option<SelectionRange>,
    pub command_cursor: crate::command_cursor::CommandCursor,
    cursor_markers: crate::command_cursor::MarkerParser,
    // Only booleans: no command text or raw input is retained for resize decisions.
    prompt_touched: bool,
    prompt_submitted: bool,
    local: bool,
}

#[derive(Clone)]
pub struct PaintCell {
    pub row: usize,
    pub col: usize,
    pub cell: Cell,
    pub selected: bool,
    pub matched: bool,
}
#[derive(Clone)]
pub struct TerminalFrame {
    pub cells: Vec<PaintCell>,
    pub cursor: Option<(usize, usize, ansi::CursorShape)>,
    pub size: GridSize,
}

/// Owned changed rows plus cursor metadata for one coherent terminal frame.
pub struct FrameUpdate {
    pub rows: Vec<usize>,
    pub frame: TerminalFrame,
}

impl TerminalBuffer {
    /// Create a real terminal parser and scrollback grid; OS clipboard OSC access stays disabled.
    pub fn new(encoding: Encoding) -> Self {
        let size = GridSize::default();
        let events = TerminalEvents::default();
        let config = Config {
            scrolling_history: 10_000,
            osc52: alacritty_terminal::term::Osc52::Disabled,
            ..Default::default()
        };
        Self {
            term: Term::new(config, &size, events.clone()),
            parser: ansi::Processor::new(),
            clear_parser: Default::default(),
            clear_sequence: Default::default(),
            decoder: TerminalDecoder::new(encoding),
            events,
            size,
            encoding,
            revision: 0,
            search_hits: vec![],
            search_index: 0,
            overlay_dirty: true,
            last_selection: None,
            command_cursor: Default::default(),
            cursor_markers: Default::default(),
            prompt_touched: false,
            prompt_submitted: false,
            local: false,
        }
    }
    /// Local consoles erase the visible screen in place on ED2; genuine scrollback is retained.
    pub fn new_local(encoding: Encoding) -> Self {
        Self {
            local: true,
            ..Self::new(encoding)
        }
    }
    /// Decode whole stream chunks, preserving partial multibyte sequences between calls.
    pub fn feed(&mut self, bytes: &[u8]) -> bool {
        let (text, errors) = self.decoder.feed(bytes);
        let markers = self.cursor_markers.feed(text.as_bytes());
        let mut offset = 0;
        for (end, ready) in markers {
            self.advance_output(&text.as_bytes()[offset..end]);
            if ready {
                if self.prompt_submitted {
                    self.prompt_touched = false;
                    self.prompt_submitted = false;
                }
                self.command_cursor.ready(&self.term);
            } else {
                self.prompt_submitted = true;
                self.command_cursor.end_editing();
            }
            offset = end;
        }
        self.advance_output(&text.as_bytes()[offset..]);
        self.revision = self.revision.wrapping_add(1);
        errors
    }
    /// Register the same per-attempt nonce used by the Shell hooks, never a persisted value.
    pub fn set_shell_token(&mut self, token: String) {
        self.cursor_markers.set_token(token);
        self.command_cursor.end_editing();
        self.prompt_touched = false;
        self.prompt_submitted = false;
    }
    /// Split at viewport erases so local ED2 does not manufacture scrollback.
    /// Explicit saved-history erase retains its existing cross-chunk semantics.
    fn advance_ansi(&mut self, bytes: &[u8]) {
        let mut start = 0;
        while start < bytes.len() {
            let consumed = self
                .clear_parser
                .advance_until_terminated(&mut self.clear_sequence, &bytes[start..]);
            let end = start + consumed;
            if self.local && self.clear_sequence.erase_viewport {
                // The final J is isolated even if the CSI started in a previous feed.
                // Cancel that pending CSI in the processor, then use its ordinary ED0
                // handler from the origin to erase all rows without clear_viewport's
                // implicit scroll. ED2 must preserve the cursor and existing history.
                self.parser.advance(&mut self.term, &bytes[start..end - 1]);
                self.parser.advance(&mut self.term, b"\x18");
                let cursor = self.term.grid().cursor.point;
                self.term.grid_mut().cursor.point = Point::new(Line(0), Column(0));
                ansi::Handler::clear_screen(&mut self.term, ansi::ClearMode::Below);
                self.term.grid_mut().cursor.point = cursor;
            } else {
                self.parser.advance(&mut self.term, &bytes[start..end]);
            }
            self.clear_sequence.erase_viewport = false;
            if self.clear_sequence.purge {
                self.term.grid_mut().clear_history();
                self.clear_sequence.purge = false;
            }
            start = end;
        }
    }

    /// Preserve intermediate echo positions before a subsequent cursor-control sequence.
    fn advance_output(&mut self, bytes: &[u8]) {
        if !self.command_cursor.editing() {
            self.advance_ansi(bytes);
            return;
        }
        let mut start = 0;
        for (index, byte) in bytes.iter().enumerate() {
            if *byte < 32 || *byte == 127 {
                if start < index {
                    self.advance_ansi(&bytes[start..index]);
                    self.command_cursor.observe(&self.term);
                }
                self.advance_ansi(&bytes[index..=index]);
                // An incomplete escape sequence has not moved the terminal yet.
                self.command_cursor.observe(&self.term);
                start = index + 1;
            }
        }
        if start < bytes.len() {
            self.advance_ansi(&bytes[start..]);
            self.command_cursor.observe(&self.term);
        }
    }
    /// Submission invalidates click movement before the Shell starts a command/password reader.
    pub fn input_sent(&mut self, bytes: &[u8]) {
        if !bytes.is_empty() {
            self.prompt_touched = true;
        }
        if !bytes.starts_with(b"\x1b[200~") && bytes.iter().any(|b| matches!(b, 3 | 4 | 10 | 13)) {
            self.prompt_submitted = true;
            self.command_cursor.end_editing();
        }
    }
    /// Plan real arrow-key movement within the verified, echoed command region.
    pub fn cursor_movement(&self, col: usize, row: usize) -> Option<Vec<u8>> {
        self.command_cursor.movement(&self.term, col, row)
    }
    /// Clear a transient click selection and invalidate its paint overlay.
    pub fn clear_selection(&mut self) {
        self.term.selection = None;
        self.overlay_dirty = true;
        self.revision = self.revision.wrapping_add(1);
    }
    /// Reset only the transcoder; the connection and visible grid remain alive.
    pub fn set_encoding(&mut self, encoding: Encoding) {
        if encoding.is_terminal() {
            self.encoding = encoding;
            self.decoder = TerminalDecoder::new(encoding);
        }
    }
    /// Resize the terminal grid; prompt placement remains owned by the Shell.
    pub fn resize(&mut self, cols: usize, rows: usize) -> bool {
        self.resize_inner(cols, rows, false)
    }
    /// Resize a local PTY grid after the PTY worker applies the same size.
    /// Prompt and right-prompt redraw remain owned by the real Shell.
    pub fn resize_local(&mut self, cols: usize, rows: usize) -> bool {
        self.resize_inner(cols, rows, true)
    }
    fn resize_inner(&mut self, cols: usize, rows: usize, local: bool) -> bool {
        let size = GridSize {
            cols: cols.clamp(2, 1000),
            rows: rows.clamp(1, 500),
        };
        if size == self.size {
            return false;
        }
        // A right prompt can wrap into new history during resize. Discard only
        // rows created from an untouched first-line prompt on an empty screen.
        let empty_prompt_at_top = local
            && self.command_cursor.editing()
            && !self.prompt_touched
            && self.term.grid().cursor.point.line == Line(0)
            && self.term.grid().history_size() == 0
            && (1..self.size.rows).all(|row| self.term.grid()[Line(row as i32)].is_clear());
        self.command_cursor.end_editing();
        self.size = size;
        self.term.resize(size);
        if empty_prompt_at_top && self.term.grid().history_size() > 0 {
            self.term.grid_mut().clear_history();
        }
        self.revision += 1;
        true
    }
    /// Read an owned snapshot without consuming damage (used by tests and explicit inspection).
    pub fn frame(&self) -> TerminalFrame {
        self.frame_rows(&(0..self.size.rows).collect::<Vec<_>>())
    }

    /// The renderer is the sole damage consumer; only changed viewport rows are copied.
    pub fn take_frame_update(&mut self, force_full: bool) -> FrameUpdate {
        // Screen edits can rotate or invalidate a selection without a mouse event.
        let selection = self.term.renderable_content().selection;
        let selection_changed = selection != self.last_selection;
        let rows: Vec<usize> = if force_full || self.overlay_dirty || selection_changed {
            (0..self.size.rows).collect()
        } else {
            match self.term.damage() {
                TermDamage::Full => (0..self.size.rows).collect(),
                TermDamage::Partial(lines) => lines
                    .map(|line| line.line)
                    .filter(|row| *row < self.size.rows)
                    .collect(),
            }
        };
        let frame = self.frame_rows(&rows);
        self.term.reset_damage();
        self.overlay_dirty = false;
        self.last_selection = selection;
        FrameUpdate { rows, frame }
    }
    /// Copy direct grid rows so a single cursor-line update does not scan the entire viewport.
    fn frame_rows(&self, rows: &[usize]) -> TerminalFrame {
        let content = self.term.renderable_content();
        let offset = content.display_offset as i32;
        let selection = content.selection;
        let cursor_row = content.cursor.point.line.0 + offset;
        let cursor = if cursor_row >= 0
            && cursor_row < self.size.rows as i32
            && content.cursor.shape != ansi::CursorShape::Hidden
        {
            Some((
                cursor_row as usize,
                content.cursor.point.column.0,
                content.cursor.shape,
            ))
        } else {
            None
        };
        let mut cells = Vec::with_capacity(rows.len() * self.size.cols);
        for &row in rows {
            for col in 0..self.size.cols {
                let point = Point::new(Line(row as i32 - offset), Column(col));
                cells.push(PaintCell {
                    row,
                    col,
                    cell: self.term.grid()[point].clone(),
                    selected: selection.is_some_and(|s| s.contains(point)),
                    matched: self
                        .search_hits
                        .get(self.search_hits.partition_point(|(_, end)| *end < point))
                        .is_some_and(|(start, end)| *start <= point && point <= *end),
                });
            }
        }
        TerminalFrame {
            cells,
            cursor,
            size: self.size,
        }
    }
    /// Scroll without changing the live process cursor.
    pub fn scroll(&mut self, lines: i32) {
        self.term.scroll_display(Scroll::Delta(lines));
        self.revision += 1;
    }
    /// Bring the live cursor back into view before typed input.
    pub fn scroll_bottom(&mut self) {
        if self.term.grid().display_offset() != 0 {
            self.term.scroll_display(Scroll::Bottom);
            self.revision = self.revision.wrapping_add(1);
        }
    }
    /// Convert a viewport cell to a scrollback point.
    pub fn point(&self, col: usize, row: usize) -> Point {
        Point::new(
            Line(row.min(self.size.rows - 1) as i32 - self.term.grid().display_offset() as i32),
            Column(col.min(self.size.cols - 1)),
        )
    }
    /// Start character or semantic selection without recording terminal input.
    pub fn select_start(&mut self, col: usize, row: usize, word: bool) {
        self.overlay_dirty = true;
        self.revision = self.revision.wrapping_add(1);
        self.term.selection = Some(Selection::new(
            if word {
                SelectionType::Semantic
            } else {
                SelectionType::Simple
            },
            self.point(col, row),
            Side::Left,
        ));
    }
    /// Extend a drag selection using actual grid coordinates.
    pub fn select_to(&mut self, col: usize, row: usize) {
        self.overlay_dirty = true;
        let point = self.point(col, row);
        if let Some(s) = &mut self.term.selection {
            s.update(point, Side::Right);
            self.revision = self.revision.wrapping_add(1);
        }
    }
    /// Copy via the emulator so wrapped lines and wide characters are handled correctly.
    pub fn selected_text(&self) -> Option<String> {
        self.term.selection_to_string()
    }
    /// Select the complete scrollback and visible screen using emulator coordinates.
    pub fn select_all(&mut self) {
        self.overlay_dirty = true;
        let start = Point::new(Line(-(self.term.grid().history_size() as i32)), Column(0));
        let end = Point::new(Line(self.size.rows as i32 - 1), Column(self.size.cols - 1));
        let mut selection = Selection::new(SelectionType::Simple, start, Side::Left);
        selection.update(end, Side::Right);
        self.term.selection = Some(selection);
        self.revision = self.revision.wrapping_add(1);
    }
    /// Literal Unicode search across wrapped terminal lines, preserving spaces.
    pub fn search(&mut self, query: &str) -> usize {
        self.overlay_dirty = true;
        self.revision = self.revision.wrapping_add(1);
        self.search_hits.clear();
        self.search_index = 0;
        if query.is_empty() {
            return 0;
        }
        let mut text = String::new();
        let mut mapping = Vec::new();
        for row in -(self.term.grid().history_size() as i32)..self.size.rows as i32 {
            for col in 0..self.size.cols {
                let p = Point::new(Line(row), Column(col));
                let cell = &self.term.grid()[p];
                if cell
                    .flags
                    .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
                {
                    continue;
                }
                mapping.push((text.len(), p));
                text.push(cell.c);
                if let Some(extra) = cell.zerowidth() {
                    for c in extra {
                        mapping.push((text.len(), p));
                        text.push(*c);
                    }
                }
            }
            if !self.term.grid()[Point::new(Line(row), Column(self.size.cols - 1))]
                .flags
                .contains(Flags::WRAPLINE)
            {
                text.push('\n');
            }
        }
        for (byte, matched) in text.match_indices(query).take(5000) {
            let start = mapping.partition_point(|(i, _)| *i < byte);
            let end = mapping
                .partition_point(|(i, _)| *i < byte + matched.len())
                .saturating_sub(1);
            if let (Some((_, p)), Some((_, q))) = (mapping.get(start), mapping.get(end)) {
                self.search_hits.push((*p, *q));
            }
        }
        self.focus_match();
        self.search_hits.len()
    }
    /// Navigate search results in either direction without transferring focus to another pane.
    pub fn next_match(&mut self, backwards: bool) {
        self.overlay_dirty = true;
        let n = self.search_hits.len();
        if n == 0 {
            return;
        }
        self.search_index = if backwards {
            (self.search_index + n - 1) % n
        } else {
            (self.search_index + 1) % n
        };
        self.focus_match();
        self.revision = self.revision.wrapping_add(1);
    }
    fn focus_match(&mut self) {
        if let Some((p, _)) = self.search_hits.get(self.search_index) {
            self.term.scroll_to_point(*p);
        }
    }
    /// Respect bracketed-paste mode; external escape characters cannot terminate the bracket early.
    pub fn paste(&self, text: &str) -> anyhow::Result<Vec<u8>> {
        let text = text.replace('\u{1b}', "");
        let text = if self.term.mode().contains(TermMode::BRACKETED_PASTE) {
            format!("\u{1b}[200~{}\u{1b}[201~", text.replace("\r\n", "\n"))
        } else {
            text.replace("\r\n", "\r").replace('\n', "\r")
        };
        crate::encoding::encode(&text, self.encoding, false)
    }
}

/// Encode non-text keys using standard VT sequences. Text and IME commit use the input handler.
pub fn key_bytes(key: &str, ctrl: bool, alt: bool, shift: bool, mode: TermMode) -> Option<Vec<u8>> {
    let modifier = 1 + usize::from(shift) + 2 * usize::from(alt) + 4 * usize::from(ctrl);
    let final_byte = match key {
        "up" => Some('A'),
        "down" => Some('B'),
        "right" => Some('C'),
        "left" => Some('D'),
        "home" => Some('H'),
        "end" => Some('F'),
        _ => None,
    };
    if let Some(last) = final_byte {
        return Some(
            if modifier > 1 {
                format!("\u{1b}[1;{modifier}{last}")
            } else if mode.contains(TermMode::APP_CURSOR) {
                format!("\u{1b}O{last}")
            } else {
                format!("\u{1b}[{last}")
            }
            .into_bytes(),
        );
    }
    let tilde = match key {
        "insert" => Some(2),
        "delete" => Some(3),
        "pageup" => Some(5),
        "pagedown" => Some(6),
        "f5" => Some(15),
        "f6" => Some(17),
        "f7" => Some(18),
        "f8" => Some(19),
        "f9" => Some(20),
        "f10" => Some(21),
        "f11" => Some(23),
        "f12" => Some(24),
        _ => None,
    };
    if let Some(n) = tilde {
        return Some(
            if modifier == 1 {
                format!("\u{1b}[{n}~")
            } else {
                format!("\u{1b}[{n};{modifier}~")
            }
            .into_bytes(),
        );
    }
    if let Some(n) = ["f1", "f2", "f3", "f4"].iter().position(|k| *k == key) {
        let final_byte = (b'P' + n as u8) as char;
        return Some(
            if modifier == 1 {
                format!("\u{1b}O{final_byte}")
            } else {
                format!("\u{1b}[1;{modifier}{final_byte}")
            }
            .into_bytes(),
        );
    }
    let control = match key {
        "enter" => Some(13),
        "backspace" => Some(if ctrl { 8 } else { 127 }),
        "escape" => Some(27),
        "tab" if !shift => Some(9),
        "tab" => return Some(b"\x1b[Z".to_vec()),
        "space" if ctrl => Some(0),
        "2" | "@" if ctrl => Some(0),
        "3" | "[" if ctrl => Some(27),
        "4" | "\\" if ctrl => Some(28),
        "5" | "]" if ctrl => Some(29),
        "6" | "^" if ctrl => Some(30),
        "7" | "_" | "-" if ctrl => Some(31),
        "8" | "?" if ctrl => Some(127),
        _ if ctrl && key.len() == 1 && key.as_bytes()[0].is_ascii_alphabetic() => {
            Some(key.as_bytes()[0].to_ascii_uppercase() & 0x1f)
        }
        _ => None,
    };
    control.map(|b| if alt { vec![27, b] } else { vec![b] })
}

#[derive(Debug, PartialEq, Eq)]
pub struct ShellReport {
    pub command: String,
    pub directory: String,
}
/// A bounded OSC collector. Only this session's nonce and base64 Shell reports are accepted.
pub struct HistoryParser {
    token: String,
    bytes: Vec<u8>,
    state: u8,
    overflow: bool,
}
impl HistoryParser {
    /// The random nonce is generated for each connection attempt and never persisted.
    pub fn new(token: String) -> Self {
        Self {
            token,
            bytes: vec![],
            state: 0,
            overflow: false,
        }
    }
    /// Parse only output from the child, never user keyboard or authentication input.
    pub fn feed(&mut self, input: &[u8]) -> Vec<ShellReport> {
        let mut reports = Vec::new();
        for &b in input {
            match self.state {
                0 if b == 27 => self.state = 1,
                1 if b == b']' => {
                    self.state = 2;
                    self.bytes.clear();
                    self.overflow = false;
                }
                1 => self.state = 0,
                2 if b == 7 => {
                    if let Some(r) = self.finish() {
                        reports.push(r);
                    }
                    self.state = 0;
                }
                2 if b == 27 => self.state = 3,
                2 => {
                    if self.bytes.len() < 32_768 {
                        self.bytes.push(b);
                    } else {
                        self.overflow = true;
                    }
                }
                3 if b == b'\\' => {
                    if let Some(r) = self.finish() {
                        reports.push(r);
                    }
                    self.state = 0;
                }
                3 => {
                    self.state = 0;
                    self.bytes.clear();
                }
                _ => {}
            }
        }
        reports
    }
    fn finish(&self) -> Option<ShellReport> {
        if self.overflow {
            return None;
        }
        let text = std::str::from_utf8(&self.bytes).ok()?;
        let mut fields = text.split(';');
        if fields.next()? != "777" || fields.next()? != "mantash" || fields.next()? != self.token {
            return None;
        }
        let command = String::from_utf8(
            base64::engine::general_purpose::STANDARD
                .decode(fields.next()?)
                .ok()?,
        )
        .ok()?;
        let directory = String::from_utf8(
            base64::engine::general_purpose::STANDARD
                .decode(fields.next()?)
                .ok()?,
        )
        .ok()?;
        if fields.next().is_some()
            || command.starts_with(' ')
            || command.len() > 4096
            || command.contains('\0')
            || directory.contains('\0')
        {
            return None;
        }
        Some(ShellReport { command, directory })
    }
}
