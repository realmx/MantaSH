//! Behaviour tests for encoding, import, persistence, terminal and restoration boundaries.
use alacritty_terminal::{
    index::{Column, Line, Point},
    term::TermMode,
};
use base64::Engine;
use mantash::{
    connections,
    encoding::{self, Encoding, TerminalDecoder},
    model::*,
    monitor,
    storage::Database,
    terminal::{self, HistoryParser, TerminalBuffer},
};

fn profile() -> Profile {
    Profile {
        id: Id::new_v4(),
        name: "开发, 主机".into(),
        host: "example.test".into(),
        port: 2222,
        username: "developer".into(),
    }
}

#[test]
fn cloned_connection_has_a_new_credential_identity_and_preserves_public_metadata() {
    let original = profile();
    let cloned = connections::clone_profile(&original, "开发副本".into());
    assert_ne!(original.id, cloned.id);
    assert_eq!(cloned.name, "开发副本");
    assert_eq!(cloned.host, original.host);
    assert_eq!(cloned.port, original.port);
}

#[test]
fn connection_search_matches_endpoints_and_all_terms_without_private_paths() {
    let connection = profile();
    for query in [
        "  ",
        "EXAMPLE 2222",
        "developer@example.test:2222",
        "开发 developer",
    ] {
        assert!(connections::matches_query(&connection, query), "{query}");
    }
    for query in ["example 3306", "~/.ssh/development", "missing"] {
        assert!(!connections::matches_query(&connection, query), "{query}");
    }
}

#[test]
fn metadata_and_password_round_trip_through_csv() {
    let profiles = vec![profile()];
    // Export places the stored password after username; id/auth/encoding stay out.
    let mut secrets = std::collections::HashMap::new();
    secrets.insert(profiles[0].id, "s3cret-密码".to_string());
    let exported = connections::export(&profiles, &secrets).unwrap();
    assert!(exported.starts_with("name,host,port,username,password"));
    assert!(!exported.contains("passphrase"));
    assert!(exported.contains("s3cret-密码"));
    let preview = connections::preview(&exported, &[]).unwrap();
    assert_eq!(preview.rows[0].password.as_deref(), Some("s3cret-密码"));
    // No id column means re-imported rows get fresh UUIDs; identity is the endpoint.
    let merged = connections::merge(&[], &preview, false);
    assert_eq!(merged.len(), 1);
    assert_ne!(merged[0].id, profiles[0].id);
    assert_eq!(
        (
            merged[0].name.as_str(),
            merged[0].host.as_str(),
            merged[0].port,
            merged[0].username.as_str()
        ),
        (
            profiles[0].name.as_str(),
            profiles[0].host.as_str(),
            profiles[0].port,
            profiles[0].username.as_str()
        )
    );
    // Exported files carry a UTF-8 BOM for spreadsheet apps; import must strip it.
    let bom_text = "\u{feff}".to_string() + exported.as_str();
    let preview = connections::preview(&bom_text, &[]).unwrap();
    assert!(preview.rows[0].profile.is_some());
    // Retired column sets are rejected outright instead of being silently tolerated.
    let legacy = "id,name,host,port,username,auth,key_path,encoding\r\n00000000-0000-4000-8000-000000000001,\"开发, 主机\",example.test,2222,developer,password,,gb18030\r\n";
    assert!(connections::preview(legacy, &[]).is_err());
    // A header missing one of the five columns is rejected as well.
    let missing = "name,host,port,username\r\n开发,example.test,22,developer\r\n";
    assert!(connections::preview(missing, &[]).is_err());
}

#[test]
fn imports_preview_errors_and_preserve_existing_identity() {
    let original = profile();
    let mut incoming = original.clone();
    incoming.id = Id::new_v4();
    let data = format!(
        "name,host,port,username,password\r\n\"{}\",{},{},{},\r\nbad host,,22,x,\r\n",
        incoming.name, incoming.host, incoming.port, incoming.username
    );
    let preview = connections::preview(&data, std::slice::from_ref(&original)).unwrap();
    assert_eq!(preview.rows[0].duplicate, Some(original.id));
    assert!(preview.rows[1].error.is_some());
    assert_eq!(
        connections::merge(std::slice::from_ref(&original), &preview, false),
        vec![original.clone()]
    );
    let updated = connections::merge(std::slice::from_ref(&original), &preview, true);
    assert_eq!(updated[0].id, original.id);
}

