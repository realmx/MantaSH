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

#[cfg(unix)]
struct InteractiveBash {
    backend: Backend,
    owner: Owner,
    session: std::sync::Arc<mantash::services::Session>,
    dir: tempfile::TempDir,
}

#[cfg(unix)]
impl InteractiveBash {
    fn start() -> Option<Self> {
        let bash = std::path::Path::new("/bin/bash");
        if !bash.exists() {
            eprintln!("skipping: /bin/bash is unavailable");
            return None;
        }
        let dir = tempfile::tempdir().unwrap();
        let (backend, events, _, _) = Backend::initialize_in(dir.path().into()).unwrap();
        let owner = Owner::new();
        let session = backend.start(
            owner,
            SessionSpec::Local {
                shell: bash.to_string_lossy().into_owned(),
                directory: dir.path().to_string_lossy().into_owned(),
                encoding: Encoding::Utf8,
            },
            zeroize::Zeroizing::new(String::new()),
            false,
        );
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            assert!(Instant::now() < deadline, "bash PTY did not start");
            match events.try_recv() {
                Ok(Event::State(o, ConnectionState::Connected)) if o == owner => break,
                Ok(Event::State(_, ConnectionState::Failed(error))) => panic!("{error}"),
                _ => std::thread::sleep(Duration::from_millis(10)),
            }
        }
        let shell = Self {
            backend,
            owner,
            session,
            dir,
        };
        shell.run_and_wait("READY");
        Some(shell)
    }

    /// Run `printf` through the shell and wait for its output, proving the
    /// prompt accepted a new command.
    fn run_and_wait(&self, marker: &str) {
        let (head, tail) = marker.split_at(marker.len() / 2);
        self.session
            .input(format!("printf '\\n%s%s\\n' {head} {tail}\r").into_bytes());
        assert!(
            wait_until(Duration::from_secs(10), || self
                .session
                .terminal
                .lock()
                .search(marker)
                > 0),
            "shell did not run a command after {marker}"
        );
    }

    fn pid_file(&self, name: &str) -> i32 {
        let path = self.dir.path().join(name);
        let mut pid = None;
        assert!(
            wait_until(Duration::from_secs(10), || {
                pid = std::fs::read_to_string(&path)
                    .ok()
                    .and_then(|text| text.trim().parse().ok());
                pid.is_some()
            }),
            "{name} was not written"
        );
        pid.unwrap()
    }
}

#[cfg(unix)]
impl Drop for InteractiveBash {
    fn drop(&mut self) {
        self.backend.close(self.owner);
        self.backend.shutdown();
    }
}

fn wait_until(timeout: Duration, mut condition: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if condition() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    condition()
}

/// Zombies awaiting their reaper count as exited.
#[cfg(unix)]
fn process_running(pid: i32) -> bool {
    let output = std::process::Command::new("ps")
        .args(["-o", "stat=", "-p", &pid.to_string()])
        .output()
        .unwrap();
    let state = String::from_utf8_lossy(&output.stdout);
    let state = state.trim();
    !state.is_empty() && !state.starts_with('Z')
}

#[cfg(unix)]
fn process_stopped(pid: i32) -> bool {
    let output = std::process::Command::new("ps")
        .args(["-o", "stat=", "-p", &pid.to_string()])
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout)
        .trim()
        .starts_with('T')
}

/// A pnpm-style wrapper whose `&` child ignores SIGINT, as every async child of
/// a non-interactive `sh -c` does, must not keep running after Ctrl+C.
#[cfg(unix)]
#[test]
fn ctrl_c_stops_the_foreground_job_and_its_leftover_members() {
    let Some(shell) = InteractiveBash::start() else {
        return;
    };
    shell.session.input(
        b"sh -c 'sleep 1000 & echo $! > bg.pid; sh -c \"echo \\$\\$ > fg.pid; exec sleep 1000\"'\r"
            .to_vec(),
    );
    let background = shell.pid_file("bg.pid");
    let foreground = shell.pid_file("fg.pid");
    assert!(process_running(background) && process_running(foreground));
    shell.session.input(vec![3]);
    assert!(
        wait_until(Duration::from_secs(5), || !process_running(foreground)),
        "SIGINT did not reach the foreground process group"
    );
    let stopped = wait_until(Duration::from_secs(5), || !process_running(background));
    if !stopped {
        unsafe { libc::kill(background, libc::SIGKILL) };
    }
    assert!(stopped, "the interrupted job left a member running");
    shell.run_and_wait("AFTER_INTERRUPT");
}

#[cfg(unix)]
#[test]
fn ctrl_c_follow_up_keeps_nohup_processes() {
    let Some(shell) = InteractiveBash::start() else {
        return;
    };
    shell.session.input(
        b"sh -c 'nohup sleep 1000 >/dev/null 2>&1 & echo $! > bg.pid; sleep 1000'\r".to_vec(),
    );
    let background = shell.pid_file("bg.pid");
    std::thread::sleep(Duration::from_millis(200));
    shell.session.input(vec![3]);
    shell.run_and_wait("AFTER_NOHUP_INTERRUPT");
    std::thread::sleep(Duration::from_millis(500));
    let running = process_running(background);
    unsafe { libc::kill(background, libc::SIGKILL) };
    assert!(
        running,
        "a nohup process was terminated by the interrupt follow-up"
    );
}

/// Ctrl+C that the job survives followed by Ctrl+Z suspends the job; the
/// shell reclaiming the terminal must not be mistaken for the job ending.
#[cfg(unix)]
#[test]
fn ctrl_z_after_a_survived_ctrl_c_keeps_the_suspended_job() {
    let Some(shell) = InteractiveBash::start() else {
        return;
    };
    shell
        .session
        .input(b"sh -c 'trap \"\" INT; echo $$ > fg.pid; exec sleep 1000'\r".to_vec());
    let job = shell.pid_file("fg.pid");
    std::thread::sleep(Duration::from_millis(200));
    shell.session.input(vec![3]);
    std::thread::sleep(Duration::from_millis(300));
    assert!(process_running(job), "the job should ignore SIGINT");
    shell.session.input(vec![0x1a]);
    let suspended = wait_until(Duration::from_secs(5), || process_stopped(job));
    shell.run_and_wait("AFTER_SUSPEND");
    std::thread::sleep(Duration::from_millis(500));
    let stopped = process_stopped(job);
    unsafe {
        libc::kill(job, libc::SIGKILL);
    }
    assert!(suspended && stopped, "the suspended job was signalled");
}

/// Closing a tab hangs up the PTY like a terminal window closing. A shell that
/// ignores SIGHUP must still exit on the hangup instead of outliving the tab.
#[cfg(unix)]
#[test]
fn closing_a_tab_hangs_up_a_shell_that_ignores_sighup() {
    let Some(shell) = InteractiveBash::start() else {
        return;
    };
    shell
        .session
        .input(b"trap '' HUP; echo $$ > shell.pid\r".to_vec());
    let pid = shell.pid_file("shell.pid");
    shell.run_and_wait("HUP_IGNORED");
    shell.backend.close(shell.owner);
    let exited = wait_until(Duration::from_secs(5), || !process_running(pid));
    if !exited {
        unsafe { libc::kill(pid, libc::SIGKILL) };
    }
    assert!(exited, "the shell outlived its closed tab");
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
