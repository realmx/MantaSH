//! A real OS PTY test: no simulated terminal adapter is involved.
use mantash::{encoding::Encoding, events::Event, model::*, services::Backend};
use std::time::{Duration, Instant};

#[test]
fn actual_shell_io_resize_and_close() {
    let dir = tempfile::tempdir().unwrap();
    let (backend, events, _, warning) = Backend::initialize_in(dir.path().into()).unwrap();
    assert!(warning.is_none());
    let owner = Owner::new();
    let shell = if cfg!(windows) {
        mantash::platform::default_shell()
    } else {
        "/bin/sh".into()
    };
    let session = backend.start(
        owner,
        SessionSpec::Local {
            shell,
            directory: dir.path().to_string_lossy().into_owned(),
            encoding: Encoding::Utf8,
        },
        zeroize::Zeroizing::new(String::new()),
        false,
    );
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut connected = false;
    while Instant::now() < deadline {
        if let Ok(event) = events.try_recv() {
            match event {
                Event::State(o, ConnectionState::Connected) if o == owner => {
                    connected = true;
                    break;
                }
                Event::State(_, ConnectionState::Failed(error)) => panic!("{error}"),
                _ => {}
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(connected, "PTY did not start");
    // A native window zoom can enqueue several geometry changes before the
    // PTY worker catches up. Exercise the same rapid grow/shrink path before
    // sending output that depends on the final grid.
    for (cols, rows) in [(101, 31), (160, 42), (80, 24), (132, 36)] {
        session.resize(cols, rows);
        let resize_deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < resize_deadline
            && session.terminal.lock().size != (mantash::terminal::GridSize { cols, rows })
        {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            session.terminal.lock().size,
            mantash::terminal::GridSize { cols, rows }
        );
    }
    session.input(if cfg!(windows) {
        b"echo ('MANTASH_' + 'PTY_OK')\r".to_vec()
    } else {
        "printf '\\n%s_%s 中文\\n' MANTASH PTY_OK\r"
            .as_bytes()
            .to_vec()
    });
    let mut found = false;
    while Instant::now() < deadline {
        let mut terminal = session.terminal.lock();
        if terminal.search("MANTASH_PTY_OK") > 0 {
            found = true;
            break;
        }
        drop(terminal);
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(found, "Expected real process output");
    assert_eq!(
        session.terminal.lock().size,
        mantash::terminal::GridSize {
            cols: 132,
            rows: 36
        }
    );
    // A continuously writing process must not starve a pending geometry change.
    session.input(b"yes MANTASH_STREAM\r".to_vec());
    std::thread::sleep(Duration::from_millis(100));
    session.resize(140, 32);
    let streaming_resize_deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < streaming_resize_deadline
        && session.terminal.lock().size
            != (mantash::terminal::GridSize {
                cols: 140,
                rows: 32,
            })
    {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        session.terminal.lock().size,
        mantash::terminal::GridSize {
            cols: 140,
            rows: 32
        },
        "continuous local output starved PTY resize"
    );
    session.input(vec![3]);
    std::thread::sleep(Duration::from_millis(100));
    backend.close(owner);
    assert!(session.cancel.is_cancelled());
    backend.shutdown();
}

/// The zsh integration must keep the user's real HISTFILE: /etc/zshrc derives
/// it from the overridden ZDOTDIR, and without the restore the terminal neither
/// loads ~/.zsh_history nor appends new commands to it.
#[cfg(unix)]
#[test]
fn zsh_integration_restores_the_user_histfile() {
    let zsh = std::path::Path::new("/bin/zsh");
    if !zsh.exists() {
        eprintln!("skipping: /bin/zsh is unavailable");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let user_zdot = tempfile::tempdir().unwrap();
    std::fs::write(
        user_zdot.path().join(".zsh_history"),
        ": 1700000000:0;echo PRESEEDED_ZMARK\n",
    )
    .unwrap();
    std::fs::write(
        user_zdot.path().join(".zshrc"),
        // Keep non-printing color markers and a right prompt shape. Those markers
        // affect zsh width accounting during SIGWINCH redraws without occupying grid cells.
        "autoload -Uz colors && colors\nsetopt prompt_subst\nGREEN=\"%{$fg[green]%}\"\nBLUE=\"%{$fg[blue]%}\"\nRED=\"%{$fg[red]%}\"\nRESET=\"%{$reset_color%}\"\nget_prompt_color() { local STATUS=\"$(git_prompt_status)\"; if [[ -n $STATUS ]]; then echo \"$RED\"; else echo \"$GREEN\"; fi; }\ngit_prompt_status() { echo \"$BLUE✹★\"; }\ngit_prompt_info() { echo '[master]'; }\ngit_prompt_short_sha() { echo '[85fde59]'; }\nTRAPWINCH() { print -r -- \"$COLUMNS:$LINES\" >> \"$MANTASH_INTEGRATION_DIR/winch.log\"; }\nPROMPT='$RESET$(get_prompt_color)$ $RESET'\nRPROMPT='$RESET$(git_prompt_status)$(get_prompt_color)$(git_prompt_info)$(git_prompt_short_sha)$RESET'\n",
    )
    .unwrap();
    let previous_zdotdir = std::env::var("ZDOTDIR").ok();
    // Test threads do not read this variable while it is swapped.
    unsafe { std::env::set_var("ZDOTDIR", user_zdot.path()) };
    let (backend, events, _, warning) = Backend::initialize_in(dir.path().into()).unwrap();
    assert!(warning.is_none());
    let owner = Owner::new();
    let session = backend.start(
        owner,
        SessionSpec::Local {
            shell: zsh.to_string_lossy().into_owned(),
            directory: dir.path().to_string_lossy().into_owned(),
            encoding: Encoding::Utf8,
        },
        zeroize::Zeroizing::new(String::new()),
        false,
    );
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut connected = false;
    while Instant::now() < deadline {
        if let Ok(event) = events.try_recv() {
            match event {
                Event::State(o, ConnectionState::Connected) if o == owner => {
                    connected = true;
                    break;
                }
                Event::State(_, ConnectionState::Failed(error)) => panic!("{error}"),
                _ => {}
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(connected, "zsh PTY did not start");
    // Wait for ZLE, not just process creation: resizing before the initial
    // prompt has rendered cannot exercise interrupted prompt redraws.
    let prompt_deadline = Instant::now() + Duration::from_secs(5);
    while !session.terminal.lock().command_cursor.editing() {
        assert!(
            Instant::now() < prompt_deadline,
            "zsh prompt did not become ready"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    // Start wide, briefly narrow, then restore width and height. An RPROMPT
    // touching the right edge must not leave the old prompt above ZLE's redraw.
    for (cols, rows) in [(282, 68), (279, 67), (174, 41), (282, 68)] {
        session.resize(cols, rows);
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline
            && session.terminal.lock().size != (mantash::terminal::GridSize { cols, rows })
        {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            session.terminal.lock().size,
            mantash::terminal::GridSize { cols, rows }
        );
        std::thread::sleep(Duration::from_millis(200));
    }
    let wide = session.terminal.lock().frame();
    let wide_rows = (0..wide.size.rows)
        .map(|row| {
            wide.cells
                .iter()
                .filter(|cell| cell.row == row)
                .map(|cell| cell.cell.c)
                .collect::<String>()
        })
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>();
    assert_eq!(
        wide_rows.len(),
        1,
        "Zoom duplicated the zsh prompt: {wide_rows:?}"
    );
    session.resize(80, 24);
    let restore_deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < restore_deadline
        && session.terminal.lock().size != (mantash::terminal::GridSize { cols: 80, rows: 24 })
    {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        session.terminal.lock().size,
        mantash::terminal::GridSize { cols: 80, rows: 24 }
    );
    // AppKit emits intermediate sizes across several frames. Queuing every
    // size at once would only exercise channel coalescing, not a zoom animation.
    for _ in 0..4 {
        for cols in (80..=280).step_by(5).chain((80..280).step_by(5).rev()) {
            session.resize(cols, 40);
            std::thread::sleep(Duration::from_millis(8));
        }
        std::thread::sleep(Duration::from_millis(150));
    }
    session.resize(80, 24);
    std::thread::sleep(Duration::from_millis(650));
    let winch_log = dir.path().join("shell-integration/winch.log");
    let winch_count = || {
        std::fs::read_to_string(&winch_log)
            .map(|text| text.lines().count())
            .unwrap_or(0)
    };
    let winch_before = winch_count();
    // A native Zoom can publish geometry frames across the full animation.
    // Keep both axes moving and leave an 80ms gap between frames to exercise
    // the stable-window boundary.
    for (cols, rows) in [(160, 42), (132, 36), (96, 28), (85, 25)] {
        session.resize(cols, rows);
        std::thread::sleep(Duration::from_millis(80));
    }
    std::thread::sleep(Duration::from_millis(650));
    let winch_after = winch_count();
    assert_eq!(
        winch_after.saturating_sub(winch_before),
        1,
        "rapid zoom sizes should produce one SIGWINCH, got {}",
        winch_after.saturating_sub(winch_before)
    );
    // Each stable endpoint must contain one complete Shell prompt. In particular,
    // an old prompt must not come back from scrollback on the next enlargement.
    let prompt_shape_ok = |expected: mantash::terminal::GridSize| {
        let terminal = session.terminal.lock();
        let frame = terminal.frame();
        if frame.size != expected || !terminal.command_cursor.editing() {
            return false;
        }
        let rows = (0..frame.size.rows)
            .map(|row| {
                frame
                    .cells
                    .iter()
                    .filter(|cell| cell.row == row)
                    .map(|cell| cell.cell.c)
                    .collect::<String>()
            })
            .collect::<Vec<_>>();
        let prompt_rows = rows
            .iter()
            .enumerate()
            .filter_map(|(row, text)| text.contains('$').then_some(row))
            .collect::<Vec<_>>();
        let branch_rows = rows
            .iter()
            .enumerate()
            .filter_map(|(row, text)| text.contains("[master]").then_some(row))
            .collect::<Vec<_>>();
        let nonempty_rows = rows.iter().filter(|row| !row.trim().is_empty()).count();
        prompt_rows.len() == 1
            && nonempty_rows == 1
            && branch_rows == prompt_rows
            && frame
                .cursor
                .is_some_and(|(row, _, _)| Some(row) == prompt_rows.first().copied())
    };
    let two_zoom_before = winch_count();
    session.resize(160, 42);
    std::thread::sleep(Duration::from_millis(650));
    assert!(
        prompt_shape_ok(mantash::terminal::GridSize {
            cols: 160,
            rows: 42
        }),
        "zoom left a duplicate or split prompt"
    );
    session.resize(80, 24);
    std::thread::sleep(Duration::from_millis(650));
    assert!(
        prompt_shape_ok(mantash::terminal::GridSize { cols: 80, rows: 24 }),
        "restore left a duplicate or split prompt"
    );
    assert_eq!(
        winch_count().saturating_sub(two_zoom_before),
        2,
        "two independent zoom endpoints should produce two SIGWINCH events"
    );
    let wait_for = |needle: &str| {
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if session.terminal.lock().search(needle) > 0 {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        false
    };
    session.input(b"printf '\\nMANTASH_AFTER_ZOOM\\n'\r".to_vec());
    assert!(
        wait_for("MANTASH_AFTER_ZOOM"),
        "shell input after the Zoom resize was not rendered"
    );
    session.input(b"printf '\\nMANTASH_HIST=%s\\n' \"$HISTFILE\"\r".to_vec());
    let expected = format!("MANTASH_HIST={}/.zsh_history", user_zdot.path().display());
    assert!(
        wait_for(&expected),
        "HISTFILE was not restored to the user's ZDOTDIR"
    );
    assert!(
        session
            .terminal
            .lock()
            .search("shell-integration/.zsh_history")
            == 0,
        "HISTFILE still points into the integration directory"
    );
    // The marker only exists in the seeded history file; the command itself
    // avoids the literal so a match proves the file was actually loaded.
    session.input(b"fc -l 1\r".to_vec());
    assert!(
        wait_for("PRESEEDED_ZMARK"),
        "seeded user history was not loaded"
    );
    // `clear` sends 3J before 2J. The latter must not reintroduce erased
    // prompts into scrollback when a later zoom grows the viewport.
    let before_clear = session.terminal.lock().revision;
    session.input(b"clear\r".to_vec());
    let clear_deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < clear_deadline
        && !(session.terminal.lock().revision > before_clear + 1
            && prompt_shape_ok(mantash::terminal::GridSize { cols: 80, rows: 24 }))
    {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        prompt_shape_ok(mantash::terminal::GridSize { cols: 80, rows: 24 }),
        "clear did not leave a single prompt"
    );
    for (cols, rows) in [(160, 42), (80, 24)] {
        session.resize(cols, rows);
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline
            && !prompt_shape_ok(mantash::terminal::GridSize { cols, rows })
        {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            prompt_shape_ok(mantash::terminal::GridSize { cols, rows }),
            "zoom resurrected a prompt erased by clear"
        );
    }
    backend.close(owner);
    backend.shutdown();
    match previous_zdotdir {
        Some(value) => unsafe { std::env::set_var("ZDOTDIR", value) },
        None => unsafe { std::env::remove_var("ZDOTDIR") },
    }
}