#[test]
fn unicode_encodings_preserve_bom_and_reject_loss() {
    for encoding in Encoding::FILE {
        let text = "中文 測試\nHello\r\n";
        let bytes = encoding::encode(text, encoding, true).unwrap();
        let decoded = encoding::decode_file(&bytes, Some(encoding)).unwrap();
        assert_eq!(decoded.text, text, "{}", encoding.label());
        assert_eq!(
            encoding::encode(&decoded.text, decoded.encoding, decoded.bom).unwrap(),
            bytes
        );
    }
    assert!(encoding::encode("😀", Encoding::Big5, false).is_err());
    assert!(encoding::decode_file(&[0xff, 0xff], Some(Encoding::Utf8)).is_err());
    assert!(encoding::decode_file(b"binary\0text", None).is_err());
}

#[test]
fn multibyte_terminal_chunks_do_not_corrupt_characters() {
    for encoding in Encoding::TERMINAL {
        let bytes = encoding::encode("中文 ABC", encoding, false).unwrap();
        let mut decoder = TerminalDecoder::new(encoding);
        let mut decoded = String::new();
        for byte in bytes {
            let (text, errors) = decoder.feed(&[byte]);
            assert!(!errors);
            decoded.push_str(&text);
        }
        assert_eq!(decoded, "中文 ABC");
    }
}

#[test]
fn terminal_ansi_search_and_selection_keep_spaces_and_wide_cells() {
    let mut term = TerminalBuffer::new(Encoding::Utf8);
    term.feed("\x1b[31m中文  test\x1b[0m\r\nsecond".as_bytes());
    assert_eq!(term.term.grid()[Point::new(Line(0), Column(0))].c, '中');
    assert_eq!(term.search("中文  test"), 1);
    assert_eq!(term.search("中文 test"), 0);
    term.select_start(0, 0, false);
    term.select_to(9, 0);
    assert_eq!(term.selected_text().unwrap().trim_end(), "中文  test");
    term.feed(b"\x1b[?1049hALT\x1b[?1049l");
    assert_eq!(term.search("second"), 1);
    assert!(term.resize(100, 30));
    assert!(!term.resize(100, 30));
}

#[test]
fn shell_reports_require_nonce_and_never_use_input_capture() {
    let token = "isolated-token";
    let mut parser = HistoryParser::new(token.into());
    let encode = |s: &str| base64::engine::general_purpose::STANDARD.encode(s);
    let report = format!(
        "\x1b]777;mantash;{token};{};{}\x07",
        encode("echo 中文"),
        encode("/tmp/work space")
    );
    let mut reports = Vec::new();
    for chunk in report.as_bytes().chunks(3) {
        reports.extend(parser.feed(chunk));
    }
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].command, "echo 中文");
    assert_eq!(reports[0].directory, "/tmp/work space");
    assert!(parser.feed(b"password: secret\r\n").is_empty());
    assert!(
        parser
            .feed(report.replace(token, "spoofed").as_bytes())
            .is_empty()
    );
    assert!(
        parser
            .feed(
                format!(
                    "\x1b]777;mantash;{token};{};{}\x07",
                    encode(" secret command"),
                    encode("/tmp")
                )
                .as_bytes()
            )
            .is_empty()
    );
    assert!(
        parser
            .feed(format!("\x1b]{}\x07", "x".repeat(40_000)).as_bytes())
            .is_empty()
    );
}

#[test]
fn terminal_keys_and_paste_preserve_shell_control_semantics() {
    assert_eq!(
        terminal::key_bytes("c", true, false, false, TermMode::empty()),
        Some(vec![3])
    );
    assert_eq!(
        terminal::key_bytes("up", false, false, false, TermMode::APP_CURSOR),
        Some(b"\x1bOA".to_vec())
    );
    assert_eq!(
        terminal::key_bytes("left", true, false, false, TermMode::empty()),
        Some(b"\x1b[1;5D".to_vec())
    );
    let mut term = TerminalBuffer::new(Encoding::Utf8);
    term.feed(b"\x1b[?2004h");
    assert_eq!(term.paste("a\nb").unwrap(), b"\x1b[200~a\nb\x1b[201~");
    assert_eq!(term.paste("\x1b[201~").unwrap(), b"\x1b[200~[201~\x1b[201~");
}

