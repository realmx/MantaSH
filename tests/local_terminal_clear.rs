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
fn local_scroll_boundaries_and_resize_history_are_real_content_only() {
    let mut short = TerminalBuffer::new_local(Encoding::Utf8);
    short.resize_local(40, 4);
    short.feed(b"one\r\ntwo\r\nthree");
    assert_eq!(short.term.grid().history_size(), 0);
    short.scroll(i32::MAX);
    assert_eq!(short.term.grid().display_offset(), 0);

    let mut exact = TerminalBuffer::new_local(Encoding::Utf8);
    exact.resize_local(40, 4);
    exact.feed(b"one\r\ntwo\r\nthree\r\nfour");
    assert_eq!(exact.term.grid().history_size(), 0);
    exact.scroll(i32::MAX);
    assert_eq!(exact.term.grid().display_offset(), 0);

    let mut overflow = TerminalBuffer::new_local(Encoding::Utf8);
    overflow.resize_local(40, 4);
    overflow.feed(b"one\r\ntwo\r\nthree\r\nfour\r\nfive");
    let history = overflow.term.grid().history_size();
    assert!(history > 0);
    overflow.scroll(i32::MAX);
    assert_eq!(overflow.term.grid().display_offset(), history);
    assert_eq!(overflow.frame().cells[0].cell.c, 'o');
    overflow.scroll_bottom();
    assert_eq!(overflow.term.grid().display_offset(), 0);
    assert!(visible_lines(&overflow).iter().any(|line| line == "five"));

    let mut resized = TerminalBuffer::new_local(Encoding::Utf8);
    resized.resize_local(40, 8);
    resized.set_shell_token("scroll-test".into());
    resized.feed(b"$ \x1b]777;mantash-cursor;scroll-test;ready\x07");
    resized.resize_local(40, 4);
    assert_eq!(resized.term.grid().history_size(), 0);
    resized.scroll(i32::MAX);
    assert_eq!(resized.term.grid().display_offset(), 0);
    let mut retained = TerminalBuffer::new_local(Encoding::Utf8);
    retained.resize_local(40, 4);
    retained.feed(b"one\r\ntwo\r\nthree\r\nfour\r\nfive");
    assert!(retained.term.grid().history_size() > 0);
    retained.resize_local(40, 3);
    assert!(retained.term.grid().history_size() > 0);
    retained.scroll(i32::MAX);
    assert_eq!(retained.frame().cells[0].cell.c, 'o');
    retained.scroll_bottom();
    assert!(visible_lines(&retained).iter().any(|line| line == "five"));
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

fn vite_refresh(rows: usize) -> Vec<u8> {
    // Vite 8.3.0 logger: console.log('\n'.repeat(rows - 2)),
    // readline.cursorTo(stdout, 0, 0), readline.clearScreenDown(stdout).
    format!("{}\x1b[1;1H\x1b[0J", "\r\n".repeat(rows - 1)).into_bytes()
}

fn visible_lines(term: &TerminalBuffer) -> Vec<String> {
    (0..term.size.rows)
        .map(|row| {
            (0..term.size.cols)
                .map(|col| term.term.grid()[Line(row as i32)][Column(col)].c)
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .filter(|line| !line.is_empty())
        .collect()
}

#[test]
fn actual_vite_refresh_reuses_blank_space_and_preserves_startup_lines() {
    let sequence = vite_refresh(20);
    for split in 0..=sequence.len() {
        let mut term = TerminalBuffer::new_local(Encoding::Utf8);
        term.resize_local(80, 20);
        term.feed(b"$ npm run dev\r\n> vite --host --port 8001\r\n\r\n");
        term.feed(&sequence[..split]);
        term.feed(&sequence[split..]);
        term.feed(b"\r\nVITE v8.3.0 ready\r\nLocal: http://localhost:8001/\r\n");
        assert_eq!(term.term.grid().history_size(), 0, "split {split}");
        assert_eq!(
            visible_lines(&term),
            [
                "$ npm run dev",
                "> vite --host --port 8001",
                "VITE v8.3.0 ready",
                "Local: http://localhost:8001/"
            ]
        );
        assert_eq!(term.term.grid().screen_lines(), 20);
        assert_eq!(term.size.rows, 20);
        term.scroll(100);
        assert_eq!(term.term.grid().display_offset(), 0);
    }
}

#[test]
fn vite_refresh_retains_all_real_output_when_combined_content_overflows() {
    let mut term = TerminalBuffer::new_local(Encoding::Utf8);
    term.resize_local(40, 8);
    let output: String = (0..18).map(|i| format!("line-{i}\r\n")).collect();
    term.feed(output.as_bytes());
    term.feed(&vite_refresh(8));
    term.feed(b"VITE ready\r\n");
    assert!(term.term.grid().history_size() > 0);
    let mut all = String::new();
    for row in -(term.term.grid().history_size() as i32)..8 {
        all.push_str(
            &(0..40)
                .map(|col| term.term.grid()[Line(row)][Column(col)].c)
                .collect::<String>(),
        );
        all.push('\n');
    }
    for i in 0..18 {
        assert!(all.contains(&format!("line-{i} ")), "missing line {i}");
    }
    term.scroll(1000);
    assert!(term.term.grid().display_offset() > 0);
    assert_eq!(term.frame().cells[0].cell.c, 'l');
    term.scroll_bottom();
    assert!(visible_lines(&term).iter().any(|line| line == "VITE ready"));
}

#[test]
fn ordinary_newlines_and_non_refresh_erases_keep_their_semantics() {
    for tail in [
        b"\x1b[0J".as_slice(),
        b"\x1b[2;1H\x1b[0J",
        b"\x1b[1;1Htext\x1b[0J",
    ] {
        let mut term = TerminalBuffer::new_local(Encoding::Utf8);
        term.resize_local(40, 8);
        term.feed(b"before\r\nsecond\r\n");
        term.feed("\r\n".repeat(7).as_bytes());
        let history = term.term.grid().history_size();
        assert!(history > 0);
        term.feed(tail);
        assert_eq!(term.term.grid().history_size(), history);
    }
    let mut remote = TerminalBuffer::new(Encoding::Utf8);
    remote.resize(40, 8);
    remote.feed(b"before\r\nsecond\r\n");
    remote.feed(&vite_refresh(8));
    assert!(remote.term.grid().history_size() > 0);
}
