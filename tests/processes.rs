//! Process identity and outcome tests do not touch any production process.
use mantash::{
    monitor,
    processes::{self, Action, Identity, Outcome},
};

const BOOT: &str = "59372983-86ce-4bae-9f6a-555ef0db8dc8";
fn identity() -> Identity {
    Identity {
        pid: 45678,
        start_ticks: 900,
        boot_id: BOOT.into(),
    }
}

/// A reused PID between ps and /proc reads is displayed but never made actionable.
#[test]
fn sampling_brackets_process_lifetimes_and_requires_matching_identity() {
    let data = format!(
        "__MANTASH_OS__\nLinux\n6.8\ntest\n100.0 1\n__MANTASH_BOOT__\n{BOOT}\n100\n1000\n__MANTASH_START_BEFORE__\n45678 900\n45679 920\n__MANTASH_PROCESS_V2__\n45678 10 3.0 2.0 1024 S alice test-process\n45679 10 2.0 1.0 512 S bob reused\n__MANTASH_START_AFTER__\n45678 900\n45679 999\n"
    );
    let sample = monitor::parse(&data, None, 1100);
    assert_eq!(sample.processes[0].identity.as_ref(), Some(&identity()));
    assert_eq!(sample.processes[0].started_at, Some(1009));
    assert_eq!(sample.processes[0].user, "alice");
    assert!(sample.processes[1].identity.is_none());
    assert!(!processes::instance_gone(&identity(), &sample));
    let mut later = sample.clone();
    later.processes.retain(|p| p.pid != 45678);
    assert!(processes::instance_gone(&identity(), &later));
    later
        .errors
        .insert("processes".into(), "permission denied".into());
    assert!(!processes::instance_gone(&identity(), &later));
    let changed = monitor::parse(&data.replace("45678 900", "45678 1200"), None, 1101);
    assert!(processes::instance_gone(&identity(), &changed));
}

/// Invalid samples or reused PIDs cannot supply live metrics for the selected instance.
#[test]
fn current_process_requires_original_identity_and_valid_sample() {
    use processes::TargetIssue;
    let id = identity();
    let mut sample = monitor::Sample {
        boot_id: Some(BOOT.into()),
        system: "Linux".into(),
        processes: vec![monitor::Process {
            user: "alice".into(),
            started_at: Some(1009),
            identity: Some(id.clone()),
            pid: id.pid,
            parent: 10,
            cpu: 3.,
            memory: 2.,
            rss: 1024,
            state: "S".into(),
            command: "test-process".into(),
        }],
        ..Default::default()
    };
    assert_eq!(
        processes::current_process(&id, &sample).unwrap().user,
        "alice"
    );
    let mut protected = id.clone();
    protected.pid = 1;
    assert_eq!(
        processes::current_process(&protected, &sample).unwrap_err(),
        TargetIssue::IdentityMissing
    );
    sample.system = "Darwin".into();
    assert_eq!(
        processes::current_process(&id, &sample).unwrap_err(),
        TargetIssue::InvalidSample
    );
    sample.system = "Linux".into();
    sample
        .errors
        .insert("processes".into(), "unavailable".into());
    assert_eq!(
        processes::current_process(&id, &sample).unwrap_err(),
        TargetIssue::InvalidSample
    );
    sample.errors.clear();
    sample.boot_id = Some("81283e7a-8d63-4097-81e0-c508c17235ef".into());
    assert_eq!(
        processes::current_process(&id, &sample).unwrap_err(),
        TargetIssue::HostChanged
    );
    sample.boot_id = Some(BOOT.into());
    sample.processes[0].identity.as_mut().unwrap().start_ticks += 1;
    assert_eq!(
        processes::current_process(&id, &sample).unwrap_err(),
        TargetIssue::Changed
    );
    sample.processes.clear();
    assert_eq!(
        processes::current_process(&id, &sample).unwrap_err(),
        TargetIssue::Gone
    );
}
/// Invalid identifiers and protected PIDs cannot be interpolated into an exec request.
#[test]
fn signal_commands_validate_the_target_and_use_only_allowlisted_signals() {
    for pid in [0, 1, 2] {
        let mut id = identity();
        id.pid = pid;
        assert!(processes::signal_command(&id, Action::Terminate).is_err());
    }
    let mut id = identity();
    id.boot_id = "'; touch /tmp/unsafe; #".into();
    assert!(processes::signal_command(&id, Action::Force).is_err());
    let term = processes::signal_command(&identity(), Action::Terminate).unwrap();
    assert!(term.contains("kill -TERM"));
    assert!(!term.contains("kill -KILL"));
    assert!(
        processes::signal_command(&identity(), Action::Force)
            .unwrap()
            .contains("kill -KILL")
    );
    assert_eq!(
        processes::outcome("__MANTASH_PROCESS_SENT__\n"),
        Outcome::Sent
    );
    assert_eq!(
        processes::outcome("__MANTASH_PROCESS_GONE__\n"),
        Outcome::Gone
    );
    assert_ne!(
        processes::outcome("__MANTASH_PROCESS_SENT__\n"),
        Outcome::Gone
    );
    assert_eq!(processes::outcome("SSH connection lost"), Outcome::Unknown);
    assert_eq!(
        processes::outcome("__MANTASH_PROCESS_DENIED__\n"),
        Outcome::Denied
    );
}

