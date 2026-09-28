//! Bounded, timestamped Linux trends; missing samples stay missing and host reboots reset history.
use crate::monitor::Sample;
use std::collections::VecDeque;

/// Display exactly three minutes of sample time.
pub const WINDOW_SECONDS: i64 = 180;
const MAX_POINTS: usize = 61;

/// A sample containing optional percentages and aggregate external-interface byte rates.
#[derive(Clone, Debug)]
pub struct Point {
    pub timestamp: i64,
    pub cpu: Option<f64>,
    pub memory: Option<f64>,
    pub received: Option<f64>,
    pub sent: Option<f64>,
}

/// Per-connection memory only; reboot changes discard incompatible samples.
#[derive(Default)]
pub struct History {
    pub points: VecDeque<Point>,
    boot_id: Option<String>,
}

/// Discard invalid measurements instead of inventing zero values.
fn finite(value: Option<f64>) -> Option<f64> {
    value.filter(|v| v.is_finite() && *v >= 0.)
}
impl History {
    /// Accept increasing timestamps, retaining at most three minutes and 61 points.
    pub fn push(&mut self, sample: &Sample) {
        if sample.system != "Linux" {
            return;
        }
        if self.boot_id != sample.boot_id {
            self.points.clear();
            self.boot_id = sample.boot_id.clone();
        }
        if self
            .points
            .back()
            .is_some_and(|p| p.timestamp >= sample.timestamp)
        {
            return;
        }
        let interfaces: Vec<_> = sample.network.iter().filter(|n| n.name != "lo").collect();
        let rate = |receive: bool| {
            if interfaces.is_empty() {
                return None;
            }
            finite(
                interfaces
                    .iter()
                    .map(|n| if receive { n.receive_rate } else { n.send_rate })
                    .sum(),
            )
        };
        self.points.push_back(Point {
            timestamp: sample.timestamp,
            cpu: finite(sample.cpu.first().and_then(|cpu| cpu.percent)).filter(|p| *p <= 100.),
            memory: sample
                .memory
                .as_ref()
                .filter(|m| m.total > 0)
                .map(|m| 100. * m.total.saturating_sub(m.available) as f64 / m.total as f64),
            received: rate(true),
            sent: rate(false),
        });
        while self.points.len() > MAX_POINTS
            || self
                .points
                .front()
                .is_some_and(|p| p.timestamp < sample.timestamp - WINDOW_SECONDS)
        {
            self.points.pop_front();
        }
    }
    /// A gap longer than two refresh intervals starts a new line segment.
    pub fn series(&self, field: fn(&Point) -> Option<f64>) -> Vec<Vec<(f32, f32)>> {
        let Some(last) = self.points.back() else {
            return vec![];
        };
        let mut segments = vec![];
        let mut segment = vec![];
        let mut previous = None;
        for point in &self.points {
            let value = finite(field(point));
            if value.is_none() || previous.is_some_and(|time| point.timestamp - time > 6) {
                if !segment.is_empty() {
                    segments.push(std::mem::take(&mut segment));
                }
            }
            if let Some(value) = value {
                segment.push((
                    (point.timestamp - last.timestamp + WINDOW_SECONDS) as f32
                        / WINDOW_SECONDS as f32,
                    value as f32,
                ));
            }
            previous = Some(point.timestamp);
        }
        if !segment.is_empty() {
            segments.push(segment);
        }
        segments
    }
}
