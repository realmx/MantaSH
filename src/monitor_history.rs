//! Bounded, timestamped Linux trends; missing samples stay missing and host reboots reset history.
use crate::monitor::Sample;
use std::{collections::VecDeque, time::Duration};

/// Display exactly three minutes of sample time.
pub const WINDOW_SECONDS: i64 = 180;
const MAX_POINTS: usize = 61;
/// Longest gap, in whole seconds, still drawn as one continuous line.
///
/// The UI requests the next sample three seconds after the previous result
/// arrives, so the spacing between timestamps is three seconds plus the remote
/// command and SSH round-trip time, quantized to whole seconds. Rates are
/// computed from the previous accepted sample's counters, so every point
/// already covers the whole interval since that sample. Only a real pause, such
/// as a hidden overview or a stalled transport, should split the line; five
/// refresh intervals tolerate slow hosts without bridging such pauses.
pub const MAX_GAP_SECONDS: i64 = 15;
/// The UI waits this long after a result before requesting the next sample.
pub const REFRESH_INTERVAL: Duration = Duration::from_secs(3);

/// Whether a pane missed a refresh this tick: its next sample is due, none is in
/// flight, and nothing samples it now (overview hidden, another dialog open, or a
/// background tab). Such a pane calls [`History::interrupt`] so the unobserved
/// interval stays a gap even when sampling resumes within [`MAX_GAP_SECONDS`].
pub fn missed_refresh(scheduled: bool, in_flight: bool, since_previous_result: Duration) -> bool {
    !scheduled && !in_flight && since_previous_result > REFRESH_INTERVAL
}

/// A sample containing optional percentages and aggregate external-interface byte rates.
#[derive(Clone, Debug)]
pub struct Point {
    pub timestamp: i64,
    pub cpu: Option<f64>,
    pub memory: Option<f64>,
    pub received: Option<f64>,
    pub sent: Option<f64>,
    /// Sampling was interrupted (failed request or missed refresh) between the previous point and this one.
    pub after_interruption: bool,
}

/// Per-connection memory only; reboot changes discard incompatible samples.
#[derive(Default)]
pub struct History {
    pub points: VecDeque<Point>,
    boot_id: Option<String>,
    interrupted: bool,
}

/// Discard invalid measurements instead of inventing zero values.
fn finite(value: Option<f64>) -> Option<f64> {
    value.filter(|v| v.is_finite() && *v >= 0.)
}
impl History {
    /// Mark that sampling stopped being continuous, so the next point starts a new segment.
    ///
    /// Call this for a failed sample or a missed refresh; the interval is then
    /// shown as unobserved even when it is shorter than [`MAX_GAP_SECONDS`].
    pub fn interrupt(&mut self) {
        self.interrupted = true;
    }
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
            after_interruption: std::mem::take(&mut self.interrupted),
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
    /// A missing value, a recorded interruption or a gap longer than [`MAX_GAP_SECONDS`]
    /// starts a new line segment.
    pub fn series(&self, field: fn(&Point) -> Option<f64>) -> Vec<Vec<(f32, f32)>> {
        let Some(last) = self.points.back() else {
            return vec![];
        };
        let mut segments = vec![];
        let mut segment = vec![];
        let mut previous = None;
        for point in &self.points {
            let value = finite(field(point));
            if value.is_none()
                || point.after_interruption
                || previous.is_some_and(|time| point.timestamp - time > MAX_GAP_SECONDS)
            {
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