/// Detail payloads are display-only; empty, malformed and expired responses stay errors.
#[test]
fn process_details_are_bounded_encoded_data() {
    let details =
        processes::details("__MANTASH_PROCESS_DETAILS__\nc2xlZXAAMzAA\nTmFtZToJc2xlZXAK\n")
            .unwrap();
    assert_eq!(details.command, "sleep 30");
    assert!(processes::details("__MANTASH_PROCESS_CHANGED__\n").is_err());
    assert!(processes::details("__MANTASH_PROCESS_DETAILS__\ninvalid!\n").is_err());
}

/// On macOS a genuine command channel rejects Linux operations before attempting a signal.
#[cfg(target_os = "macos")]
#[test]
fn non_linux_host_refuses_real_probe_and_signal_scripts() {
    for command in [
        processes::details_command(&identity()).unwrap(),
        processes::signal_command(&identity(), Action::Terminate).unwrap(),
    ] {
        let out = std::process::Command::new("/bin/sh")
            .args(["-c", &command])
            .output()
            .unwrap();
        assert!(out.status.success());
        assert_eq!(
            processes::outcome(&String::from_utf8_lossy(&out.stdout)),
            Outcome::Unsupported
        );
    }
}

/// This real Linux test creates, signals and waits only for its own disposable child.
#[cfg(target_os = "linux")]
#[test]
fn real_linux_process_identity_and_termination() {
    use std::{
        process::{Command, Stdio},
        time::Duration,
    };
    for action in [Action::Terminate, Action::Force] {
        let mut child = Command::new("sleep")
            .arg("30")
            .stdin(Stdio::null())
            .spawn()
            .unwrap();
        let stat = std::fs::read_to_string(format!("/proc/{}/stat", child.id())).unwrap();
        let tail = stat.rsplit_once(") ").unwrap().1;
        let id = Identity {
            pid: child.id(),
            start_ticks: tail.split_whitespace().nth(19).unwrap().parse().unwrap(),
            boot_id: std::fs::read_to_string("/proc/sys/kernel/random/boot_id")
                .unwrap()
                .trim()
                .into(),
        };
        let probe = Command::new("sh")
            .args(["-c", &processes::details_command(&id).unwrap()])
            .output()
            .unwrap();
        assert!(
            processes::details(&String::from_utf8_lossy(&probe.stdout))
                .unwrap()
                .command
                .contains("sleep 30")
        );
        let mut stale = id.clone();
        stale.start_ticks += 1;
        let rejected = Command::new("sh")
            .args(["-c", &processes::signal_command(&stale, action).unwrap()])
            .output()
            .unwrap();
        assert_eq!(
            processes::outcome(&String::from_utf8_lossy(&rejected.stdout)),
            Outcome::Changed
        );
        assert!(child.try_wait().unwrap().is_none());
        let sent = Command::new("sh")
            .args(["-c", &processes::signal_command(&id, action).unwrap()])
            .output()
            .unwrap();
        assert_eq!(
            processes::outcome(&String::from_utf8_lossy(&sent.stdout)),
            Outcome::Sent
        );
        for _ in 0..50 {
            if child.try_wait().unwrap().is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let exited = child.try_wait().unwrap().is_some();
        if !exited {
            let _ = child.kill();
        }
        let _ = child.wait();
        assert!(exited);
    }
}

/// Dynamic metadata is intentionally conservative and never mutates SSH profile titles.
#[test]
fn local_titles_do_not_guess_raw_input_or_include_arguments() {
    assert_eq!(
        mantash::titles::reported_program("vim secret.txt"),
        Some("vim".into())
    );
    assert!(mantash::titles::reported_program("TOKEN=secret command").is_none());
    assert_eq!(
        mantash::titles::local_label("/srv/api-service", Some("vim"), "/bin/zsh"),
        "vim · api-service"
    );
}
