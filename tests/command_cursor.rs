//! A cursor click must move the actual line editor and never a historical/password prompt.
use mantash::{encoding::Encoding, terminal::TerminalBuffer};

fn ready(cols: usize) -> TerminalBuffer {
    let mut term = TerminalBuffer::new(Encoding::Utf8);
    term.resize(cols, 6);
    term.set_shell_token("test-token".into());
    term.feed(b"> \x1b]777;mantash-cursor;test-token;ready\x07");
    term
}

#[test]
fn prompt_markers_are_scoped_and_streamed() {
    let mut term = ready(20);
    assert!(term.command_cursor.editing());
    term.feed(b"\x1b]777;mantash-cursor;other-token;running\x07");
    assert!(term.command_cursor.editing());
    term.feed(b"\x1b]777;mantash-cursor;test-");
    term.feed(b"token;running\x1b\\");
    assert!(!term.command_cursor.editing());
    assert!(term.cursor_movement(2, 0).is_none());
}

#[test]
fn clicks_count_characters_not_wide_spacer_cells() {
    let mut term = ready(30);
    term.feed("echo ab中cd".as_bytes());
    assert_eq!(
        term.cursor_movement(9, 0),
        Some(b"\x1b[D\x1b[D\x1b[D".to_vec())
    );
    assert_eq!(term.cursor_movement(10, 0), term.cursor_movement(9, 0));
    assert!(term.cursor_movement(0, 0).is_none());
    assert!(term.cursor_movement(25, 0).is_none());
}

#[test]
fn wrapped_commands_move_both_directions_after_cursor_redraws() {
    let mut term = ready(10);
    term.feed(b"abcdefghijklm");
    assert_eq!(term.cursor_movement(4, 0), Some(b"\x1b[D".repeat(11)));
    term.feed(b"\x1b[1;5H");
    assert_eq!(term.cursor_movement(3, 1), Some(b"\x1b[C".repeat(9)));
    term.feed(b"\r\x1b[4C");
    assert!(term.cursor_movement(3, 1).is_some());
}

#[test]
fn executing_scrollback_and_unverified_shells_do_not_synthesize_keys() {
    let mut term = ready(20);
    term.feed(b"command");
    term.input_sent(b"\r");
    term.feed(b"\r\nPassword: ");
    assert!(term.cursor_movement(3, 1).is_none());
    let mut term = ready(20);
    term.feed(b"echo text\x1b[?1049h");
    assert!(term.cursor_movement(3, 0).is_none());
    let mut scrolled = ready(20);
    scrolled.feed(b"one\r\ntwo\r\nthree\r\nfour\r\nfive\r\nsix\r\n> \x1b]777;mantash-cursor;test-token;ready\x07echo text");
    assert!(scrolled.command_cursor.editing());
    scrolled.scroll(2);
    assert!(scrolled.cursor_movement(4, 3).is_none());
    let term = TerminalBuffer::new(Encoding::Utf8);
    assert!(term.cursor_movement(2, 0).is_none());
}

#[test]
fn terminal_mouse_reports_follow_requested_protocol_and_bounds() {
    use alacritty_terminal::term::TermMode;
    let legacy = TermMode::MOUSE_REPORT_CLICK;
    assert_eq!(
        mantash::terminal_io::mouse_bytes(legacy, 0, 3, 2, false),
        Some(vec![27, b'[', b'M', 32, 36, 35])
    );
    assert_eq!(
        mantash::terminal_io::mouse_bytes(legacy | TermMode::SGR_MOUSE, 0, 3, 2, true),
        Some(b"\x1b[<0;4;3m".to_vec())
    );
    assert!(mantash::terminal_io::mouse_bytes(TermMode::empty(), 0, 3, 2, false).is_none());
    assert!(mantash::terminal_io::mouse_bytes(legacy, 0, 300, 2, false).is_none());
    assert!(
        mantash::terminal_io::mouse_bytes(legacy | TermMode::UTF8_MOUSE, 0, 300, 2, false)
            .is_some()
    );
    assert!(
        mantash::terminal_io::mouse_bytes(legacy | TermMode::SGR_MOUSE, 0, usize::MAX, 2, false)
            .is_none()
    );
    assert!(
        mantash::integration::cmd_prompt("$P$G", "test-token")
            .starts_with("$P$G$E]777;mantash-cursor;test-token;ready")
    );
}

