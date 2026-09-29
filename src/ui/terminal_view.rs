//! GPUI terminal surface: measured cells, IME composition and native keyboard/mouse input.
use super::theme::Palette;
use crate::terminal_paint::{self, Background, PaintRow};
use crate::{
    model::{Owner, Preferences, Theme},
    services::Session,
    terminal,
};
use alacritty_terminal::grid::Dimensions as _;
use alacritty_terminal::{
    term::{TermMode, cell::Flags},
    vte::ansi::{Color, CursorShape, NamedColor},
};
use gpui::prelude::*;
use gpui::*;
use std::{ops::Range, sync::Arc};

/// Rows retain shaped glyphs across cursor, pointer and unrelated workbench repaints.
struct CachedRow {
    plan: PaintRow,
    text: Vec<(usize, usize, ShapedLine)>,
}
#[derive(PartialEq)]
struct PaintKey {
    family: String,
    size: f32,
    cell_width: Pixels,
    line_height: Pixels,
    theme: Theme,
}
struct CachedPaint {
    key: PaintKey,
    revision: u64,
    /// Terminal dimensions used to validate row reuse after a window or font resize.
    grid: terminal::GridSize,
    rows: Vec<Arc<CachedRow>>,
    cursor: Option<(usize, usize, CursorShape)>,
}
#[cfg(debug_assertions)]
#[derive(Default, serde::Serialize)]
pub struct PaintStatistics {
    paints: u64,
    frames_prepared: u64,
    rows_shaped: u64,
    cells_copied: u64,
    total_paint_ms: f64,
    max_paint_ms: f64,
}

#[derive(Clone)]
pub struct PaneFocused(pub Owner);
pub struct TerminalView {
    pub session: Arc<Session>,
    pub focus: FocusHandle,
    pub preferences: Preferences,
    pub connected: bool,
    pub error: Option<String>,
    bounds: Bounds<Pixels>,
    cell_width: Pixels,
    line_height: Pixels,
    /// True while the overlay scrollbar thumb is being dragged.
    scroll_dragging: bool,
    selecting: bool,
    mouse_reporting: bool,
    click_start: Option<Point<Pixels>>,
    click_moved: bool,
    scroll: crate::terminal_io::ScrollAccumulator,
    preedit: String,
    preedit_selection: Range<usize>,
    cursor_bounds: Bounds<Pixels>,
    cached_paint: Option<Arc<CachedPaint>>,
    measured_font: Option<(String, f32)>,
    measured_grid: Option<(usize, usize)>,
    #[cfg(debug_assertions)]
    pub paint_statistics: PaintStatistics,
    #[cfg(debug_assertions)]
    pub(super) host_scroll_metrics: std::cell::Cell<Option<(f32, f32, f32)>>,
}
impl EventEmitter<PaneFocused> for TerminalView {}
impl Focusable for TerminalView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl TerminalView {
    /// Geometry only, exposed by the opt-in isolated native QA driver.
    #[cfg(debug_assertions)]
    pub(super) fn qa_geometry(&self) -> serde_json::Value {
        serde_json::json!({"height": f32::from(self.bounds.size.height),
            "width": f32::from(self.bounds.size.width),
            "line_height": f32::from(self.line_height),
            "cell_width": f32::from(self.cell_width), "measured_grid": self.measured_grid,
            "rendered_scroll_metrics": self.host_scroll_metrics.get()})
    }

