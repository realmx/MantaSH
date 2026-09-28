//! Sparse rendering must preserve terminal geometry and visual attributes.
use alacritty_terminal::{
    term::cell::Flags,
    vte::ansi::{Color, NamedColor},
};
use mantash::{
    encoding::Encoding,
    terminal::TerminalBuffer,
    terminal_paint::{self, Background},
};

fn terminal(text: &str) -> TerminalBuffer {
    let mut term = TerminalBuffer::new(Encoding::Utf8);
    term.feed(text.as_bytes());
    term
}

#[test]
fn blank_screen_has_no_per_cell_draw_work() {
    let term = terminal("");
    let rows = terminal_paint::rows(&term.frame());
    assert_eq!(rows.len(), 24);
    assert!(
        rows.iter()
            .all(|row| row.text.is_empty() && row.backgrounds.is_empty())
    );
}

#[test]
fn adjacent_ascii_is_batched_without_shifting_spaces() {
    let term = terminal("abcd  ef");
    let rows = terminal_paint::rows(&term.frame());
    assert_eq!(rows[0].text.len(), 2);
    assert_eq!(
        (
            &*rows[0].text[0].text,
            rows[0].text[0].column,
            rows[0].text[0].width
        ),
        ("abcd", 0, 4)
    );
    assert_eq!((&*rows[0].text[1].text, rows[0].text[1].column), ("ef", 6));
    assert!(rows[0].backgrounds.is_empty());
}

#[test]
fn colored_blank_backgrounds_are_merged_and_not_dropped() {
    let term = terminal("\x1b[41m       \x1b[0m ");
    let rows = terminal_paint::rows(&term.frame());
    assert!(rows[0].text.is_empty());
    assert_eq!(rows[0].backgrounds.len(), 1);
    let run = &rows[0].backgrounds[0];
    assert_eq!(run.column, 0);
    assert_eq!(run.width, 7);
    assert_eq!(run.fill, Background::Color(Color::Named(NamedColor::Red)));
}

#[test]
fn wide_and_combining_characters_stay_in_their_original_cells() {
    let term = terminal("a中e\u{301}😀z");
    let rows = terminal_paint::rows(&term.frame());
    let spans = &rows[0].text;
    assert_eq!(
        spans
            .iter()
            .map(|s| (s.column, s.width, s.text.as_str()))
            .collect::<Vec<_>>(),
        vec![
            (0, 1, "a"),
            (1, 2, "中"),
            (3, 1, "e\u{301}"),
            (4, 2, "😀"),
            (6, 1, "z")
        ]
    );
    assert!(!spans[1].ascii && !spans[2].ascii && !spans[3].ascii);
}

#[test]
fn style_changes_inverse_hidden_and_decorated_spaces_preserve_rendering() {
    let term = terminal("a\x1b[1mb\x1b[0;7m \x1b[0;4m \x1b[0;8mx");
    let rows = terminal_paint::rows(&term.frame());
    let text = &rows[0].text;
    assert_eq!(text.len(), 3);
    assert!(text[1].flags.contains(Flags::BOLD));
    assert_eq!((text[2].column, text[2].text.as_str()), (3, " "));
    assert!(text[2].flags.contains(Flags::UNDERLINE));
    assert_eq!(
        rows[0].backgrounds[0].fill,
        Background::Color(Color::Named(NamedColor::Foreground))
    );
    assert_eq!(rows[0].backgrounds[0].column, 2);
}

#[test]
fn selections_and_search_highlight_blank_cells_and_invalidate_cached_frames() {
    let mut term = terminal("ab  cd");
    let revision = term.revision;
    term.select_start(2, 0, false);
    term.select_to(3, 0);
    assert!(term.revision > revision);
    let rows = terminal_paint::rows(&term.frame());
    assert!(
        rows[0]
            .backgrounds
            .iter()
            .any(|b| b.fill == Background::Selection)
    );
    let revision = term.revision;
    term.search("cd");
    assert!(term.revision > revision);
    let rows = terminal_paint::rows(&term.frame());
    assert!(
        rows[0]
            .backgrounds
            .iter()
            .any(|b| b.fill == Background::Match)
    );
    let revision = term.revision;
    term.search("");
    assert!(term.revision > revision);
    assert!(
        !terminal_paint::rows(&term.frame())[0]
            .backgrounds
            .iter()
            .any(|b| b.fill == Background::Match)
    );
}

#[test]
fn scrolling_and_cursor_changes_preserve_the_grid_and_cursor() {
    let mut term = terminal("");
    term.resize(20, 3);
    term.feed(b"one\r\ntwo\r\nthree\r\nfour\r\n");
    let live = term.frame();
    term.scroll(2);
    let old_revision = term.revision;
    assert_ne!(term.frame().cells[0].cell.c, live.cells[0].cell.c);
    term.scroll_bottom();
    assert!(term.revision > old_revision);
    let first = terminal_paint::rows(&term.frame());
    term.feed(b"\x1b[1;1H");
    assert_eq!(terminal_paint::rows(&term.frame()), first);
    assert_eq!(
        term.frame().cursor.map(|(row, col, _)| (row, col)),
        Some((0, 0))
    );
}
