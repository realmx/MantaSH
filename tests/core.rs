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
    let a = db
        .record_history(HistoryScope::Local, "echo one")
        .unwrap()
        .items[0]
        .id;
    let b = db
        .record_history(HistoryScope::Ssh, "echo two")
        .unwrap()
        .items[0]
        .id;
    db.delete_history(&[a]).unwrap();
    assert_eq!(
        db.record_history(HistoryScope::Ssh, "echo two")
            .unwrap()
            .items[0]
            .id,
        b
    );
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
    assert_eq!(snapshot.history.items.len(), 1);
    assert_eq!(snapshot.history.items[0].id, b);
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

fn move_source(path: &str, directory: bool) -> mantash::files::CopySource {
    mantash::files::CopySource {
        path: path.into(),
        directory,
        symlink: false,
    }
}

#[test]
fn move_plan_maps_each_source_into_the_target_in_batch_order() {
    use mantash::files::plan_move;
    let sources = [
        move_source("/srv/a/z.txt", false),
        move_source("/srv/a/中文 folder", true),
    ];
    assert_eq!(
        plan_move(&sources, "/srv/b").unwrap(),
        vec![
            ("/srv/a/z.txt".to_string(), "/srv/b/z.txt".to_string()),
            (
                "/srv/a/中文 folder".to_string(),
                "/srv/b/中文 folder".to_string()
            ),
        ]
    );
    // The root and a trailing slash are valid destinations; a sibling sharing a
    // prefix ("/srv/ab") is not a descendant of "/srv/a".
    assert_eq!(
        plan_move(&[move_source("/srv/a", true)], "/").unwrap(),
        vec![("/srv/a".to_string(), "/a".to_string())]
    );
    assert_eq!(
        plan_move(&[move_source("/srv/a", true)], "/srv/ab/").unwrap(),
        vec![("/srv/a".to_string(), "/srv/ab/a".to_string())]
    );
}

#[test]
fn move_plan_refuses_same_directory_self_nesting_duplicates_and_bad_paths() {
    use mantash::files::{MoveRejection, plan_move};
    let file = move_source("/srv/a/z.txt", false);
    let folder = move_source("/srv/a/folder", true);
    assert_eq!(plan_move(&[], "/srv/b"), Err(MoveRejection::Empty));
    assert_eq!(
        plan_move(&[file.clone()], "/srv/a"),
        Err(MoveRejection::SameDirectory)
    );
    assert_eq!(
        plan_move(&[file.clone()], "/srv/a/"),
        Err(MoveRejection::SameDirectory)
    );
    assert_eq!(
        plan_move(&[move_source("/top", true)], "/"),
        Err(MoveRejection::SameDirectory)
    );
    for target in [
        "/srv/a/folder",
        "/srv/a/folder/",
        "/srv/a/folder/inner/deep",
    ] {
        assert_eq!(
            plan_move(&[folder.clone()], target),
            Err(MoveRejection::IntoItself("/srv/a/folder".into())),
            "{target}"
        );
    }
    assert_eq!(
        plan_move(
            &[
                move_source("/srv/a/x", false),
                move_source("/srv/c/x", false)
            ],
            "/srv/b"
        ),
        Err(MoveRejection::DuplicateName("x".into()))
    );
    for target in ["relative", "/srv/../etc", "/srv/a\\b", ""] {
        assert!(
            matches!(
                plan_move(&[file.clone()], target),
                Err(MoveRejection::InvalidPath(_))
            ),
            "{target}"
        );
    }
    for source in ["/", "relative/x", "/srv/../x", "/srv/./x"] {
        assert!(
            matches!(
                plan_move(&[move_source(source, false)], "/srv/b"),
                Err(MoveRejection::InvalidPath(_))
            ),
            "{source}"
        );
    }
}

