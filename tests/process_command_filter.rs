//! Process display and filtering share the sampled executable-and-arguments string.
use mantash::monitor;

#[test]
fn sampled_commands_include_arguments_and_filter_only_displayed_commands() {
    let sample = monitor::parse(
        "__MANTASH_OS__\nLinux\n6.8\nfixture\n1 0\n__MANTASH_PROCESS_V2__\n\
         12345 1 0.1 0.2 256 S alice /usr/bin/node /srv/api.js --port=8001\n\
         23456 1 0.1 0.2 256 S bob /usr/bin/node /srv/worker.js --queue=邮件\n\
         34567 1 0.1 0.2 256 S carol /usr/bin/python task.py --label=alice-12345\n",
        None,
        0,
    );
    assert_eq!(sample.processes.len(), 3);
    assert_eq!(
        sample.processes[0].command,
        "/usr/bin/node /srv/api.js --port=8001"
    );
    let matches = |query: &str| {
        sample
            .processes
            .iter()
            .filter(|p| p.matches_command(query))
            .map(|p| p.pid)
            .collect::<Vec<_>>()
    };
    assert_eq!(matches(""), [12345, 23456, 34567]);
    assert_eq!(matches("NODE"), [12345, 23456]);
    assert_eq!(matches("--port=8001"), [12345]);
    assert_eq!(matches("worker.js --queue=邮件"), [23456]);
    assert_eq!(matches("alice"), [34567]);
    assert_eq!(matches("12345"), [34567]);
    assert!(matches("bob").is_empty());
    assert!(matches("23456").is_empty());
    assert!(matches("does-not-exist").is_empty());
    assert!(monitor::SAMPLE_COMMAND.contains("ps -ww -eo"));
    assert!(monitor::SAMPLE_COMMAND.contains("user:64=,args="));
    assert!(!monitor::SAMPLE_COMMAND.contains("comm="));
}