    /// Construct a view for a fixed session attempt.
    pub fn new(session: Arc<Session>, preferences: Preferences, cx: &mut Context<Self>) -> Self {
        Self {
            session,
            preferences,
            focus: cx.focus_handle(),
            connected: false,
            error: None,
            bounds: Bounds::default(),
            cell_width: px(8.),
            line_height: px(18.),
            scroll_dragging: false,
            selecting: false,
            mouse_reporting: false,
            click_start: None,
            click_moved: false,
            scroll: Default::default(),
            preedit: String::new(),
            preedit_selection: 0..0,
            cursor_bounds: Bounds::default(),
            cached_paint: None,
            measured_font: None,
            measured_grid: None,
            #[cfg(debug_assertions)]
            paint_statistics: PaintStatistics::default(),
            #[cfg(debug_assertions)]
            host_scroll_metrics: Default::default(),
        }
    }
    /// Prepare only changed rows, and never wait on the PTY parser from a paint callback.
    fn prepare_paint(&mut self, window: &mut Window) -> Option<Arc<CachedPaint>> {
        let key = PaintKey {
            family: self.preferences.terminal_font.clone(),
            size: self.preferences.terminal_size,
            cell_width: self.cell_width,
            line_height: self.line_height,
            theme: self.preferences.theme,
        };
        let Some(mut buffer) = self.session.terminal.try_lock() else {
            window.request_animation_frame();
            // A busy parser can reuse the last frame only with the same
            // font metrics and measured grid; a resize must wait for reflow.
            return self
                .cached_paint
                .as_ref()
                .filter(|cached| {
                    cached.key == key
                        && self.measured_grid == Some((cached.grid.cols, cached.grid.rows))
                })
                .cloned();
        };
        // The canvas is retained while AppKit animates a window resize. Do not
        // paint a frame for the old geometry into the new bounds: its prompt
        // cells would remain visible until a later frame clears them.
        if self.measured_grid != Some((buffer.size.cols, buffer.size.rows)) {
            window.request_animation_frame();
            return None;
        }
        let revision = buffer.revision;
        self.session.output_wakeup.acknowledge();
        if let Some(cached) = &self.cached_paint {
            if cached.key == key && cached.grid == buffer.size && cached.revision == revision {
                return Some(cached.clone());
            }
        }
        let previous_compatible = self.cached_paint.as_ref().is_some_and(|cached| {
            cached.key == key && cached.grid == buffer.size && cached.rows.len() == buffer.size.rows
        });
        let update = buffer.take_frame_update(!previous_compatible);
        drop(buffer);
        let frame = update.frame;
        let changed: std::collections::HashSet<_> = update.rows.into_iter().collect();
        #[cfg(debug_assertions)]
        {
            self.paint_statistics.cells_copied += frame.cells.len() as u64;
        }
        let p = Palette::new(key.theme);
        let old = self
            .cached_paint
            .as_ref()
            .filter(|old| old.key == key && old.grid == frame.size);
        let mut rows = Vec::with_capacity(frame.size.rows);
        for (index, plan) in terminal_paint::rows(&frame).into_iter().enumerate() {
            if let Some(cached) = old
                .and_then(|old| old.rows.get(index))
                .filter(|row| !changed.contains(&index) || row.plan == plan)
            {
                rows.push(cached.clone());
                continue;
            }
            let mut text = Vec::with_capacity(plan.text.len());
            for span in &plan.text {
                let mut fg = ansi_color(span.foreground, p, key.theme);
                if span.flags.contains(Flags::DIM) {
                    fg = fg.opacity(0.65);
                }
                let mut family = font(key.family.clone());
                // Each terminal cell is independently positioned; cross-cell ligatures were never supported.
                family.features = FontFeatures(Arc::new(vec![
                    ("liga".into(), 0),
                    ("clig".into(), 0),
                    ("calt".into(), 0),
                ]));
                if span.flags.contains(Flags::BOLD) {
                    family.weight = FontWeight::BOLD;
                }
                if span.flags.contains(Flags::ITALIC) {
                    family.style = FontStyle::Italic;
                }
                let run = TextRun {
                    len: span.text.len(),
                    font: family,
                    color: fg,
                    background_color: None,
                    underline: span.flags.intersects(Flags::ALL_UNDERLINES).then_some(
                        UnderlineStyle {
                            color: Some(fg),
                            thickness: px(1.),
                            wavy: span.flags.contains(Flags::UNDERCURL),
                        },
                    ),
                    strikethrough: span.flags.contains(Flags::STRIKEOUT).then_some(
                        StrikethroughStyle {
                            color: Some(fg),
                            thickness: px(1.),
                        },
                    ),
                };
                text.push((
                    span.column,
                    span.width,
                    window.text_system().shape_line(
                        span.text.clone().into(),
                        px(key.size),
                        &[run],
                        Some(key.cell_width),
                    ),
                ));
            }
            #[cfg(debug_assertions)]
            {
                self.paint_statistics.rows_shaped += 1;
            }
            rows.push(Arc::new(CachedRow { plan, text }));
        }
        let cached = Arc::new(CachedPaint {
            key,
            revision,
            grid: frame.size,
            rows,
            cursor: frame.cursor,
        });
        self.cached_paint = Some(cached.clone());
        #[cfg(debug_assertions)]
        {
            self.paint_statistics.frames_prepared += 1;
        }
        Some(cached)
    }
    /// Send typed text after IME commit. It is never added directly to history.
    pub fn type_text(&mut self, text: &str, cx: &mut Context<Self>) {
        if !self.connected {
            return;
        }
        let mut terminal = self.session.terminal.lock();
        terminal.scroll_bottom();
        match crate::encoding::encode(text, terminal.encoding, false) {
            Ok(bytes) => {
                drop(terminal);
                self.session.input(bytes);
                self.error = None;
            }
            Err(error) => {
                self.error = Some(error.to_string());
            }
        }
        cx.notify();
    }
    /// Paste through the terminal's bracketed-paste rules.
    pub fn paste_text(&mut self, text: &str, cx: &mut Context<Self>) {
        if !self.connected {
            return;
        }
        let result = self.session.terminal.lock().paste(text);
        match result {
            Ok(bytes) => self.session.input(bytes),
            Err(error) => self.error = Some(error.to_string()),
        }
        cx.notify();
    }
    fn copy(&self, cx: &mut App) {
        if let Some(text) = self.session.terminal.lock().selected_text() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }
    fn key(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let key = &event.keystroke;
        let mods = key.modifiers;
        let shortcut = if cfg!(target_os = "macos") {
            mods.platform
        } else {
            mods.control && mods.shift
        };
        if shortcut && key.key == "c" {
            self.copy(cx);
            cx.stop_propagation();
            return;
        }
        if shortcut && key.key == "v" {
            if let Some(text) = cx.read_from_clipboard().and_then(|i| i.text()) {
                self.paste_text(&text, cx);
            }
            cx.stop_propagation();
            return;
        }
        if mods.platform {
            return;
        }
        if !self.preedit.is_empty() {
            // The IME owns marked text. A fallback Tab must not become a text
            // replacement, clear composition, or reach the Shell as completion.
            if key.key == "tab" {
                cx.stop_propagation();
            }
            return;
        }
        if mods.shift && matches!(key.key.as_str(), "pageup" | "pagedown") {
            self.session
                .terminal
                .lock()
                .scroll(if key.key == "pageup" { 20 } else { -20 });
            cx.notify();
            cx.stop_propagation();
            return;
        }
        let mode = *self.session.terminal.lock().term.mode();
        if self.connected {
            if let Some(bytes) =
                terminal::key_bytes(&key.key, mods.control, mods.alt, mods.shift, mode)
            {
                self.session.terminal.lock().scroll_bottom();
                self.session.input(bytes);
                cx.stop_propagation();
            } else if mods.alt && key.key.len() == 1 {
                self.type_text(&format!("\u{1b}{}", key.key), cx);
                cx.stop_propagation();
            }
        }
    }
    fn cell_at(&self, position: Point<Pixels>) -> (usize, usize) {
        (
            ((position.x - self.bounds.left()) / self.cell_width).max(0.) as usize,
            ((position.y - self.bounds.top()) / self.line_height).max(0.) as usize,
        )
    }
    fn mouse_sequence(&self, button: u8, col: usize, row: usize, release: bool) -> Option<Vec<u8>> {
        crate::terminal_io::mouse_bytes(
            *self.session.terminal.lock().term.mode(),
            button,
            col,
            row,
            release,
        )
    }
    fn mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.focus.focus(window);
        cx.emit(PaneFocused(self.session.owner));
        let (col, row) = self.cell_at(event.position);
        self.click_start =
            if event.click_count == 1 && !event.modifiers.shift && self.preedit.is_empty() {
                Some(event.position)
            } else {
                None
            };
        self.click_moved = false;
        self.mouse_reporting = false;
        if !event.modifiers.shift {
            if let Some(bytes) = self.mouse_sequence(0, col, row, false) {
                self.mouse_reporting = true;
                self.selecting = false;
                self.click_start = None;
                self.session.input(bytes);
                return;
            }
        }
        self.selecting = true;
        self.session
            .terminal
            .lock()
            .select_start(col, row, event.click_count == 2);
        cx.notify();
    }
    fn mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        let (col, row) = self.cell_at(event.position);
        if self.click_start.is_some_and(|start| {
            (event.position.x - start.x).abs() > px(3.)
                || (event.position.y - start.y).abs() > px(3.)
        }) {
            self.click_moved = true;
        }
        if self.selecting && event.pressed_button != Some(MouseButton::Left) {
            self.selecting = false;
        }
        if self.selecting {
            self.session.terminal.lock().select_to(col, row);
            cx.notify();
        } else if event.pressed_button == Some(MouseButton::Left) {
            let mode = *self.session.terminal.lock().term.mode();
            if mode.intersects(TermMode::MOUSE_DRAG | TermMode::MOUSE_MOTION) {
                if let Some(bytes) = self.mouse_sequence(32, col, row, false) {
                    self.session.input(bytes);
                }
            }
        }
    }
    /// A click moves the real Shell caret only inside a verified command; a drag still selects.
    fn mouse_up(&mut self, event: &MouseUpEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let (col, row) = self.cell_at(event.position);
        if self.mouse_reporting {
            if let Some(bytes) = self.mouse_sequence(0, col, row, true) {
                self.session.input(bytes);
            }
        } else if self.connected
            && self.preedit.is_empty()
            && !event.modifiers.shift
            && !self.click_moved
            && self.click_start.is_some()
            && self.bounds.contains(&event.position)
        {
            let movement = self.session.terminal.lock().cursor_movement(col, row);
            if let Some(bytes) = movement {
                self.session.terminal.lock().clear_selection();
                self.session.input(bytes);
            }
        }
        self.mouse_reporting = false;
        self.selecting = false;
        self.click_start = None;
        cx.notify();
    }
    /// Expose composition only to the explicitly enabled isolated debug driver.
    #[cfg(debug_assertions)]
    pub(super) fn qa_preedit(&self) -> &str {
        &self.preedit
    }
    /// Shared entry for the isolated native driver; identical target routing to physical clicks.
    #[cfg(debug_assertions)]
    pub(super) fn click_cell(
        &mut self,
        col: usize,
        row: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let position = self.bounds.origin
            + point(
                self.cell_width * (col as f32 + 0.25),
                self.line_height * (row as f32 + 0.5),
            );
        self.mouse_down(
            &MouseDownEvent {
                button: MouseButton::Left,
                position,
                click_count: 1,
                ..Default::default()
            },
            window,
            cx,
        );
        self.mouse_up(
            &MouseUpEvent {
                button: MouseButton::Left,
                position,
                click_count: 1,
                ..Default::default()
            },
            window,
            cx,
        );
    }
}

