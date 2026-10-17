//! Explicit, language-independent cleanup; ordinary reads and saves do not use it.
use std::ops::Range;

/// Text and a UTF-8 cursor offset mapped through newline and trailing-space cleanup.
#[derive(Debug, PartialEq, Eq)]
pub struct Formatted {
    pub text: String,
    pub cursor: usize,
}

/// Text and both UTF-8 selection endpoints, mapped in a single pass.
#[derive(Debug, PartialEq, Eq)]
pub struct FormattedSelection {
    pub text: String,
    pub selection: Range<usize>,
}

pub fn format(text: &str, cursor: usize) -> Formatted {
    let formatted = format_selection(text, cursor..cursor);
    Formatted {
        text: formatted.text,
        cursor: formatted.selection.start,
    }
}

/// Keep indentation, Unicode and blank lines. Endpoints in removed whitespace clamp
/// to the surviving line end; endpoints inside CRLF move after its single LF.
pub fn format_selection(text: &str, selection: Range<usize>) -> FormattedSelection {
    let mut output = String::with_capacity(text.len() + 1);
    let offsets = [
        selection.start.min(text.len()),
        selection.end.min(text.len()),
    ];
    let mut mapped = [0; 2];
    let mut start = 0;
    while start < text.len() {
        let end = text[start..]
            .find(['\r', '\n'])
            .map_or(text.len(), |i| start + i);
        let trimmed = text[start..end].trim_end_matches([' ', '\t']);
        let base = output.len();
        output.push_str(trimmed);
        let newline_end = if end == text.len() {
            end
        } else if text.as_bytes()[end] == b'\r' && text.as_bytes().get(end + 1) == Some(&b'\n') {
            end + 2
        } else {
            end + 1
        };
        for (cursor, mapped) in offsets.iter().zip(mapped.iter_mut()) {
            if *cursor >= start && *cursor <= end {
                *mapped = base + (*cursor - start).min(trimmed.len());
            }
        }
        if newline_end > end {
            output.push('\n');
            for (cursor, mapped) in offsets.iter().zip(mapped.iter_mut()) {
                if *cursor > end && *cursor <= newline_end {
                    *mapped = output.len();
                }
            }
        }
        start = newline_end;
    }
    if !output.ends_with('\n') {
        output.push('\n');
    }
    for offset in &mut mapped {
        while !output.is_char_boundary(*offset) {
            *offset = offset.saturating_sub(1);
        }
    }
    FormattedSelection {
        text: output,
        selection: mapped[0]..mapped[1],
    }
}