#[test]
fn database_round_trip_recovery_and_scoped_deletion() {
    let dir = tempfile::tempdir().unwrap();
    let mut db = Database::open(dir.path()).unwrap();
    let profile = profile();
    db.save_profiles(std::slice::from_ref(&profile)).unwrap();
    let a = HistoryEntry {
        id: Id::new_v4(),
        scope: "local".into(),
        command: "echo one".into(),
        timestamp: 1,
    };
    let b = HistoryEntry {
        id: Id::new_v4(),
        scope: profile.scope(),
        command: "echo two".into(),
        timestamp: 2,
    };
    let c = HistoryEntry {
        id: Id::new_v4(),
        scope: "ssh".into(),
        command: "echo two".into(),
        timestamp: 3,
    };
    db.touch_history(&a).unwrap();
    db.touch_history(&b).unwrap();
    db.delete_history(&[a.id]).unwrap();
    // Same command in the shared SSH list replaces the older copy regardless of scope form.
    db.touch_history(&c).unwrap();
    db.trust(&profile.host, profile.port, "SHA256:verified")
        .unwrap();
    assert_eq!(
        db.fingerprint(&profile.host.to_uppercase(), profile.port)
            .unwrap()
            .as_deref(),
        Some("SHA256:verified")
    );
    let mut prefs = Preferences::default();
    prefs.ui_size = 17.;
    prefs.terminal_size = 13.;
    db.save_workspace(&prefs, &Workspace::default()).unwrap();
    drop(db);
    let snapshot = Database::open(dir.path()).unwrap().load().unwrap();
    assert_eq!(snapshot.profiles, vec![profile]);
    assert_eq!(snapshot.history.len(), 1);
    assert_eq!(snapshot.history[0].id, c.id);
    assert_eq!(snapshot.preferences.ui_size, 17.);
    assert_eq!(snapshot.preferences.terminal_size, 13.);
}

#[test]
fn malformed_database_is_preserved() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("mantash.sqlite3");
    let bytes = b"not a sqlite database";
    std::fs::write(&path, bytes).unwrap();
    assert!(Database::open(dir.path()).is_err());
    assert_eq!(std::fs::read(path).unwrap(), bytes);
}

#[test]
fn font_sizes_are_bounded_to_the_12_to_18_range() {
    let mut prefs = Preferences::default();
    prefs.ui_size = 30.;
    prefs.terminal_size = 4.;
    prefs.normalize();
    assert_eq!((prefs.ui_size, prefs.terminal_size), (18., 12.));
    prefs.ui_size = 9.;
    prefs.terminal_size = 44.;
    prefs.normalize();
    assert_eq!((prefs.ui_size, prefs.terminal_size), (12., 18.));
    prefs.ui_size = 15.;
    prefs.terminal_size = 13.;
    prefs.normalize();
    assert_eq!((prefs.ui_size, prefs.terminal_size), (15., 13.));
    prefs.ui_size = f32::NAN;
    prefs.normalize();
    assert_eq!(prefs.ui_size, 14.);
}

#[test]
fn restoration_recovers_sessions_from_a_damaged_layout() {
    let spec = SessionSpec::Local {
        shell: "/bin/sh".into(),
        directory: "/tmp".into(),
        encoding: Encoding::Utf8,
    };
    let panes: Vec<_> = (0..3).map(|_| SavedPane::new(spec.clone())).collect();
    let ids: Vec<_> = panes.iter().map(|p| p.id).collect();
    let mut workspace = Workspace {
        tabs: vec![SavedTab {
            id: Id::new_v4(),
            panes,
            active_pane: Id::nil(),
            layout: None,
        }],
        active_tab: 42,
    };
    workspace.normalize();
    assert_eq!(workspace.tabs.len(), 3);
    assert_eq!(
        workspace.tabs.iter().map(|t| t.panes.len()).sum::<usize>(),
        3
    );
    assert_eq!(workspace.tabs[0].active_pane, ids[0]);
    assert_eq!(workspace.active_tab, 0);
}

