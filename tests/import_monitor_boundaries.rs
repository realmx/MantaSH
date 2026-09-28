use mantash::{connections, files, monitor};

#[test]
fn csv_header_permutation_case_and_bom_import_real_profile_and_password() {
    let input =
        "\u{feff}PASSWORD,HoSt,Name,USERNAME,PoRt\nsecret,example.test,Build host,alice,2201\n";
    let preview = connections::preview(input, &[]).unwrap();

    assert_eq!(preview.rows.len(), 1);
    let row = &preview.rows[0];
    let profile = row.profile.as_ref().expect("valid CSV row should import");
    assert_eq!(profile.name, "Build host");
    assert_eq!(profile.host, "example.test");
    assert_eq!(profile.port, 2201);
    assert_eq!(profile.username, "alice");
    assert_eq!(row.password.as_deref(), Some("secret"));
    assert!(row.error.is_none());
}

#[test]
fn csv_header_rejects_case_insensitive_duplicates_extra_and_missing_columns() {
    for input in [
        "name,NAME,host,port,username,password\nDemo,Duplicate,example.test,22,alice,secret\n",
        "name,host,port,username,password,extra\nDemo,example.test,22,alice,secret,x\n",
        "name,host,port,username\nDemo,example.test,22,alice\n",
    ] {
        assert!(connections::preview(input, &[]).is_err(), "{input}");
    }
}

#[test]
fn csv_preview_keeps_valid_error_valid_rows_and_their_passwords() {
    let input = concat!(
        "name,host,port,username,password\n",
        "First,first.test,22,alice,first-secret\n",
        "Broken,broken.test,not-a-port,bob,discarded-secret\n",
        "Last,last.test,2202,carol,last-secret\n",
    );
    let preview = connections::preview(input, &[]).unwrap();

    assert_eq!(preview.rows.len(), 3);
    assert_eq!(preview.rows[0].profile.as_ref().unwrap().name, "First");
    assert_eq!(preview.rows[0].password.as_deref(), Some("first-secret"));
    assert!(preview.rows[0].error.is_none());
    assert!(preview.rows[1].profile.is_none());
    assert!(preview.rows[1].password.is_none());
    assert!(preview.rows[1].error.is_some());
    assert_eq!(preview.rows[2].profile.as_ref().unwrap().name, "Last");
    assert_eq!(preview.rows[2].password.as_deref(), Some("last-secret"));
    assert!(preview.rows[2].error.is_none());
}

fn parse_linux_port_section(port_output: &str) -> monitor::Sample {
    let output =
        format!("__MANTASH_OS__\nLinux\n6.8\ntest-host\n100 0\n__MANTASH_PORT__\n{port_output}");
    monitor::parse(&output, None, 100)
}

#[test]
fn port_parser_accepts_successful_empty_header_only_and_row_output() {
    let empty = parse_linux_port_section("");
    assert!(empty.ports.is_empty());
    assert!(!empty.errors.contains_key("ports"));

    let header_only = parse_linux_port_section(
        "Netid State Recv-Q Send-Q Local Address:Port Peer Address:Port Process\n",
    );
    assert!(header_only.ports.is_empty());
    assert!(!header_only.errors.contains_key("ports"));

    let with_rows = parse_linux_port_section(
        "Netid State Recv-Q Send-Q Local Peer Process\ntcp LISTEN 0 128 0.0.0.0:22 0.0.0.0:* users:((sshd,pid=1,fd=3))\n",
    );
    assert_eq!(with_rows.ports.len(), 1);
    assert_eq!(with_rows.ports[0].protocol, "tcp");
    assert_eq!(with_rows.ports[0].state, "LISTEN");
    assert_eq!(with_rows.ports[0].local, "0.0.0.0:22");
    assert_eq!(with_rows.ports[0].peer, "0.0.0.0:*");
    assert_eq!(with_rows.ports[0].process, "users:((sshd,pid=1,fd=3))");
    assert!(!with_rows.errors.contains_key("ports"));
}

#[test]
fn port_parser_preserves_missing_section_diagnostics_and_malformed_output_errors() {
    let missing = monitor::parse("__MANTASH_OS__\nLinux\n6.8\ntest-host\n100 0\n", None, 100);
    assert_eq!(
        missing.errors.get("ports").map(String::as_str),
        Some("Port sampling returned no section")
    );

    for output in [
        "Cannot open netlink socket: Operation not permitted\n",
        "MANTASH_ERROR: ss exited with status 1\n",
        "not an ss header\n",
        "Netid State Recv-Q Send-Q Local Peer Process\ntcp LISTEN invalid 0 0.0.0.0:22 0.0.0.0:*\n",
    ] {
        let sample = parse_linux_port_section(output);
        assert!(sample.ports.is_empty(), "{output}");
        assert!(
            sample
                .errors
                .get("ports")
                .is_some_and(|error| !error.is_empty()),
            "{output}"
        );
    }
}

#[test]
fn sampling_command_marks_a_silent_ss_failure() {
    assert!(monitor::SAMPLE_COMMAND.contains("ss -lntuap 2>&1 || {"));
    assert!(monitor::SAMPLE_COMMAND.contains("MANTASH_ERROR: ss exited with status %s"));
}

#[test]
fn delete_target_validation_rejects_root_relative_and_unsafe_components() {
    for path in ["/", "//", "relative", "/tmp/../secret", "/tmp/bad\\name"] {
        assert!(files::validate_delete_target(path).is_err(), "{path}");
    }
    assert!(files::validate_delete_target("/tmp/safe-name").is_ok());
}