/// Resolve all standard ANSI/indexed colors against MantaSH's active light/dark palette.
fn ansi_color(color: Color, palette: Palette, theme: Theme) -> Hsla {
    let base = if theme == Theme::Day {
        [
            0x242c3a, 0xb03036, 0x287447, 0x866214, 0x285ab5, 0x86469b, 0x187380, 0x626f80,
            0x626f80, 0xc93642, 0x338351, 0x936d13, 0x326bd0, 0x9953b0, 0x16808e, 0xd1d8e3,
        ]
    } else {
        [
            0x242c3a, 0xeb817f, 0x8acc95, 0xe0bd74, 0x8daff0, 0xcaa0e0, 0x79c8cc, 0xdce3ed,
            0xa2aec0, 0xffa39a, 0x9edbab, 0xecd397, 0xaac6fa, 0xdeb9ec, 0x9cdddc, 0xf5f7fb,
        ]
    };
    match color {
        Color::Spec(c) => rgb(((c.r as u32) << 16) | ((c.g as u32) << 8) | c.b as u32).into(),
        Color::Named(
            NamedColor::Foreground | NamedColor::BrightForeground | NamedColor::DimForeground,
        ) => palette.text,
        Color::Named(NamedColor::Background) => palette.terminal,
        Color::Named(c) => rgb(base[(c as usize).min(15)]).into(),
        Color::Indexed(i) if i < 16 => rgb(base[i as usize]).into(),
        Color::Indexed(i) if i >= 232 => {
            let n = 8 + (i as u32 - 232) * 10;
            rgb((n << 16) | (n << 8) | n).into()
        }
        Color::Indexed(i) => {
            let n = i as u32 - 16;
            let component = |x| if x == 0 { 0 } else { 55 + 40 * x };
            rgb((component(n / 36) << 16) | (component(n / 6 % 6) << 8) | component(n % 6)).into()
        }
    }
}