#[test]
fn linux_cpu_and_network_rates_need_a_real_second_sample() {
    let sample = "__MANTASH_OS__\nLinux\n6.8\ntest\n100 0\n__MANTASH_CPU__\ncpu 10 0 10 80 0 0 0 0\n__MANTASH_MEM__\nMemTotal: 1000 kB\nMemAvailable: 400 kB\nSwapTotal: 100 kB\nSwapFree: 90 kB\n__MANTASH_NET__\neth0: 1000 0 0 0 0 0 0 0 2000 0 0 0 0 0 0 0\n__MANTASH_DISK__\nFilesystem 1024-blocks Used Available Capacity Mounted on\n/dev/sda1 100 30 70 30% /\n__MANTASH_PROCESS__\nPID PPID %CPU %MEM RSS STAT COMMAND\n1 0 0.1 0.2 100 S init\n__MANTASH_PORT__\nNetid State Recv-Q Send-Q Local Peer Process\ntcp LISTEN 0 128 0.0.0.0:22 0.0.0.0:* users:sshd\n";
    let first = monitor::parse(sample, None, 100);
    assert_eq!(first.cpu[0].percent, None);
    assert_eq!(first.network[0].receive_rate, None);
    assert!(first.errors.is_empty());
    let updated = sample
        .replace("100 0\n__MANTASH_CPU", "102 0\n__MANTASH_CPU")
        .replace("10 0 10 80", "30 0 30 110")
        .replace("eth0: 1000", "eth0: 1200");
    let second = monitor::parse(&updated, Some(&first), 102);
    assert!((second.cpu[0].percent.unwrap() - 4000. / 70.).abs() < 0.01);
    assert_eq!(second.network[0].receive_rate, Some(100.));
    assert!(
        monitor::parse("__MANTASH_OS__\nDarwin\n", None, 0)
            .errors
            .contains_key("system")
    );
}

#[test]
fn path_components_and_shell_quoting_cannot_escape_targets() {
    assert!(mantash::files::valid_name("12:30.log").is_ok());
    for name in ["12:30.log", "C:escape", "CON.txt", "LPT1", "trailing."] {
        assert!(mantash::files::valid_download_name(name, true).is_err());
    }
    assert!(mantash::files::valid_download_name("中文 report.txt", true).is_ok());
    for name in ["..", ".", "/root", "../secret", "back\\slash", "a\0b"] {
        assert!(mantash::files::valid_name(name).is_err());
    }
    assert_eq!(
        mantash::files::join("/home/a", "hello world.txt").unwrap(),
        "/home/a/hello world.txt"
    );
    assert_eq!(mantash::files::parent("/"), "/");
    assert_eq!(mantash::files::parent("//"), "/");
    assert_eq!(mantash::files::parent("/home/"), "/");
    assert_eq!(mantash::files::parent("/home/a/"), "/home");
    assert_eq!(mantash::integration::quote("a'b$(pwd)"), "'a'\\''b$(pwd)'");
}

#[test]
fn empty_port_section_is_a_valid_empty_snapshot() {
    let sample = monitor::parse(
        "__MANTASH_OS__\nLinux\n6.8\ntest\n100 0\n__MANTASH_PORT__\n",
        None,
        100,
    );
    assert!(sample.ports.is_empty());
    assert!(!sample.errors.contains_key("ports"));
}

#[test]
fn csv_header_must_have_exactly_five_unique_native_columns() {
    let valid = "NAME,host,PORT,username,password\nDemo,example.test,22,user,secret\n";
    assert!(connections::preview(valid, &[]).is_ok());
    for header in [
        "name,host,port,username\n",
        "name,host,port,username,password,extra\n",
        "name,host,port,username,username\n",
        "name,host,port,password,password\n",
    ] {
        assert!(connections::preview(header, &[]).is_err(), "{header}");
    }
}

#[test]
fn completed_and_cancelled_transfers_reject_late_progress() {
    let owner = Owner::new();
    let mut task = TransferRecord {
        id: Id::new_v4(),
        profile: profile(),
        upload: true,
        local: "a".into(),
        remote: "/a".into(),
        session: Some(owner.session),
        attempt: Some(owner.attempt),
        state: TransferState::Queued,
        bytes: 0,
        total: None,
        error: None,
        timestamp: 0,
    };
    assert!(task.belongs_to(owner));
    assert!(!task.belongs_to(Owner {
        session: owner.session,
        attempt: Id::new_v4()
    }));
    task.progress(10, None);
    assert_eq!(task.state, TransferState::Running);
    task.state = TransferState::Cancelled;
    task.progress(100, Some(100));
    assert_eq!(task.state, TransferState::Cancelled);
    assert_eq!(task.bytes, 10);
}
