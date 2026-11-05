//! Monitoring trends preserve time, missing data and source identity.
use mantash::{
    monitor::{Cpu, Memory, Network, Sample},
    monitor_history::{History, MAX_GAP_SECONDS, REFRESH_INTERVAL, WINDOW_SECONDS, missed_refresh},
};
use std::time::Duration;

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
#[test]
fn slow_remote_samples_stay_one_continuous_network_line() {
    // The next request starts three seconds after the previous result, so a
    // slow remote command yields 6–8 second whole-second spacing. Those
    // consecutive rate samples must render as one line, not isolated dots.
    let mut history = History::default();
    let mut timestamp = 1_000;
    for step in [6, 7, 6, 8, 7, 6, 7, 7, 6, 8].iter().cycle().take(25) {
        history.push(&sample(timestamp));
        timestamp += step;
    }
    let last = history.points.back().unwrap().timestamp;
    let first = history.points.front().unwrap().timestamp;
    assert!(last - first <= WINDOW_SECONDS);
    for lines in [history.series(|p| p.received), history.series(|p| p.sent)] {
        assert_eq!(lines.len(), 1, "continuous traffic split into {lines:?}");
        assert_eq!(lines[0].len(), history.points.len());
        assert_eq!(lines[0].last().unwrap().0, 1.);
    }
}
#[test]
fn a_real_sampling_pause_still_splits_the_line() {
    let mut history = History::default();
    history.push(&sample(100));
    history.push(&sample(100 + MAX_GAP_SECONDS));
    history.push(&sample(101 + 2 * MAX_GAP_SECONDS));
    history.push(&sample(104 + 2 * MAX_GAP_SECONDS));
    let lines = history.series(|p| p.received);
    assert_eq!(lines.iter().map(Vec::len).collect::<Vec<_>>(), vec![2, 2]);
}
#[test]
fn failed_or_paused_sampling_splits_even_a_short_gap() {
    let mut history = History::default();
    history.push(&sample(100));
    history.push(&sample(103));
    // A failed request between 103 and 107 leaves that interval unobserved.
    history.interrupt();
    history.push(&sample(107));
    history.push(&sample(110));
    assert_eq!(
        history
            .series(|p| p.received)
            .iter()
            .map(Vec::len)
            .collect::<Vec<_>>(),
        vec![2, 2]
    );
    // The mark applies once; later regular samples continue the new segment.
    assert!(history.points[2].after_interruption && !history.points[3].after_interruption);
}
#[test]
fn only_an_unsampled_due_pane_counts_as_a_missed_refresh() {
    let due = REFRESH_INTERVAL + Duration::from_millis(250);
    // Hidden overview, another dialog or a background tab: the refresh is missed.
    assert!(missed_refresh(false, false, due));
    // The visible pane is sampled this tick; slow remote commands are in flight.
    assert!(!missed_refresh(true, false, due));
    assert!(!missed_refresh(false, true, Duration::from_secs(10)));
    // Not yet due, so hiding briefly between refreshes loses nothing.
    assert!(!missed_refresh(false, false, Duration::from_secs(1)));
}