impl Render for TerminalView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        let measure = entity.clone();
        let paint = entity.clone();
        let p = Palette::new(self.preferences.theme);
        div()
            .size_full()
            .min_w_0()
            .min_h_0()
            .relative()
            .bg(p.terminal)
            .track_focus(&self.focus)
            .key_context("MantaSHTerminal")
            .on_action(cx.listener(|this, _: &gpui_component::input::Copy, _, cx| this.copy(cx)))
            .on_action(
                cx.listener(|this, _: &gpui_component::input::Paste, _, cx| {
                    if let Some(text) = cx.read_from_clipboard().and_then(|i| i.text()) {
                        this.paste_text(&text, cx);
                    }
                }),
            )
            .on_action(
                cx.listener(|this, _: &gpui_component::input::SelectAll, _, cx| {
                    this.session.terminal.lock().select_all();
                    cx.notify();
                }),
            )
            .on_key_down(cx.listener(Self::key))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
            .on_mouse_move(cx.listener(Self::mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::mouse_up))
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, event: &MouseUpEvent, w, cx| {
                    this.click_start = None;
                    this.click_moved = true;
                    if this.selecting || this.mouse_reporting {
                        this.mouse_up(event, w, cx);
                    }
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, _, _, cx| {
                    if this.session.terminal.lock().selected_text().is_some() {
                        this.copy(cx);
                    } else if let Some(text) = cx.read_from_clipboard().and_then(|i| i.text()) {
                        this.paste_text(&text, cx);
                    }
                }),
            )
            .on_scroll_wheel(cx.listener(|this, e: &ScrollWheelEvent, _, cx| {
                let delta = match e.delta {
                    ScrollDelta::Lines(p) => p.y,
                    ScrollDelta::Pixels(p) => p.y / this.line_height,
                };
                let (col, row) = this.cell_at(e.position);
                let mode = *this.session.terminal.lock().term.mode();
                let reporting = !e.modifiers.shift && mode.intersects(TermMode::MOUSE_MODE);
                let alternate = !e.modifiers.shift
                    && !reporting
                    && mode.contains(TermMode::ALT_SCREEN | TermMode::ALTERNATE_SCROLL);
                let route = if reporting {
                    1
                } else if alternate {
                    2
                } else {
                    0
                };
                let lines = this.scroll.lines(delta as f64, route);
                if lines == 0 {
                    return;
                }
                if reporting {
                    if let Some(bytes) =
                        this.mouse_sequence(if lines > 0 { 64 } else { 65 }, col, row, false)
                    {
                        for _ in 0..lines.unsigned_abs() {
                            this.session.input(bytes.clone());
                        }
                    }
                } else if alternate {
                    if let Some(bytes) = terminal::key_bytes(
                        if lines > 0 { "up" } else { "down" },
                        false,
                        false,
                        false,
                        mode,
                    ) {
                        for _ in 0..lines.unsigned_abs() {
                            this.session.input(bytes.clone());
                        }
                    }
                } else {
                    this.session.terminal.lock().scroll(lines);
                    cx.notify();
                }
                cx.stop_propagation();
            }))
            .child(
                canvas(
                    move |bounds, window, cx| {
                        measure.update(cx, |view, _| {
                            let font_key = (
                                view.preferences.terminal_font.clone(),
                                view.preferences.terminal_size,
                            );
                            if view.measured_font.as_ref() != Some(&font_key) {
                                let font = font(font_key.0.clone());
                                let id = window.text_system().resolve_font(&font);
                                let size = px(font_key.1);
                                view.cell_width = window
                                    .text_system()
                                    .advance(id, size, 'M')
                                    .map(|s| s.width)
                                    .unwrap_or(size * 0.6)
                                    .max(px(1.));
                                let measured = window.text_system().ascent(id, size)
                                    + window.text_system().descent(id, size).abs();
                                view.line_height = (size * 1.5).max(measured);
                                view.measured_font = Some(font_key);
                            }
                            view.bounds = bounds;
                            let grid = (
                                (bounds.size.width / view.cell_width).floor() as usize,
                                (bounds.size.height / view.line_height).floor() as usize,
                            );
                            if view.measured_grid != Some(grid) {
                                view.session.resize(grid.0, grid.1);
                                view.measured_grid = Some(grid);
                            }
                        });
                    },
                    move |bounds, _, window, cx| {
                        #[cfg(debug_assertions)]
                        let started = std::time::Instant::now();
                        // Clear the retained canvas on every layout frame. A
                        // zoom can change the bounds before the PTY worker has
                        // published the matching grid, and leaving old glyphs
                        // in the backing surface is visible as scattered `$`
                        // cells after the window returns to its old size.
                        let background = Palette::new(paint.read(cx).preferences.theme).terminal;
                        window.paint_quad(fill(bounds, background));
                        let Some(prepared) = paint.update(cx, |view, _| view.prepare_paint(window))
                        else {
                            return;
                        };
                        let view = paint.read(cx);
                        let preferences = view.preferences.clone();
                        let focus = view.focus.clone();
                        let cell_width = view.cell_width;
                        let line_height = view.line_height;
                        let preedit = view.preedit.clone();
                        let p = Palette::new(preferences.theme);
                        let font_size = px(preferences.terminal_size);
                        let mut cursor_bounds =
                            Bounds::new(bounds.origin, size(cell_width, line_height));
                        if let Some((row, col, _)) = prepared.cursor {
                            cursor_bounds.origin = bounds.origin
                                + point(cell_width * col as f32, line_height * row as f32);
                        }
                        window.with_content_mask(Some(ContentMask { bounds }), |window| {
                            let clip = window.content_mask().bounds;
                            for (row_index, row) in prepared.rows.iter().enumerate() {
                                let y = bounds.origin.y + line_height * row_index as f32;
                                if y + line_height < clip.top() || y > clip.bottom() {
                                    continue;
                                }
                                for run in &row.plan.backgrounds {
                                    let bg = match run.fill {
                                        Background::Color(color) => {
                                            ansi_color(color, p, preferences.theme)
                                        }
                                        Background::Selection => p.selected,
                                        Background::Match => p.accent.opacity(0.25),
                                    };
                                    if bg != p.terminal {
                                        let position = point(
                                            bounds.origin.x + cell_width * run.column as f32,
                                            y,
                                        );
                                        window.paint_quad(fill(
                                            Bounds::new(
                                                position,
                                                size(cell_width * run.width as f32, line_height),
                                            ),
                                            bg,
                                        ));
                                    }
                                }
                                for (column, width, line) in &row.text {
                                    let position =
                                        point(bounds.origin.x + cell_width * *column as f32, y);
                                    if position.x > clip.right()
                                        || position.x + line.width.max(cell_width * *width as f32)
                                            < clip.left()
                                    {
                                        continue;
                                    }
                                    let _ = line.paint(position, line_height, window, cx);
                                }
                            }
                            if focus.is_focused(window) {
                                if let Some((_, _, shape)) = prepared.cursor {
                                    let mut cursor = cursor_bounds;
                                    match shape {
                                        CursorShape::Beam => cursor.size.width = px(1.5),
                                        CursorShape::Underline => {
                                            cursor.origin.y += line_height - px(2.);
                                            cursor.size.height = px(2.);
                                        }
                                        _ => {}
                                    }
                                    window.paint_quad(fill(
                                        cursor,
                                        p.accent.opacity(if shape == CursorShape::Block {
                                            0.35
                                        } else {
                                            0.9
                                        }),
                                    ));
                                }
                                if !preedit.is_empty() {
                                    let run = TextRun {
                                        len: preedit.len(),
                                        font: font(preferences.terminal_font.clone()),
                                        color: p.text,
                                        background_color: Some(p.selected),
                                        underline: Some(UnderlineStyle {
                                            thickness: px(1.),
                                            color: Some(p.accent),
                                            wavy: false,
                                        }),
                                        strikethrough: None,
                                    };
                                    let line = window.text_system().shape_line(
                                        preedit.into(),
                                        font_size,
                                        &[run],
                                        None,
                                    );
                                    let _ =
                                        line.paint(cursor_bounds.origin, line_height, window, cx);
                                }
                            }
                        });
                        window.handle_input(
                            &focus,
                            ElementInputHandler::new(bounds, paint.clone()),
                            cx,
                        );
                        paint.update(cx, |view, _| {
                            view.cursor_bounds = cursor_bounds;
                            #[cfg(debug_assertions)]
                            {
                                let elapsed = started.elapsed().as_secs_f64() * 1000.;
                                view.paint_statistics.paints += 1;
                                view.paint_statistics.total_paint_ms += elapsed;
                                view.paint_statistics.max_paint_ms =
                                    view.paint_statistics.max_paint_ms.max(elapsed);
                            }
                        });
                    },
                )
                .size_full(),
            )
            .when_some(self.error.clone(), |d, e| {
                d.child(
                    div()
                        .absolute()
                        .bottom_0()
                        .left_0()
                        .right_0()
                        .p_2()
                        .bg(p.surface)
                        .text_color(p.error)
                        .text_size(px(self.preferences.ui_size))
                        .child(e),
                )
            })
    }
}