#[test]
fn move_conflicts_report_every_existing_destination_without_overwrite() {
    use mantash::files::{move_conflicts, plan_move};
    let plan = plan_move(
        &[
            move_source("/srv/a/keep.txt", false),
            move_source("/srv/a/.hidden", false),
            move_source("/srv/a/new", true),
        ],
        "/srv/b",
    )
    .unwrap();
    // Hidden names count: the destination listing check includes entries the
    // panel currently hides, so a filtered view cannot hide a collision.
    let existing = ["/srv/b/keep.txt", "/srv/b/.hidden", "/srv/b/other"];
    assert_eq!(
        move_conflicts(&plan, |path| existing.contains(&path)),
        vec!["/srv/b/keep.txt", "/srv/b/.hidden"]
    );
    assert!(move_conflicts(&plan, |_| false).is_empty());
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

#[test]
fn file_list_mtime_shows_seconds_in_zone_and_unknown_is_a_placeholder() {
    use chrono::{FixedOffset, Utc};
    use mantash::files::{UNKNOWN_MODIFIED, format_modified, format_modified_in};
    // 2026-10-21 09:39:59 UTC: seconds are shown as reported, not truncated or rounded.
    let mtime = Some(1_792_575_599);
    assert_eq!(format_modified_in(mtime, &Utc), "2026-10-21 09:39:59");
    let shanghai = FixedOffset::east_opt(8 * 3600).unwrap();
    assert_eq!(format_modified_in(mtime, &shanghai), "2026-10-21 17:39:59");
    let west = FixedOffset::west_opt(5 * 3600).unwrap();
    assert_eq!(format_modified_in(mtime, &west), "2026-10-21 04:39:59");
    // The epoch and the largest SFTP v3 value are real timestamps, not unknown.
    assert_eq!(format_modified_in(Some(0), &Utc), "1970-01-01 00:00:00");
    assert_eq!(
        format_modified_in(Some(u32::MAX), &Utc),
        "2106-02-07 06:28:15"
    );
    assert_eq!(format_modified_in(None, &shanghai), UNKNOWN_MODIFIED);
    assert_eq!(format_modified(None), UNKNOWN_MODIFIED);
    // Local time keeps the same fixed `YYYY-MM-DD HH:mm:ss` shape the column width assumes.
    let local = format_modified(mtime);
    assert_eq!(local.len(), "0000-00-00 00:00:00".len());
    assert!(
        local
            .chars()
            .zip("0000-00-00 00:00:00".chars())
            .all(|(c, shape)| {
                if shape == '0' {
                    c.is_ascii_digit()
                } else {
                    c == shape
                }
            })
    );
}

#[test]
fn listing_entry_carries_readdir_mtime_and_keeps_missing_attributes_unknown() {
    use mantash::files::listing_entry;
    use russh_sftp::protocol::FileAttributes;
    let mut attributes = FileAttributes::empty();
    attributes.set_regular(true);
    attributes.size = Some(42);
    attributes.mtime = Some(1_792_575_599);
    attributes.permissions = Some(attributes.permissions.unwrap_or(0) | 0o640);
    let entry = listing_entry("/srv/data/", "报告.txt".into(), &attributes).unwrap();
    assert_eq!(entry.path, "/srv/data/报告.txt");
    assert_eq!(entry.name, "报告.txt");
    assert!(!entry.directory && !entry.symlink);
    assert_eq!(entry.size, Some(42));
    assert_eq!(entry.modified, Some(1_792_575_599));
    assert_eq!(entry.permissions.unwrap() & 0o777, 0o640);

    let mut directory = FileAttributes::empty();
    directory.set_dir(true);
    let entry = listing_entry("/srv", "logs".into(), &directory).unwrap();
    assert!(entry.directory);
    assert_eq!(entry.size, None);
    assert_eq!(entry.modified, None);

    // Names that cannot form a safe child path are still rejected.
    assert!(listing_entry("/srv", "../escape".into(), &attributes).is_err());
}

fn sort_row(
    name: &str,
    directory: bool,
    size: Option<u64>,
    modified: Option<u32>,
) -> mantash::files::FileEntry {
    mantash::files::FileEntry {
        name: name.into(),
        path: format!("/srv/{name}"),
        directory,
        symlink: false,
        size,
        modified,
        permissions: None,
    }
}

fn sorted_names(
    entries: &[mantash::files::FileEntry],
    sort: mantash::files::FileSort,
) -> Vec<String> {
    let mut entries = entries.to_vec();
    mantash::files::sort_entries(&mut entries, sort);
    entries.into_iter().map(|entry| entry.name).collect()
}

#[test]
fn file_sort_header_cycles_descending_ascending_then_default() {
    use mantash::files::{FileSort, SortColumn};
    let size_desc = FileSort::By {
        column: SortColumn::Size,
        descending: true,
    };
    let size_asc = FileSort::By {
        column: SortColumn::Size,
        descending: false,
    };
    assert_eq!(FileSort::default(), FileSort::Default);
    assert_eq!(FileSort::Default.toggled(SortColumn::Size), size_desc);
    assert_eq!(size_desc.toggled(SortColumn::Size), size_asc);
    assert_eq!(size_asc.toggled(SortColumn::Size), FileSort::Default);
    // Switching columns always starts the new column descending.
    assert_eq!(
        size_asc.toggled(SortColumn::Modified),
        FileSort::By {
            column: SortColumn::Modified,
            descending: true
        }
    );
    assert_eq!(size_desc.direction(SortColumn::Size), Some(true));
    assert_eq!(size_asc.direction(SortColumn::Size), Some(false));
    assert_eq!(size_desc.direction(SortColumn::Modified), None);
    assert_eq!(FileSort::Default.direction(SortColumn::Size), None);
}

#[test]
fn file_sort_keeps_folders_first_unknown_last_and_breaks_ties_by_name() {
    use mantash::files::{FileSort, SortColumn};
    let rows = vec![
        sort_row("b.log", false, Some(10), Some(300)),
        sort_row("Zeta", true, None, Some(100)),
        sort_row("a.txt", false, Some(10), Some(200)),
        sort_row("unknown.bin", false, None, None),
        sort_row("alpha", true, None, None),
        sort_row("big.iso", false, Some(9_000), Some(100)),
        sort_row("Mid", true, None, Some(500)),
        sort_row("c.md", false, Some(5), Some(300)),
    ];
    let by = |column, descending| FileSort::By { column, descending };
    // Default: folders, then files, each case-insensitively by name.
    assert_eq!(
        sorted_names(&rows, FileSort::Default),
        [
            "alpha",
            "Mid",
            "Zeta",
            "a.txt",
            "b.log",
            "big.iso",
            "c.md",
            "unknown.bin"
        ]
    );
    // Size: folders have no size and keep name order; equal sizes fall back to name;
    // the unknown size stays last in both directions.
    assert_eq!(
        sorted_names(&rows, by(SortColumn::Size, true)),
        [
            "alpha",
            "Mid",
            "Zeta",
            "big.iso",
            "a.txt",
            "b.log",
            "c.md",
            "unknown.bin"
        ]
    );
    assert_eq!(
        sorted_names(&rows, by(SortColumn::Size, false)),
        [
            "alpha",
            "Mid",
            "Zeta",
            "c.md",
            "a.txt",
            "b.log",
            "big.iso",
            "unknown.bin"
        ]
    );
    // Modified: folders sort by time inside their group, unknown times last both ways.
    assert_eq!(
        sorted_names(&rows, by(SortColumn::Modified, true)),
        [
            "Mid",
            "Zeta",
            "alpha",
            "b.log",
            "c.md",
            "a.txt",
            "big.iso",
            "unknown.bin"
        ]
    );
    assert_eq!(
        sorted_names(&rows, by(SortColumn::Modified, false)),
        [
            "Zeta",
            "Mid",
            "alpha",
            "big.iso",
            "a.txt",
            "b.log",
            "c.md",
            "unknown.bin"
        ]
    );
    // Resetting from any prior order restores exactly the default order.
    let mut entries = rows.clone();
    mantash::files::sort_entries(&mut entries, by(SortColumn::Modified, true));
    mantash::files::sort_entries(&mut entries, FileSort::Default);
    assert_eq!(
        entries.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
        sorted_names(&rows, FileSort::Default)
    );
    // Case-only name differences resolve deterministically, independent of input order.
    let twins = vec![
        sort_row("readme", false, Some(1), None),
        sort_row("README", false, Some(1), None),
    ];
    let mut reversed = twins.clone();
    reversed.reverse();
    assert_eq!(
        sorted_names(&twins, by(SortColumn::Size, true)),
        sorted_names(&reversed, by(SortColumn::Size, true))
    );
}

#[test]
fn file_sort_reorders_display_without_changing_path_selection_ranges() {
    use mantash::file_selection::RowSelection;
    use mantash::files::{FileSort, SortColumn};
    let rows = vec![
        sort_row("a", false, Some(3), None),
        sort_row("b", false, Some(1), None),
        sort_row("c", false, Some(2), None),
    ];
    let mut entries = rows.clone();
    let visible = |entries: &[mantash::files::FileEntry]| {
        entries.iter().map(|e| e.path.clone()).collect::<Vec<_>>()
    };
    let mut selection = RowSelection::default();
    selection.click(&visible(&entries), "/srv/a", false, false);
    mantash::files::sort_entries(
        &mut entries,
        FileSort::By {
            column: SortColumn::Size,
            descending: false,
        },
    );
    // Identity survives the reorder; a Shift range then follows the displayed order b, c, a.
    assert!(selection.contains("/srv/a"));
    selection.click(&visible(&entries), "/srv/c", true, false);
    assert!(selection.contains("/srv/c") && selection.contains("/srv/a"));
    assert!(!selection.contains("/srv/b"));
}

#[test]
fn saved_password_reveal_reads_only_on_demand_for_saved_profiles() {
    use mantash::credentials::reveal::may_read_saved;
    assert!(may_read_saved(true, true, true, false));
    // New or cloned forms, memory off, typed input or a finished read never read.
    assert!(!may_read_saved(false, true, true, false));
    assert!(!may_read_saved(true, false, true, false));
    assert!(!may_read_saved(true, true, false, false));
    assert!(!may_read_saved(true, true, true, true));
}

#[test]
fn saved_password_read_applies_only_to_the_same_untouched_request() {
    use mantash::credentials::reveal::applies;
    let (request, profile) = (Id::new_v4(), Id::new_v4());
    assert!(applies(Some(request), profile, request, profile, true));
    assert!(
        !applies(None, profile, request, profile, true),
        "cancelled or closed"
    );
    assert!(
        !applies(Some(Id::new_v4()), profile, request, profile, true),
        "stale request"
    );
    assert!(
        !applies(Some(request), Id::new_v4(), request, profile, true),
        "other profile"
    );
    assert!(
        !applies(Some(request), profile, request, profile, false),
        "user typed meanwhile"
    );
}

#[test]
fn unchanged_revealed_password_is_not_submitted_as_new() {
    use mantash::credentials::reveal::submitted_secret;
    assert_eq!(submitted_secret("saved", Some("saved")), "");
    assert_eq!(submitted_secret("edited", Some("saved")), "edited");
    assert_eq!(submitted_secret("", Some("saved")), "");
    assert_eq!(submitted_secret("typed", None), "typed");
}
