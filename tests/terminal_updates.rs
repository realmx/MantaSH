//! Regression cases for event-driven updates, damage-based snapshots and terminal input.
use alacritty_terminal::{grid::Dimensions, term::TermMode};
use mantash::{
    encoding::Encoding,
    terminal::{self, TerminalBuffer, TerminalFrame},
    terminal_io::{OutputWakeup, ScrollAccumulator},
};

/// Merge only supplied damage into a retained frame, then compare with a complete read.
fn assert_incremental(term: &mut TerminalBuffer, retained: &mut TerminalFrame) {
    let update = term.take_frame_update(false);
    if update.frame.size != retained.size {
        assert_eq!(update.rows.len(), update.frame.size.rows);
        *retained = term.frame();
    } else {
        for cell in update.frame.cells {
            let index = cell.row * retained.size.cols + cell.col;
            retained.cells[index] = cell;
        }
        retained.cursor = update.frame.cursor;
    }
    let actual = term.frame();
    assert_eq!(retained.cursor, actual.cursor);
    for (a, b) in retained.cells.iter().zip(&actual.cells) {
        assert_eq!(
            (a.row, a.col, &a.cell, a.selected, a.matched),
            (b.row, b.col, &b.cell, b.selected, b.matched)
        );
    }
}

#[test]
fn a_small_edit_copies_only_affected_rows() {
    let mut term = TerminalBuffer::new(Encoding::Utf8);
    term.resize(200, 80);
    assert_eq!(term.take_frame_update(true).frame.cells.len(), 16_000);
    term.feed(b"small edit");
    let update = term.take_frame_update(false);
    assert_eq!(update.rows, vec![0]);
    assert_eq!(update.frame.cells.len(), 200);
    term.feed(b"\x1b[4;1Hline four");
    let update = term.take_frame_update(false);
    assert_eq!(update.rows, vec![0, 3]);
    assert_eq!(update.frame.cells.len(), 400);
}

#[test]
fn incremental_snapshots_match_full_terminal_across_tui_operations() {
    let mut term = TerminalBuffer::new(Encoding::Utf8);
    term.resize(20, 6);
    let mut retained = term.take_frame_update(true).frame;
    for bytes in [
        "first\r\n中文 e\u{301}😀",
        "\x1b[2;4H\x1b[31mRED\x1b[0m",
        "\x1b[1;1H\x1b[2@XY",
        "\x1b[2;1H\x1b[K",
        "\x1b[2;5r\x1b[5;1H\n\n",
        "\x1b[r\x1b[?1049h\x1b[2J\x1b[Hvim",
        "\x1b[?25l",
        "\x1b[?25h\x1b[?1049l",
        "\x1b[Hone\r\ntwo\r\nthree\r\nfour\r\nfive\r\nsix\r\nseven\r\n",
    ] {
        term.feed(bytes.as_bytes());
        assert_incremental(&mut term, &mut retained);
    }
    term.scroll(3);
    assert_incremental(&mut term, &mut retained);
    term.scroll_bottom();
    assert_incremental(&mut term, &mut retained);
    term.select_start(0, 0, false);
    term.select_to(8, 1);
    assert_incremental(&mut term, &mut retained);
    term.feed(b"\x1b[2;1Hoverwrite selection\x1b[K");
    assert_incremental(&mut term, &mut retained);
    term.search("six");
    assert_incremental(&mut term, &mut retained);
    term.search("");
    assert_incremental(&mut term, &mut retained);
    term.resize(25, 8);
    assert_incremental(&mut term, &mut retained);
}