impl TerminalView {
    /// Always-visible position indicator matching the file panels' overlay
    /// bars, driven by the alacritty grid's history: display_offset 0 is the
    /// live bottom, history_size() the scrolled-up top. Dragging or clicking
    /// the track rewinds/fast-forwards the viewport.
    /// Bar metrics for the shell-mounted indicator: (position, thumb, track)
    /// in logical px, or None when there is no scrollback.
    pub fn scroll_metrics(&self) -> Option<(f32, f32, f32)> {
        let buffer = self.session.terminal.lock();
        let history = buffer.term.grid().history_size();
        if history == 0 {
            return None;
        }
        let display_offset = buffer.term.grid().display_offset();
        drop(buffer);
        let viewport = f32::from(self.bounds.size.height);
        let rows = (viewport / f32::from(self.line_height)).max(1.);
        let total = history as f32 + rows;
        let track = (viewport - 4.).max(1.);
        let thumb = (rows / total * track).clamp(12f32.min(track), track);
        let scroll_top = history as f32 - display_offset as f32;
        let position = (scroll_top / history as f32 * (track - thumb)).clamp(0., track - thumb);
        Some((position, thumb, track))
    }

    /// Point the viewport at the history line under a window-space y position
    /// (used by the shell-mounted scrollbar).
    pub fn scrollbar_jump(&mut self, y: Pixels, cx: &mut Context<Self>) {
        self.scroll_to(y, cx);
    }