#[cfg(unix)]
mod real_shell {
    use super::*;
    use parking_lot::Mutex;
    use std::{
        io::{Read, Write},
        sync::Arc,
        time::{Duration, Instant},
    };
    struct Shell {
        term: Arc<Mutex<TerminalBuffer>>,
        writer: Arc<Mutex<Box<dyn Write + Send>>>,
        killer: Box<dyn portable_pty::ChildKiller + Send + Sync>,
        _dir: tempfile::TempDir,
    }
    impl Drop for Shell {
        fn drop(&mut self) {
            let _ = self.killer.kill();
        }
    }
    impl Shell {
        /// Load only isolated hook files and keep authentication/history input out of the fixture.
        fn new(zsh: bool) -> Self {
            let dir = tempfile::tempdir().unwrap();
            let token = uuid::Uuid::new_v4().to_string();
            let rc = if zsh {
                // Anchor only on the user-rc source line: the HISTFILE restore
                // block added later sits between the ZDOTDIR line and this one,
                // so a needle spanning both no longer matches.
                mantash::integration::ZSH_RC.replacen(
                    "[[ -r \"$ZDOTDIR/.zshrc\" ]] && source \"$ZDOTDIR/.zshrc\"",
                    "HISTFILE=/dev/null\nPROMPT='> '\nRPROMPT=''",
                    1,
                )
            } else {
                mantash::integration::BASH_RC.replacen(
                    "[[ -r \"$HOME/.bashrc\" ]] && source \"$HOME/.bashrc\"",
                    "HISTFILE=/dev/null\nPS1='> '\nHISTCONTROL=ignorespace\nhistory -c",
                    1,
                )
            };
            let rc_path = dir.path().join(if zsh { ".zshrc" } else { "bash.rc" });
            std::fs::write(&rc_path, rc).unwrap();
            let pair = portable_pty::native_pty_system()
                .openpty(portable_pty::PtySize {
                    rows: 24,
                    cols: 80,
                    pixel_width: 0,
                    pixel_height: 0,
                })
                .unwrap();
            let mut command =
                portable_pty::CommandBuilder::new(if zsh { "/bin/zsh" } else { "/bin/bash" });
            if zsh {
                command.args(["-d", "-i"]);
                command.env("ZDOTDIR", dir.path());
            } else {
                command.args(["--noprofile", "--rcfile"]);
                command.arg(&rc_path);
                command.arg("-i");
            }
            command.env("TERM", "xterm-256color");
            // Readline needs a UTF-8 locale to account multi-byte input as
            // single characters; without it edits and redraws split bytes and
            // the column assertions drift (the GUI always inherits the user's
            // locale, so this mirrors the real session).
            command.env("LANG", "en_US.UTF-8");
            command.env("MANTASH_HISTORY_TOKEN", &token);
            command.cwd(dir.path());
            let mut child = pair.slave.spawn_command(command).unwrap();
            let killer = child.clone_killer();
            drop(pair.slave);
            let mut reader = pair.master.try_clone_reader().unwrap();
            let writer = Arc::new(Mutex::new(pair.master.take_writer().unwrap()));
            let mut buffer = TerminalBuffer::new(Encoding::Utf8);
            buffer.set_shell_token(token);
            let term = Arc::new(Mutex::new(buffer));
            let output = term.clone();
            let replies = writer.clone();
            std::thread::spawn(move || {
                let _master = pair.master;
                let mut bytes = [0; 8192];
                loop {
                    let Ok(n) = reader.read(&mut bytes) else {
                        break;
                    };
                    if n == 0 {
                        break;
                    }
                    let mut term = output.lock();
                    term.feed(&bytes[..n]);
                    let events: Vec<_> = term.events.0.lock().drain(..).collect();
                    drop(term);
                    for event in events {
                        if let alacritty_terminal::event::Event::PtyWrite(text) = event {
                            let _ = replies.lock().write_all(text.as_bytes());
                        }
                    }
                }
                let _ = child.wait();
            });
            let shell = Self {
                term,
                writer,
                killer,
                _dir: dir,
            };
            shell.wait(|t| {
                t.command_cursor.editing() && t.frame().cursor.is_some_and(|(_, c, _)| c == 2)
            });
            shell
        }
        fn send(&self, bytes: &[u8]) {
            self.term.lock().input_sent(bytes);
            let mut writer = self.writer.lock();
            writer.write_all(bytes).unwrap();
            writer.flush().unwrap();
        }
        fn wait(&self, test: impl Fn(&TerminalBuffer) -> bool) {
            let deadline = Instant::now() + Duration::from_secs(5);
            while Instant::now() < deadline {
                if test(&self.term.lock()) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            let t = self.term.lock();
            panic!(
                "Shell cursor did not reach expected state: {:?}, editing {}",
                t.frame().cursor,
                t.command_cursor.editing()
            );
        }
        fn click(&self, col: usize, row: usize) {
            let movement = self
                .term
                .lock()
                .cursor_movement(col, row)
                .expect("verified command location");
            self.send(&movement);
        }
        fn check(zsh: bool) {
            let shell = Self::new(zsh);
            let row = shell.term.lock().frame().cursor.unwrap().0;
            shell.send("echo ab中cd".as_bytes());
            shell.wait(|t| {
                t.frame()
                    .cursor
                    .is_some_and(|(r, c, _)| r == row && c == 13)
            });
            shell.click(9, row);
            shell.wait(|t| t.frame().cursor.is_some_and(|(r, c, _)| r == row && c == 9));
            shell.click(12, row);
            shell.wait(|t| {
                t.frame()
                    .cursor
                    .is_some_and(|(r, c, _)| r == row && c == 12)
            });
            shell.send(b"X");
            shell.wait(|t| {
                t.frame()
                    .cells
                    .iter()
                    .any(|c| c.row == row && c.col == 12 && c.cell.c == 'X')
            });
            shell.send(b"\r");
            shell.wait(|t| {
                t.command_cursor.editing() && t.frame().cursor.is_some_and(|(r, _, _)| r > row)
            });
            let row = shell.term.lock().frame().cursor.unwrap().0;
            shell.send("echo e\u{301}😀z".as_bytes());
            shell.wait(|t| {
                t.frame()
                    .cursor
                    .is_some_and(|(r, c, _)| r == row && c == 11)
            });
            shell.click(7, row);
            shell.wait(|t| t.frame().cursor.is_some_and(|(r, c, _)| r == row && c == 7));
            shell.click(10, row);
            shell.wait(|t| {
                t.frame()
                    .cursor
                    .is_some_and(|(r, c, _)| r == row && c == 10)
            });
        }
    }
    #[test]
    fn bash_click_moves_the_real_readline_cursor() {
        Shell::check(false);
    }
    #[test]
    fn zsh_click_moves_the_real_zle_cursor() {
        if std::path::Path::new("/bin/zsh").exists() {
            Shell::check(true);
        }
    }
}
