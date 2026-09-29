//! Local npm-style viewport erases must not manufacture scrollback.
use alacritty_terminal::{
    grid::Dimensions,
    index::{Column, Line, Point},
    term::TermMode,
};
use mantash::{encoding::Encoding, terminal::TerminalBuffer};

#[test]
fn short_output_and_split_npm_clear_do_not_create_history() {
    let clear = b"\x1b[H\x1b[2J";
    for split in 0..=clear.len() {
        let mut term = TerminalBuffer::new_local(Encoding::Utf8);
        term.resize_local(100, 41);
        term.feed(b"$ npm run dev\r\n> project dev\r\n> vite\r\n");
        assert_eq!(term.term.grid().history_size(), 0);
        term.feed(&clear[..split]);
        term.feed(&clear[split..]);
        term.feed(b"Server ready\r\n");
        assert_eq!(term.term.grid().history_size(), 0, "split={split}");
        assert_eq!(term.term.grid()[Line(0)][Column(0)].c, 'S');
        term.scroll(100);
        assert_eq!(term.term.grid().display_offset(), 0);
        term.resize_local(120, 50);
        assert_eq!(term.term.grid().history_size(), 0);
    }
}

#[test]
fn real_overflow_remains_scrollable_before_and_after_viewport_erase() {
    let mut term = TerminalBuffer::new_local(Encoding::Utf8);
    term.resize_local(40, 4);
    term.feed(b"one\r\ntwo\r\nthree\r\nfour");
    assert_eq!(term.term.grid().history_size(), 0);
    term.feed(b"\r\nfive\r\nsix");
    assert_eq!(term.term.grid().history_size(), 2);
    term.scroll(100);
    assert_eq!(term.term.grid().display_offset(), 2);
    assert_eq!(term.frame().cells[0].cell.c, 'o');
    term.scroll_bottom();
    term.feed(b"\x1b[H\x1b[2Jnew");
    assert_eq!(term.term.grid().history_size(), 2);
    term.scroll(100);
    assert_eq!(term.term.grid().display_offset(), 2);
    assert_eq!(term.frame().cells[0].cell.c, 'o');
    term.scroll_bottom();
    term.feed(b"\x1b[3J\x1b[H\x1b[2Jclean");
    assert_eq!(term.term.grid().history_size(), 0);
}

#[test]
fn viewport_erase_keeps_cursor_background_and_repaints_all_rows() {
    let mut term = TerminalBuffer::new_local(Encoding::Utf8);
    term.resize_local(20, 5);
    term.feed(b"old\r\noutput\x1b[4;7H\x1b[44m");
    let bg = term.term.grid().cursor.template.bg;
    let cursor = term.term.grid().cursor.point;
    term.take_frame_update(true);
    for byte in b"\x1b[2J" {
        term.feed(&[*byte]);
    }
    assert_eq!(term.term.grid().cursor.point, cursor);
    assert_eq!(cursor, Point::new(Line(3), Column(6)));
    assert_eq!(term.term.grid().history_size(), 0);
    let frame = term.take_frame_update(false);
    assert_eq!(frame.rows.len(), 5);
    assert!(
        frame
            .frame
            .cells
            .iter()
            .all(|c| c.cell.c == ' ' && c.cell.bg == bg)
    );
    term.feed(b"X");
    assert_eq!(term.term.grid()[Line(3)][Column(6)].c, 'X');
}

#[test]
fn alternate_screen_and_remote_clear_behavior_are_preserved() {
    let mut local = TerminalBuffer::new_local(Encoding::Utf8);
    local.feed(b"main\x1b[?1049halt\x1b[2J");
    assert!(local.term.mode().contains(TermMode::ALT_SCREEN));
    assert_eq!(local.term.grid().history_size(), 0);
    local.feed(b"\x1b[?1049l");
    assert_eq!(local.term.grid()[Line(0)][Column(0)].c, 'm');
    let mut remote = TerminalBuffer::new(Encoding::Utf8);
    remote.feed(b"previous\r\n\x1b[H\x1b[2J");
    assert!(remote.term.grid().history_size() > 0);
}