    #[allow(dead_code)]
    fn render_scrollbar(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let p = Palette::new(self.preferences.theme);
        let (display_offset, history) = {
            let buffer = self.session.terminal.lock();
            (
                buffer.term.grid().display_offset(),
                buffer.term.grid().history_size(),
            )
        };
        if history == 0 {
            return div().into_any_element();
        }
        let viewport = f32::from(self.bounds.size.height);
        let rows = (viewport / f32::from(self.line_height)).max(1.);
        let total = history as f32 + rows;
        let track = (viewport - 4.).max(1.);
        let thumb = (rows / total * track).clamp(12f32.min(track), track);
        // scrollTop (0 = top of history) ↔ display_offset = history - scrollTop.
        let scroll_top = history as f32 - display_offset as f32;
        let position = (scroll_top / history as f32 * (track - thumb)).clamp(0., track - thumb);
        div()
            .id("terminal-scrollbar")
            .absolute()
            .top(px(2.))
            .bottom(px(2.))
            .right(px(1.))
            .w(px(10.))
            .flex_shrink_0()
            .occlude()
            .cursor_pointer()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                    this.scroll_dragging = true;
                    this.scroll_to(event.position.y, cx);
                    cx.notify();
                }),
            )
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                if this.scroll_dragging && event.pressed_button == Some(MouseButton::Left) {
                    this.scroll_to(event.position.y, cx);
                    cx.notify();
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.scroll_dragging = false;
                    cx.notify();
                }),
            )
            .child(
                div()
                    .absolute()
                    .top(px(2. + position))
                    .left(px(2.))
                    .w(px(6.))
                    .h(px(thumb))
                    .rounded(px(3.))
                    .bg(p.muted.opacity(0.55))
                    .hover(|style| style.bg(p.muted)),
            )
            .into_any_element()
    }

    /// Point the viewport at the history line under a window-space y position.
    fn scroll_to(&mut self, y: Pixels, _cx: &mut Context<Self>) {
        let viewport = f32::from(self.bounds.size.height);
        let rows = (viewport / f32::from(self.line_height)).max(1.);
        let (display_offset, history) = {
            let buffer = self.session.terminal.lock();
            (
                buffer.term.grid().display_offset(),
                buffer.term.grid().history_size(),
            )
        };
        if history == 0 {
            return;
        }
        let total = history as f32 + rows;
        let track = (viewport - 4.).max(1.);
        let thumb = (rows / total * track).clamp(12f32.min(track), track);
        let local = (f32::from(y - self.bounds.top()) - 2. - thumb / 2.).clamp(0., track - thumb);
        let target_top = local / (track - thumb).max(1.) * history as f32;
        let target_offset = (history as f32 - target_top).round() as i32;
        let delta = target_offset - display_offset as i32;
        if delta != 0 {
            self.session.terminal.lock().scroll(delta);
        }
    }
}

