//! Convert a terminal grid to sparse paint runs without changing terminal coordinates.
use crate::terminal::TerminalFrame;
use alacritty_terminal::{
    term::cell::Flags,
    vte::ansi::{Color, NamedColor},
};

/// Semantic fills remain theme-independent until the desktop renderer resolves them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Background {
    Color(Color),
    Selection,
    Match,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackgroundRun {
    pub column: usize,
    pub width: usize,
    pub fill: Background,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextSpan {
    pub column: usize,
    pub width: usize,
    pub text: String,
    pub foreground: Color,
    pub flags: Flags,
    /// Complex graphemes stay in their original cell; only simple ASCII is combined.
    pub ascii: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PaintRow {
    pub backgrounds: Vec<BackgroundRun>,
    pub text: Vec<TextSpan>,
}

/// Omit invisible blanks, merge adjacent backgrounds and batch safe text spans.
/// Selection and decoration on blank cells must still be painted. Wide spacers do
/// not add an extra glyph, and combining characters remain attached to their base.
pub fn rows(frame: &TerminalFrame) -> Vec<PaintRow> {
    let mut rows = vec![PaintRow::default(); frame.size.rows];
    for painted in &frame.cells {
        if painted.row >= rows.len()
            || painted
                .cell
                .flags
                .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
        {
            continue;
        }
        let row = &mut rows[painted.row];
        let cell = &painted.cell;
        let width = if cell.flags.contains(Flags::WIDE_CHAR) {
            2
        } else {
            1
        };
        let (foreground, background) = if cell.flags.contains(Flags::INVERSE) {
            (cell.bg, cell.fg)
        } else {
            (cell.fg, cell.bg)
        };
        let fill = if painted.selected {
            Some(Background::Selection)
        } else if painted.matched {
            Some(Background::Match)
        } else if background != Color::Named(NamedColor::Background) {
            Some(Background::Color(background))
        } else {
            None
        };
        if let Some(fill) = fill {
            if let Some(last) = row
                .backgrounds
                .last_mut()
                .filter(|last| last.fill == fill && last.column + last.width == painted.col)
            {
                last.width += width;
            } else {
                row.backgrounds.push(BackgroundRun {
                    column: painted.col,
                    width,
                    fill,
                });
            }
        }
        if cell.flags.contains(Flags::HIDDEN) {
            continue;
        }
        let flags = cell.flags
            & (Flags::BOLD | Flags::ITALIC | Flags::DIM | Flags::ALL_UNDERLINES | Flags::STRIKEOUT);
        let extra = cell.zerowidth().filter(|extra| !extra.is_empty());
        if cell.c == ' '
            && extra.is_none()
            && !flags.intersects(Flags::ALL_UNDERLINES | Flags::STRIKEOUT)
        {
            continue;
        }
        let ascii =
            cell.c.is_ascii() && !cell.c.is_ascii_control() && extra.is_none() && width == 1;
        if let Some(last) = row.text.last_mut().filter(|last| {
            ascii
                && last.ascii
                && last.column + last.width == painted.col
                && last.foreground == foreground
                && last.flags == flags
        }) {
            last.text.push(cell.c);
            last.width += 1;
        } else {
            let mut text = cell.c.to_string();
            if let Some(extra) = extra {
                text.extend(extra);
            }
            row.text.push(TextSpan {
                column: painted.col,
                width,
                text,
                foreground,
                flags,
                ascii,
            });
        }
    }
    rows
}
