//! Regression cases for event-driven updates, damage-based snapshots and terminal input.
use alacritty_terminal::{grid::Dimensions, term::TermMode};
use mantash::{
    encoding::Encoding,
    terminal::{self, TerminalBuffer, TerminalFrame},
    terminal_io::{
        DRAG_AUTOSCROLL_EDGE, DRAG_AUTOSCROLL_MAX_LINES, OutputWakeup, ScrollAccumulator,
        drag_autoscroll_lines,
    },
};
use parking_lot::Mutex;
use std::sync::Arc;

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

fn visible_lines(term: &TerminalBuffer) -> Vec<String> {
    let frame = term.frame();
    (0..frame.size.rows)
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
        .collect()
}

fn history_contains(term: &TerminalBuffer, needle: &str) -> bool {
    let history = term.term.grid().history_size() as i32;
    let cols = term.size.cols;
    let mut lines = Vec::new();
    for row in -history..0 {
        let line: String = (0..cols)
            .map(|col| {
                term.term.grid()[alacritty_terminal::index::Point::new(
                    alacritty_terminal::index::Line(row),
                    alacritty_terminal::index::Column(col),
                )]
                .c
            })
            .collect::<String>()
            .trim_end()
            .to_owned();
        if !line.is_empty() {
            lines.push(line);
        }
    }
    lines.iter().any(|line| line.contains(needle))
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
fn local_resize_drops_only_untouched_prompt_reflow_history() {
    for local_resize in [false, true] {
        let mut term = TerminalBuffer::new_local(Encoding::Utf8);
        term.resize_local(282, 2);
        term.set_shell_token("test-session".into());
        term.feed(b"$ \x1b]777;mantash-cursor;test-session;ready\x07");
        // Shell-owned right prompt, with the editing cursor restored to the left.
        term.feed(b"\x1b[260G[right prompt]\x1b[3G");
        assert!(term.command_cursor.editing());
        assert_eq!(term.term.grid().history_size(), 0);
        if local_resize {
            term.resize_local(174, 1);
            assert_eq!(term.term.grid().history_size(), 0);
            term.scroll(i32::MAX);
            assert_eq!(term.term.grid().display_offset(), 0);
        } else {
            term.resize(174, 1);
            assert!(term.term.grid().history_size() > 0);
        }
    }
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
    // Startup shells may emit a prompt before the test command. Consume that
    // initial notification so this assertion measures the command's wakeup.
    while events.try_recv().is_ok() {}
    {
        let mut term = session.terminal.lock();
        term.take_frame_update(true);
        session.output_wakeup.acknowledge();
    }
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

#[test]
fn drag_autoscroll_speed_follows_pointer_distance_past_an_edge() {
    let (top, bottom, line) = (100., 460., 18.);
    // Inside the viewport, away from the edges, nothing scrolls.
    assert_eq!(
        drag_autoscroll_lines(101. + DRAG_AUTOSCROLL_EDGE, top, bottom, line),
        0
    );
    assert_eq!(drag_autoscroll_lines(280., top, bottom, line), 0);
    assert_eq!(
        drag_autoscroll_lines(bottom - DRAG_AUTOSCROLL_EDGE - 1., top, bottom, line),
        0
    );
    // At or above the top edge scrolls toward history (positive), faster further out.
    assert_eq!(drag_autoscroll_lines(top, top, bottom, line), 1);
    assert_eq!(drag_autoscroll_lines(top + 1., top, bottom, line), 1);
    assert_eq!(drag_autoscroll_lines(top - 17., top, bottom, line), 1);
    assert_eq!(drag_autoscroll_lines(top - 18., top, bottom, line), 2);
    assert_eq!(drag_autoscroll_lines(top - 60., top, bottom, line), 4);
    // At or below the bottom edge scrolls toward live output (negative).
    assert_eq!(drag_autoscroll_lines(bottom - 1., top, bottom, line), -1);
    assert_eq!(drag_autoscroll_lines(bottom, top, bottom, line), -1);
    assert_eq!(drag_autoscroll_lines(bottom + 40., top, bottom, line), -3);
    // Speed is capped, and degenerate geometry never scrolls.
    assert_eq!(
        drag_autoscroll_lines(-10_000., top, bottom, line),
        DRAG_AUTOSCROLL_MAX_LINES
    );
    assert_eq!(
        drag_autoscroll_lines(10_000., top, bottom, line),
        -DRAG_AUTOSCROLL_MAX_LINES
    );
    assert_eq!(drag_autoscroll_lines(0., top, bottom, 0.), 0);
    assert_eq!(drag_autoscroll_lines(f32::NAN, top, bottom, line), 0);
    assert_eq!(drag_autoscroll_lines(0., bottom, top, line), 0);
}

#[test]
fn drag_autoscroll_extends_selection_through_history_and_stops_at_limits() {
    let mut term = TerminalBuffer::new(Encoding::Utf8);
    term.resize(20, 5);
    for line in 0..30 {
        term.feed(format!("line{line:02}\r\n").as_bytes());
    }
    let history = term.term.grid().history_size();
    assert!(history > 10);
    // Without a selection, auto-scroll is a no-op.
    assert!(!term.scroll_selection(3, 0, 0));
    assert_eq!(term.term.grid().display_offset(), 0);

    // Drag from the bottom viewport row upward past the top edge.
    term.select_start(5, 3, false);
    assert!(term.scroll_selection(3, 0, 0));
    assert_eq!(term.term.grid().display_offset(), 3);
    // The end follows the scrolled content: viewport row 0 is now line23.
    let text = term.selected_text().unwrap();
    assert!(text.starts_with("line23\nline24"), "{text:?}");
    assert_eq!(text.lines().count(), 7, "{text:?}");

    // Running into the top of history stops further scrolling.
    assert!(term.scroll_selection(1_000, 0, 0));
    assert_eq!(term.term.grid().display_offset(), history);
    assert!(!term.scroll_selection(1, 0, 0));
    let text = term.selected_text().unwrap();
    assert!(text.starts_with("line00\nline01"), "{text:?}");

    // Dragging back down past the bottom edge follows content to the live bottom.
    assert!(term.scroll_selection(-1_000, 19, 4));
    assert_eq!(term.term.grid().display_offset(), 0);
    assert!(!term.scroll_selection(-1, 19, 4));
    // The anchor stays on line29; the end is now below it on the live screen.
    let text = term.selected_text().unwrap();
    assert!(!text.contains("line28"), "{text:?}");
}

#[test]
fn reverse_drag_selection_includes_the_first_column_and_the_anchor_cell() {
    let mut term = TerminalBuffer::new(Encoding::Utf8);
    term.resize(20, 5);
    term.feed(b"[INFO] alpha\r\n[WARN] bravo\r\n#3 charlie");

    // Forward drag keeps both end cells.
    term.select_start(0, 0, false);
    term.select_to(5, 1);
    assert_eq!(term.selected_text().unwrap(), "[INFO] alpha\n[WARN]");

    // Bottom-to-top drag ending on column 0 includes the '[' there and the
    // anchor cell 'r' where the drag started.
    term.select_start(4, 2, false);
    term.select_to(0, 0);
    assert_eq!(
        term.selected_text().unwrap(),
        "[INFO] alpha\n[WARN] bravo\n#3 ch"
    );

    // Same-line right-to-left drag, and a pointer past the left edge (the view
    // clamps it to column 0).
    term.select_start(5, 1, false);
    term.select_to(0, 1);
    assert_eq!(term.selected_text().unwrap(), "[WARN]");

    // Returning to the anchor cell selects exactly that cell in both orders.
    term.select_to(5, 1);
    assert_eq!(term.selected_text().unwrap(), "]");

    // Auto-scroll clamps the end to column 0 of the top row and must keep it.
    for line in 0..30 {
        term.feed(format!("\r\n{line:02}x").as_bytes());
    }
    term.select_start(2, 4, false);
    assert!(term.scroll_selection(2, 0, 0));
    let text = term.selected_text().unwrap();
    assert_eq!(text, "23x\n24x\n25x\n26x\n27x\n28x\n29x");
}

#[test]
fn shift_click_extends_selection_from_the_original_anchor() {
    let mut term = TerminalBuffer::new(Encoding::Utf8);
    term.resize(20, 5);
    term.feed(b"[INFO] alpha\r\n[WARN] bravo\r\n#3 charlie");

    // Extending forward on the same line keeps the anchor and includes the clicked cell.
    term.select_start(7, 0, false);
    term.select_to(8, 0);
    assert_eq!(term.selected_text().unwrap(), "al");
    assert!(term.select_extend(11, 0));
    assert_eq!(term.selected_text().unwrap(), "alpha");

    // Extending backward past the anchor flips the range around the same
    // anchor cell and includes column 0.
    assert!(term.select_extend(0, 0));
    assert_eq!(term.selected_text().unwrap(), "[INFO] a");

    // Extending across lines, after and then before a selection drawn
    // bottom-to-top: the drag start (anchor 'b' of "bravo") stays fixed.
    term.select_start(7, 1, false);
    term.select_to(1, 1);
    assert_eq!(term.selected_text().unwrap(), "WARN] b");
    assert!(term.select_extend(4, 2));
    assert_eq!(term.selected_text().unwrap(), "bravo\n#3 ch");
    assert!(term.select_extend(2, 0));
    assert_eq!(term.selected_text().unwrap(), "NFO] alpha\n[WARN] b");

    // Shift+click then drag keeps moving the same end.
    term.select_to(5, 1);
    assert_eq!(term.selected_text().unwrap(), "] b");

    // Extension survives scrolling: the anchor follows its content into history.
    term.select_start(0, 2, false);
    term.select_to(1, 2);
    for line in 0..6 {
        term.feed(format!("\r\nnew{line}").as_bytes());
    }
    term.scroll(3);
    assert_eq!(term.term.grid().display_offset(), 3);
    // Viewport row 0 now shows "[WARN] bravo"; the anchor '#' is on row 1.
    assert!(term.select_extend(7, 0));
    assert_eq!(term.selected_text().unwrap(), "bravo\n#");
}

#[test]
fn shift_click_extension_keeps_word_granularity() {
    let mut term = TerminalBuffer::new(Encoding::Utf8);
    term.resize(30, 5);
    term.feed(b"one two three\r\nfour five six");

    // Double-click "two", then Shift+click inside "five" and before the anchor.
    term.select_start(5, 0, true);
    assert_eq!(term.selected_text().unwrap(), "two");
    assert!(term.select_extend(6, 1));
    assert_eq!(term.selected_text().unwrap(), "two three\nfour five");
    assert!(term.select_extend(1, 0));
    assert_eq!(term.selected_text().unwrap(), "one two");
}

#[test]
fn shift_click_without_a_visible_selection_is_a_fallback() {
    let mut term = TerminalBuffer::new(Encoding::Utf8);
    term.resize(20, 5);
    term.feed(b"hello world");

    // No selection at all.
    assert!(!term.select_extend(4, 0));
    assert!(term.selected_text().is_none());

    // A plain click without dragging leaves only an empty anchor point.
    term.select_start(2, 0, false);
    assert!(!term.select_extend(8, 0));
    assert!(term.selected_text().is_none());

    // Cleared selection (e.g. click-to-move-cursor).
    term.select_start(0, 0, false);
    term.select_to(4, 0);
    term.clear_selection();
    assert!(!term.select_extend(8, 0));
    assert!(term.selected_text().is_none());

    // The caller's fallback starts a fresh selection at the Shift+click.
    term.select_start(6, 0, false);
    term.select_to(10, 0);
    assert_eq!(term.selected_text().unwrap(), "world");
}

#[test]
fn prepare_reconnect_keeps_scrollback_and_viewport_in_history() {
    let mut term = TerminalBuffer::new(Encoding::Utf8);
    term.resize(20, 5);
    // Fill the viewport and force prior lines into scrollback.
    for i in 0..8 {
        term.feed(format!("line-{i}\r\n").as_bytes());
    }
    let history_before = term.term.grid().history_size();
    assert!(
        history_before > 0,
        "expected scrollback before reconnect prep"
    );
    assert!(
        visible_lines(&term)
            .iter()
            .any(|line| line.contains("line-7")),
        "latest line should be visible before prep: {:?}",
        visible_lines(&term)
    );

    term.select_start(0, 0, false);
    term.select_to(4, 0);
    assert!(term.selected_text().is_some());
    term.search("line");
    assert!(!term.search_hits.is_empty());

    term.prepare_reconnect();

    assert!(
        term.term.grid().history_size() > history_before,
        "viewport should be pushed into scrollback"
    );
    assert!(
        history_contains(&term, "line-7"),
        "previous screen content must remain reachable in history"
    );
    assert!(
        history_contains(&term, "line-0") || history_contains(&term, "line-1"),
        "earlier scrollback must survive prepare_reconnect"
    );
    // Viewport is blank for the new shell; selection and search overlays are cleared.
    assert!(
        visible_lines(&term).is_empty(),
        "viewport should be clear after prep: {:?}",
        visible_lines(&term)
    );
    assert!(term.selected_text().is_none());
    assert!(term.search_hits.is_empty());
    assert!(!term.term.mode().contains(TermMode::ALT_SCREEN));

    // New connection output continues on the preserved buffer.
    term.feed(b"after-reconnect\r\n");
    assert!(
        visible_lines(&term)
            .iter()
            .any(|line| line.contains("after-reconnect")),
        "{:?}",
        visible_lines(&term)
    );
    assert!(
        history_contains(&term, "line-7"),
        "pre-reconnect content must still be scrollable after new output"
    );
}

#[test]
fn prepare_reconnect_exits_alternate_screen_before_preserving() {
    let mut term = TerminalBuffer::new(Encoding::Utf8);
    term.resize(20, 5);
    term.feed(b"primary-before\r\n");
    term.feed(b"\x1b[?1049h\x1b[2J\x1b[Halt-screen\r\n");
    assert!(term.term.mode().contains(TermMode::ALT_SCREEN));

    term.prepare_reconnect();

    assert!(!term.term.mode().contains(TermMode::ALT_SCREEN));
    assert!(
        history_contains(&term, "primary-before"),
        "primary scrollback must survive after leaving the alternate screen"
    );
    assert!(
        visible_lines(&term).is_empty(),
        "viewport should be clear for the new attempt: {:?}",
        visible_lines(&term)
    );
}

#[test]
fn shared_terminal_buffer_survives_session_style_reconnect_transfer() {
    // Mirrors Backend::start_with_terminal(Some(preserved)): the new attempt
    // keeps the same Arc so history is not wiped when the view swaps.
    let shared = Arc::new(Mutex::new(TerminalBuffer::new(Encoding::Utf8)));
    {
        let mut term = shared.lock();
        term.resize(24, 6);
        for i in 0..10 {
            term.feed(format!("keep-{i}\r\n").as_bytes());
        }
    }
    let history_before = shared.lock().term.grid().history_size();
    assert!(history_before > 0);

    shared.lock().prepare_reconnect();
    let transferred = Arc::clone(&shared);
    {
        let mut term = transferred.lock();
        assert!(term.term.grid().history_size() >= history_before);
        assert!(history_contains(&term, "keep-9"));
        term.feed(b"next-attempt\r\n");
        assert!(
            visible_lines(&term)
                .iter()
                .any(|line| line.contains("next-attempt"))
        );
        assert!(history_contains(&term, "keep-9"));
    }
    // Both handles observe the same grid.
    assert!(Arc::ptr_eq(&shared, &transferred));
    assert!(
        history_contains(&shared.lock(), "keep-0") || history_contains(&shared.lock(), "keep-1")
    );
}

#[test]
fn prepare_reconnect_resets_modes_cursor_and_character_state() {
    let mut term = TerminalBuffer::new(Encoding::Utf8);
    term.resize(24, 6);
    term.feed(b"preserved-output\r\n");
    // A disconnected full-screen program can leave input modes, cursor shape,
    // margins, attributes and a non-ASCII character set active.
    term.feed(b"\x1b[?1000h\x1b[?1006h\x1b[?1h\x1b[?2004h\x1b[?25l");
    term.feed(b"\x1b[2;4r\x1b[?6h\x1b[31;44m\x1b[6 q\x1b(0\x1b7");
    let mut retained = term.frame();
    term.take_frame_update(false);

    term.prepare_reconnect();

    assert_eq!(*term.term.mode(), TermMode::default());
    assert_eq!(term.size.cols, 24);
    assert_eq!(term.size.rows, 6);
    assert!(history_contains(&term, "preserved-output"));
    assert_eq!(term.term.grid().cursor.point.line.0, 0);
    assert_eq!(term.term.grid().cursor.point.column.0, 0);
    assert_incremental(&mut term, &mut retained);
    assert_eq!(
        terminal::key_bytes("up", false, false, false, *term.term.mode()),
        Some(b"\x1b[A".to_vec())
    );
    assert_eq!(term.paste("hello").unwrap(), b"hello");

    // Restoring the cursor must not restore the previous attempt's attributes.
    term.feed(b"\x1b8qqq\r\nsecond\r\nthird\r\nfourth\r\nfifth");
    let mut fresh = TerminalBuffer::new(Encoding::Utf8);
    fresh.resize(24, 6);
    fresh.feed(b"\x1b8qqq\r\nsecond\r\nthird\r\nfourth\r\nfifth");
    assert_eq!(term.frame().cursor, fresh.frame().cursor);
    for (actual, expected) in term.frame().cells.iter().zip(fresh.frame().cells.iter()) {
        assert_eq!(actual.cell, expected.cell);
    }
}

#[test]
fn prepare_reconnect_drops_incomplete_escape_and_decode_sequences() {
    for unfinished in [
        b"\x1b]0;old-title".as_slice(),
        b"\x1b[31;".as_slice(),
        b"\xe4\xb8".as_slice(),
    ] {
        let mut term = TerminalBuffer::new(Encoding::Utf8);
        term.resize(24, 6);
        term.feed(b"keep-before\r\n");
        term.feed(unfinished);
        term.prepare_reconnect();
        assert!(!term.feed(b"new-login\r\n"), "{unfinished:?}");
        assert_eq!(visible_lines(&term), vec!["new-login"], "{unfinished:?}");
        assert!(history_contains(&term, "keep-before"), "{unfinished:?}");
    }
}

#[test]
fn prepare_reconnect_drops_old_saved_history_erase_state() {
    let mut term = TerminalBuffer::new(Encoding::Utf8);
    term.resize(24, 6);
    term.feed(b"keep-before\r\n\x1b[3J");
    term.prepare_reconnect();
    // ED2 from the new shell must not combine with ED3 from the old attempt.
    term.feed(b"\x1b[2Jnew-login");
    assert!(history_contains(&term, "keep-before"));
    assert_eq!(visible_lines(&term), vec!["new-login"]);
}