#[test]
fn clear_does_not_resurrect_old_prompts_on_resize() {
    let mut term = TerminalBuffer::new(Encoding::Utf8);
    term.feed(b"$ previous\r\n$ clear\r\n");
    // macOS clear sends saved-history erase before home and viewport erase.
    // Split the sequence as a PTY reader is allowed to do.
    term.feed(b"\x1b[3J\x1b[");
    term.feed(b"H\x1b[2J$ current");
    assert_eq!(term.term.grid().history_size(), 0);
    term.resize(160, 42);
    let frame = term.frame();
    let visible = (0..frame.size.rows)
        .map(|row| {
            frame
                .cells
                .iter()
                .filter(|cell| cell.row == row)
                .map(|cell| cell.cell.c)
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    assert_eq!(visible, ["$ current"]);
}

#[test]
fn independent_clear_codes_preserve_new_scrollback() {
    let mut term = TerminalBuffer::new(Encoding::Utf8);
    term.feed(b"old\r\n");
    term.feed(b"\x1b[3J");
    term.feed(b"new\r\n");
    term.feed(b"\x1b[H\x1b[2J");
    assert!(term.term.grid().history_size() > 0);

    let mut term = TerminalBuffer::new(Encoding::Utf8);
    term.feed(b"old\r\n\x1b[H\x1b[2J");
    assert!(term.term.grid().history_size() > 0);
}

#[test]
fn clear_from_scrollback_repaints_the_live_viewport() {
    let mut term = TerminalBuffer::new(Encoding::Utf8);
    term.resize(20, 3);
    term.feed(b"one\r\ntwo\r\nthree\r\nfour\r\n");
    let mut retained = term.take_frame_update(true).frame;
    term.scroll(2);
    assert!(term.term.grid().display_offset() > 0);
    assert_incremental(&mut term, &mut retained);

    term.feed(b"\x1b[3J\x1b[H\x1b[2J$ current");
    assert_eq!(term.term.grid().display_offset(), 0);
    assert_eq!(term.term.grid().history_size(), 0);
    assert_incremental(&mut term, &mut retained);
    term.resize(40, 6);
    assert_incremental(&mut term, &mut retained);
}

#[test]
fn local_resize_keeps_wrapped_command_text_in_scrollback() {
    let mut term = TerminalBuffer::new(Encoding::Utf8);
    term.resize_local(282, 2);
    term.set_shell_token("test-session".into());
    term.feed(b"$ \x1b]777;mantash-cursor;test-session;ready\x07");
    assert!(term.command_cursor.editing());
    term.input_sent(b"x");
    term.feed("x".repeat(260).as_bytes());
    // A prompt redraw during editing must not make the line look untouched.
    term.feed(b"\x1b]777;mantash-cursor;test-session;ready\x07");
    assert!(term.command_cursor.editing());
    assert_eq!(term.term.grid().history_size(), 0);
    term.resize_local(174, 1);
    assert!(
        term.term.grid().history_size() > 0,
        "resize discarded the edited command"
    );
}

#[test]
fn local_resize_keeps_output_below_an_untouched_prompt() {
    let mut term = TerminalBuffer::new(Encoding::Utf8);
    term.resize_local(20, 3);
    term.set_shell_token("test-session".into());
    term.feed(b"$ \x1b]777;mantash-cursor;test-session;ready\x07");
    term.feed(b"\x1b[19Gz\x1b[2;1Hreal output\x1b[3;1Hmore output\x1b[1;3H");
    assert!(term.command_cursor.editing());
    assert_eq!(term.term.grid().history_size(), 0);
    term.resize_local(10, 3);
    assert!(
        term.term.grid().history_size() > 0,
        "resize discarded output below the prompt"
    );
    assert!(term.search("real") > 0);
}

#[test]
fn unpainted_output_is_coalesced_and_rearmed_after_acknowledgement() {
    let wake = OutputWakeup::default();
    let count = std::sync::atomic::AtomicUsize::new(0);
    std::thread::scope(|scope| {
        for _ in 0..8 {
            scope.spawn(|| {
                for _ in 0..1000 {
                    if wake.mark() {
                        count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    }
                }
            });
        }
    });
    assert_eq!(count.load(std::sync::atomic::Ordering::Relaxed), 1);
    assert!(wake.pending());
    wake.acknowledge();
    assert!(!wake.pending());
    assert!(wake.mark());
    assert!(!wake.mark());
}

#[test]
fn fractional_scroll_is_not_lost_or_rounded_into_overscroll() {
    let mut scroll = ScrollAccumulator::default();
    for _ in 0..9 {
        assert_eq!(scroll.lines(0.1, 0), 0);
    }
    assert_eq!(scroll.lines(0., 0), 0);
    assert_eq!(scroll.lines(0.1, 0), 1);
    assert_eq!(scroll.lines(-0.4, 0), 0);
    assert_eq!(scroll.lines(-0.6, 0), -1);
    assert_eq!(scroll.lines(0.9, 0), 0);
    assert_eq!(scroll.lines(0.2, 1), 0);
    assert_eq!(scroll.lines(0.8, 1), 1);
    assert_eq!(scroll.lines(f64::NAN, 0), 0);
    assert_eq!(scroll.lines(-2., 2), -2);
}

#[test]
fn modified_function_keys_and_control_aliases_keep_their_vt_meaning() {
    let mode = TermMode::empty();
    assert_eq!(
        terminal::key_bytes("f1", false, false, false, mode),
        Some(b"\x1bOP".to_vec())
    );
    assert_eq!(
        terminal::key_bytes("f1", true, false, false, mode),
        Some(b"\x1b[1;5P".to_vec())
    );
    assert_eq!(
        terminal::key_bytes("f4", false, true, true, mode),
        Some(b"\x1b[1;4S".to_vec())
    );
    for (key, byte) in [
        ("2", 0),
        ("3", 27),
        ("4", 28),
        ("5", 29),
        ("6", 30),
        ("7", 31),
        ("8", 127),
        ("?", 127),
        ("c", 3),
    ] {
        assert_eq!(
            terminal::key_bytes(key, true, false, false, mode),
            Some(vec![byte])
        );
    }
    assert!(terminal::key_bytes("1", true, false, false, mode).is_none());
}

/// A real PTY keeps parsing bytes while its unpainted notification remains coalesced.
#[cfg(unix)]
#[test]
fn actual_pty_output_wakes_without_periodic_ui_polling() {
    use mantash::{events::Event, model::*, services::Backend};
    use std::time::{Duration, Instant};
    let dir = tempfile::tempdir().unwrap();
    let (backend, events, _, _) = Backend::initialize_in(dir.path().into()).unwrap();
    let owner = Owner::new();
    let session = backend.start(
        owner,
        SessionSpec::Local {
            shell: "/bin/sh".into(),
            directory: dir.path().display().to_string(),
            encoding: Encoding::Utf8,
        },
        zeroize::Zeroizing::new(String::new()),
        false,
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut connected = false;
    while Instant::now() < deadline {
        match events.try_recv() {
            Ok(Event::State(o, ConnectionState::Connected)) if o == owner => {
                connected = true;
                break;
            }
            Ok(Event::State(_, ConnectionState::Failed(error))) => panic!("{error}"),
            _ => std::thread::sleep(Duration::from_millis(5)),
        }
    }
    assert!(connected, "PTY did not connect before the deadline");
    session.input(b"printf 'WAKE_%s\\n' COMPLETE\r".to_vec());
    let mut wakeups = 0;
    let mut complete = false;
    while Instant::now() < deadline {
        while let Ok(event) = events.try_recv() {
            if matches!(event,Event::Output(o) if o==owner) {
                wakeups += 1;
            }
        }
        let text: String = session
            .terminal
            .lock()
            .frame()
            .cells
            .iter()
            .map(|c| c.cell.c)
            .collect();
        if text.contains("WAKE_COMPLETE") {
            complete = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(complete);
    assert_eq!(wakeups, 1);
    {
        let mut term = session.terminal.lock();
        term.take_frame_update(true);
        session.output_wakeup.acknowledge();
    }
    assert!(!session.output_wakeup.pending());
    backend.close(owner);
    backend.shutdown();
}
