//! Real Shell-hook checks isolated from the user's rc files and Shell history.
#![cfg(unix)]
use mantash::{
    integration,
    terminal::{HistoryParser, ShellReport},
};
use std::io::{Read, Write};

fn bash_reports(rc: &str, input: &[u8]) -> Vec<ShellReport> {
    let directory = tempfile::tempdir().unwrap();
    let rc_path = directory.path().join("bash.rc");
    std::fs::write(&rc_path, rc).unwrap();
    let token = uuid::Uuid::new_v4().to_string();
    let pair = portable_pty::native_pty_system()
        .openpty(portable_pty::PtySize {
            rows: 24,
            cols: 100,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let mut command = portable_pty::CommandBuilder::new("/bin/bash");
    command.arg("--noprofile");
    command.arg("--rcfile");
    command.arg(rc_path);
    command.arg("-i");
    command.env("MANTASH_HISTORY_TOKEN", &token);
    command.env("TERM", "xterm-256color");
    command.cwd(directory.path());
    let mut child = pair.slave.spawn_command(command).unwrap();
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader().unwrap();
    let mut writer = pair.master.take_writer().unwrap();
    let mut killer = child.clone_killer();
    let (done, wait) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        if wait
            .recv_timeout(std::time::Duration::from_secs(5))
            .is_err()
        {
            let _ = killer.kill();
        }
    });
    writer.write_all(input).unwrap();
    writer.flush().unwrap();
    let mut parser = HistoryParser::new(token);
    let mut reports = vec![];
    let mut bytes = [0u8; 4096];
    loop {
        match reader.read(&mut bytes) {
            Ok(0) | Err(_) => break,
            Ok(n) => reports.extend(parser.feed(&bytes[..n])),
        }
    }
    let _ = done.send(());
    let _ = child.wait();
    reports
}

#[test]
fn bash_records_current_commands_without_replaying_ignored_history() {
    let rc = integration::BASH_RC.replacen(
        "[[ -r \"$HOME/.bashrc\" ]] && source \"$HOME/.bashrc\"",
        "HISTFILE=/dev/null\nHISTCONTROL=ignorespace\nhistory -c\nhistory -s __old_history_should_not_replay__",
        1,
    );
    let reports = bash_reports(
        &rc,
        b"echo first-hook-command\r echo hidden-history-command\recho second-hook-command\rexit\r",
    );
    assert!(
        reports
            .iter()
            .any(|s| s.command == "echo first-hook-command"),
        "{reports:?}"
    );
    assert!(
        reports
            .iter()
            .any(|s| s.command == "echo second-hook-command"),
        "{reports:?}"
    );
    assert!(
        !reports
            .iter()
            .any(|s| s.command.contains("hidden-history") || s.command.contains("old_history")),
        "{reports:?}"
    );
}

#[test]
fn an_existing_debug_trap_is_not_overwritten() {
    let rc = integration::BASH_RC.replacen(
        "[[ -r \"$HOME/.bashrc\" ]] && source \"$HOME/.bashrc\"",
        "HISTFILE=/dev/null\nhistory -c\ntrap ':' DEBUG",
        1,
    );
    assert!(
        bash_reports(&rc, b"echo custom-debug-trap\rexit\r")
            .iter()
            .all(|r| r.command.is_empty())
    );
}

#[test]
fn prompt_reports_the_directory_after_cd_without_a_history_entry() {
    let rc = integration::BASH_RC.replacen(
        "[[ -r \"$HOME/.bashrc\" ]] && source \"$HOME/.bashrc\"",
        "HISTFILE=/dev/null\nhistory -c",
        1,
    );
    let reports = bash_reports(&rc, b"mkdir nested\rcd nested\rexit\r");
    assert!(
        reports
            .iter()
            .any(|r| r.command.is_empty() && r.directory.ends_with("/nested")),
        "{reports:?}"
    );
}