impl EntityInputHandler for TerminalView {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        actual: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let utf16: Vec<u16> = self.preedit.encode_utf16().collect();
        let start = range.start.min(utf16.len());
        let end = range.end.min(utf16.len()).max(start);
        *actual = Some(start..end);
        String::from_utf16(&utf16[start..end]).ok()
    }
    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.preedit_selection.clone(),
            reversed: false,
        })
    }
    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        (!self.preedit.is_empty()).then(|| 0..self.preedit.encode_utf16().count())
    }
    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.preedit.clear();
        self.preedit_selection = 0..0;
        cx.notify();
    }
    fn replace_text_in_range(
        &mut self,
        _: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.preedit.clear();
        self.preedit_selection = 0..0;
        self.type_text(text, cx);
    }
    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        selection: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut utf16: Vec<u16> = self.preedit.encode_utf16().collect();
        let range = range.unwrap_or(0..utf16.len());
        let start = range.start.min(utf16.len());
        let end = range.end.min(utf16.len()).max(start);
        utf16.splice(start..end, text.encode_utf16());
        self.preedit = String::from_utf16_lossy(&utf16);
        let len = self.preedit.encode_utf16().count();
        self.preedit_selection = selection.unwrap_or(len..len);
        cx.notify();
    }
    fn bounds_for_range(
        &mut self,
        _: Range<usize>,
        _: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        Some(self.cursor_bounds)
    }
    fn character_index_for_point(
        &mut self,
        _: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        Some(self.preedit.encode_utf16().count())
    }
}
