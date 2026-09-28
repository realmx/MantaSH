//! Monitoring trends preserve time, missing data and source identity.
use mantash::{
    monitor::{Cpu, Memory, Network, Sample},
    monitor_history::History,
};

/// Build a Linux sample with one valid external interface and an excluded loopback device.
fn sample(timestamp: i64) -> Sample {
    Sample {
        timestamp,
        system: "Linux".into(),
        boot_id: Some("boot-a".into()),
        cpu: vec![Cpu {
            percent: Some(25.),
            ..Default::default()
        }],
        memory: Some(Memory {
            total: 100,
            available: 75,
            ..Default::default()
        }),
        network: vec![
            Network {
                name: "eth0".into(),
                receive_rate: Some(128.),
                send_rate: Some(24.),
                ..Default::default()
            },
            Network {
                name: "lo".into(),
                receive_rate: Some(999.),
                send_rate: Some(999.),
                ..Default::default()
            },
        ],
        ..Default::default()
    }
}
#[test]
fn trends_use_real_timestamps_and_break_for_missing_samples() {
    let mut history = History::default();
    history.push(&sample(100));
    history.push(&sample(103));
    let mut missing = sample(106);
    missing.cpu[0].percent = None;
    history.push(&missing);
    history.push(&sample(109));
    history.push(&sample(130));
    let lines = history.series(|p| p.cpu);
    assert_eq!(
        lines.iter().map(Vec::len).collect::<Vec<_>>(),
        vec![2, 1, 1]
    );
    assert_eq!(lines.last().unwrap()[0].0, 1.);
    assert_eq!(history.points[0].received, Some(128.));
    assert_eq!(history.points[0].memory, Some(25.));
}
#[test]
fn history_is_bounded_and_rejects_late_samples_then_resets_on_reboot() {
    let mut history = History::default();
    for timestamp in (0..=600).step_by(3) {
        history.push(&sample(timestamp));
    }
    assert_eq!(history.points.len(), 61);
    assert_eq!(history.points.front().unwrap().timestamp, 420);
    history.push(&sample(599));
    assert_eq!(history.points.back().unwrap().timestamp, 600);
    let mut reboot = sample(603);
    reboot.boot_id = Some("boot-b".into());
    history.push(&reboot);
    assert_eq!(history.points.len(), 1);
}
#[test]
fn invalid_values_never_turn_into_successful_zero_measurements() {
    let mut history = History::default();
    let mut invalid = sample(100);
    invalid.cpu[0].percent = Some(f64::NAN);
    invalid.network[0].receive_rate = None;
    invalid.memory.as_mut().unwrap().total = 0;
    history.push(&invalid);
    let point = history.points.back().unwrap();
    assert!(point.cpu.is_none() && point.received.is_none() && point.memory.is_none());
    assert!(history.series(|p| p.cpu).is_empty());
    let mut other = sample(103);
    other.system = "Darwin".into();
    history.push(&other);
    assert_eq!(history.points.len(), 1);
}
