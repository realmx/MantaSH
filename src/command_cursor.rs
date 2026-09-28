//! Cursor requests are based on authenticated prompt markers and echoed grid content.
use crate::terminal::TerminalEvents;
use alacritty_terminal::{
    grid::Dimensions,
    index::{Column, Line, Point},
    term::{Term, TermMode, cell::Flags},
};

const MAX_COMMAND_CELLS: usize = 32768;

/// A bounded parser for session-owned prompt boundaries; it contains no command text.
#[derive(Default)]
pub struct MarkerParser {
    token: String,
    bytes: Vec<u8>,
    state: u8,
}
impl MarkerParser {
    /// Replace the connection-attempt nonce and discard a partially received marker.
    pub fn set_token(&mut self, token: String) {
        self.token = token;
        self.bytes.clear();
        self.state = 0;
    }
    /// Return marker ends inside this chunk so the terminal can capture the precise cursor there.
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<(usize, bool)> {
        let mut result = vec![];
        for (index, &byte) in bytes.iter().enumerate() {
            match (self.state, byte) {
                (0, 27) => self.state = 1,
                (1, b']') => {
                    self.state = 2;
                    self.bytes.clear();
                }
                (1, _) => self.state = 0,
                (2, 7) | (3, b'\\') => {
                    if !self.token.is_empty() {
                        let prefix = format!("777;mantash-cursor;{};", self.token);
                        if let Ok(text) = std::str::from_utf8(&self.bytes) {
                            if let Some(state) = text.strip_prefix(&prefix) {
                                match state {
                                    "ready" => result.push((index + 1, true)),
                                    "running" => result.push((index + 1, false)),
                                    _ => {}
                                }
                            }
                        }
                    }
                    self.state = 0;
                }
                (2, 27) => self.state = 3,
                (2, _) if self.bytes.len() < 256 => self.bytes.push(byte),
                _ => {
                    self.state = 0;
                    self.bytes.clear();
                }
            }
        }
        result
    }
}

/// Only a Shell-owned editable prompt is eligible for synthesized arrow keys.
#[derive(Default)]
pub struct CommandCursor {
    prompt: Option<Vec<(char, Vec<char>)>>,
    end: usize,
}
impl CommandCursor {
    /// Invalidate command clicks as soon as submission or execution starts.
    pub fn end_editing(&mut self) {
        self.prompt = None;
        self.end = 0;
    }
    /// Whether a prompt boundary has been observed for this attempt.
    pub fn editing(&self) -> bool {
        self.prompt.is_some()
    }
    /// Capture only the displayed prompt prefix at the exact marker position.
    pub fn ready(&mut self, term: &Term<TerminalEvents>) {
        self.end_editing();
        let cursor = term.grid().cursor.point;
        let Some(start) = logical_start(term, cursor.line.0) else {
            return;
        };
        let length = (cursor.line.0 - start) as usize * term.columns() + cursor.column.0;
        if length > 4096 {
            return;
        }
        let prefix = (0..length).map(|n| cell_key(term, start, n)).collect();
        self.prompt = Some(prefix);
        self.end = length;
    }
    /// Observe echoed cursor movement, including spaces, without intercepting or retaining keys.
    pub fn observe(&mut self, term: &Term<TerminalEvents>) {
        if !self.editing() {
            return;
        }
        let Some((_, cursor)) = self.position(term) else {
            return;
        };
        self.end = self.end.max(cursor).min(MAX_COMMAND_CELLS);
    }
    fn position(&self, term: &Term<TerminalEvents>) -> Option<(i32, usize)> {
        let prefix = self.prompt.as_ref()?;
        if term
            .mode()
            .intersects(TermMode::ALT_SCREEN | TermMode::MOUSE_MODE)
        {
            return None;
        }
        let cursor = term.grid().cursor.point;
        let start = logical_start(term, cursor.line.0)?;
        let current = (cursor.line.0 - start) as usize * term.columns()
            + cursor.column.0
            + usize::from(term.grid().cursor.input_needs_wrap);
        if current < prefix.len() || current > MAX_COMMAND_CELLS {
            return None;
        }
        if !(0..prefix.len()).all(|n| cell_key(term, start, n) == prefix[n]) {
            return None;
        }
        Some((start, current))
    }
    /// Move within the current echoed command, never into a prompt, history or password input.
    pub fn movement(&self, term: &Term<TerminalEvents>, col: usize, row: usize) -> Option<Vec<u8>> {
        if term.grid().display_offset() != 0 || col >= term.columns() || row >= term.screen_lines()
        {
            return None;
        }
        let (start, current) = self.position(term)?;
        if (row as i32) < start {
            return None;
        }
        let mut target = (row as i32 - start) as usize * term.columns() + col;
        let prefix = self.prompt.as_ref()?.len();
        if target < prefix || target > self.end.max(current) {
            return None;
        }
        // A wide glyph's trailing cell targets the beginning of that glyph.
        if term.grid()[Point::new(Line(row as i32), Column(col))]
            .flags
            .contains(Flags::WIDE_CHAR_SPACER)
        {
            target = target.saturating_sub(1);
        }
        let (from, to) = if target < current {
            (target, current)
        } else {
            (current, target)
        };
        if to > MAX_COMMAND_CELLS {
            return None;
        }
        let steps = (from..to)
            .filter(|&n| {
                let point = Point::new(
                    Line(start + (n / term.columns()) as i32),
                    Column(n % term.columns()),
                );
                !term.grid()[point]
                    .flags
                    .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
            })
            .count();
        let arrow = crate::terminal::key_bytes(
            if target < current { "left" } else { "right" },
            false,
            false,
            false,
            *term.mode(),
        )?;
        Some(arrow.repeat(steps))
    }
}

/// Follow only soft wraps; hard newlines or a prompt from another context are separate regions.
fn logical_start(term: &Term<TerminalEvents>, row: i32) -> Option<i32> {
    let mut start = row;
    let first = -(term.grid().history_size() as i32);
    while start > first
        && term.grid()[Point::new(Line(start - 1), Column(term.columns() - 1))]
            .flags
            .contains(Flags::WRAPLINE)
    {
        start -= 1;
        if (row - start) as usize * term.columns() > MAX_COMMAND_CELLS {
            return None;
        }
    }
    Some(start)
}
fn cell_key(term: &Term<TerminalEvents>, start: i32, n: usize) -> (char, Vec<char>) {
    let cell = &term.grid()[Point::new(
        Line(start + (n / term.columns()) as i32),
        Column(n % term.columns()),
    )];
    (
        cell.c,
        cell.zerowidth()
            .map(|extra| extra.to_vec())
            .unwrap_or_default(),
    )
}
