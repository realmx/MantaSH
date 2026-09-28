//! Window presets in logical pixels, independent of native APIs and terminal sessions.
use serde::{Deserialize, Serialize};

/// Minimum usable window width before the work-area limit is applied.
pub const MIN_WIDTH: f32 = 960.;
/// Minimum usable window height before the work-area limit is applied.
pub const MIN_HEIGHT: f32 = 640.;

/// Coordinates share the native adapter's current-screen coordinate system.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}
impl Rect {
    /// Reject malformed platform or user values before passing them to a window manager.
    pub fn valid(self) -> bool {
        [self.x, self.y, self.width, self.height]
            .iter()
            .all(|value| value.is_finite())
            && self.width > 0.
            && self.height > 0.
    }
}

/// A nine-grid alignment retains the current size, bounded by the usable screen.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Position {
    TopLeft,
    Top,
    TopRight,
    Left,
    Center,
    Right,
    BottomLeft,
    Bottom,
    BottomRight,
}
impl Position {
    pub const ALL: [Self; 9] = [
        Self::TopLeft,
        Self::Top,
        Self::TopRight,
        Self::Left,
        Self::Center,
        Self::Right,
        Self::BottomLeft,
        Self::Bottom,
        Self::BottomRight,
    ];
    /// Match the visual order and translation keys of the positioning grid.
    pub fn key(self) -> &'static str {
        match self {
            Self::TopLeft => "window_top_left",
            Self::Top => "window_top",
            Self::TopRight => "window_top_right",
            Self::Left => "window_left",
            Self::Center => "window_center",
            Self::Right => "window_right",
            Self::BottomLeft => "window_bottom_left",
            Self::Bottom => "window_bottom",
            Self::BottomRight => "window_bottom_right",
        }
    }
    /// Return alignment factors without assuming the display starts at (0, 0).
    fn factors(self) -> (f32, f32) {
        match self {
            Self::TopLeft => (0., 0.),
            Self::Top => (0.5, 0.),
            Self::TopRight => (1., 0.),
            Self::Left => (0., 0.5),
            Self::Center => (0.5, 0.5),
            Self::Right => (1., 0.5),
            Self::BottomLeft => (0., 1.),
            Self::Bottom => (0.5, 1.),
            Self::BottomRight => (1., 1.),
        }
    }
}

/// Every UI and native menu entry uses the same geometry command.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Command {
    Compact,
    Standard,
    Wide,
    Fill,
    Position { position: Position },
    Custom { width: f32, height: f32 },
    Restore,
}

/// Fit a requested size into the work area and align it with equal opposite margins when centered.
fn place(area: Rect, width: f32, height: f32, position: Position) -> Rect {
    let width = width.max(MIN_WIDTH).min(area.width);
    let height = height.max(MIN_HEIGHT).min(area.height);
    let (x, y) = position.factors();
    Rect {
        x: (area.x + (area.width - width) * x).round(),
        y: (area.y + (area.height - height) * y).round(),
        width: width.round(),
        height: height.round(),
    }
}

/// Compute a bounded target without changing restore history; the caller commits that only after success.
pub fn target(
    command: Command,
    current: Rect,
    area: Rect,
    restore: Option<Rect>,
) -> Result<Rect, &'static str> {
    if !current.valid() || !area.valid() {
        return Err("window_unavailable");
    }
    let centered = |w, h| place(area, w, h, Position::Center);
    Ok(match command {
        Command::Compact => centered(960., 640.),
        Command::Standard => centered(1280., 800.),
        Command::Wide => centered(1440., 900.),
        Command::Fill => area,
        Command::Position { position } => place(area, current.width, current.height, position),
        Command::Custom { width, height } => {
            if !width.is_finite()
                || !height.is_finite()
                || width < MIN_WIDTH
                || height < MIN_HEIGHT
                || width > 16000.
                || height > 16000.
            {
                return Err("window_invalid_size");
            }
            centered(width, height)
        }
        Command::Restore => {
            let saved = restore
                .filter(|value| value.valid())
                .ok_or("window_no_restore")?;
            let size = centered(saved.width, saved.height);
            Rect {
                x: saved.x.clamp(area.x, area.x + area.width - size.width),
                y: saved.y.clamp(area.y, area.y + area.height - size.height),
                ..size
            }
        }
    })
}
