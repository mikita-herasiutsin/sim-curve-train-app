//! Sample stream bookkeeping: batches sent to the UI and their rate statistics.

use serde::Serialize;

use crate::input::RawSample;

/// Live statistics about the input stream, shown in the debug HUD.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamStats {
    /// Samples per second over the last measurement window.
    pub sample_rate_hz: f32,
    /// Age of the oldest sample in the batch when it was sent, in milliseconds.
    pub batch_age_ms: f32,
}

/// One batch of samples sent from the input thread to the UI.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SampleBatch {
    pub samples: Vec<RawSample>,
    pub stats: StreamStats,
}

/// Counts events over fixed windows to report a rate in Hz.
#[derive(Debug, Clone)]
pub struct RateMeter {
    window_us: u64,
    window_start_us: Option<u64>,
    count: u32,
    rate_hz: f32,
}

impl RateMeter {
    /// Creates a meter that recomputes the rate every `window_us` microseconds.
    ///
    /// # Panics
    ///
    /// Panics if `window_us` is zero.
    #[must_use]
    pub fn new(window_us: u64) -> Self {
        assert!(window_us > 0, "rate window must be positive");
        Self {
            window_us,
            window_start_us: None,
            count: 0,
            rate_hz: 0.0,
        }
    }

    /// Records one event at time `t_us` and returns the latest rate.
    pub fn record(&mut self, t_us: u64) -> f32 {
        let start = *self.window_start_us.get_or_insert(t_us);
        let elapsed = t_us.saturating_sub(start);
        if elapsed >= self.window_us {
            // Events in [start, t_us) over the elapsed time; the current event opens the next window.
            #[expect(
                clippy::cast_precision_loss,
                reason = "window lengths are far below f64's exact integer range"
            )]
            let seconds = elapsed as f64 / 1e6;
            #[expect(
                clippy::cast_possible_truncation,
                reason = "rates are small; f32 precision is plenty for display"
            )]
            let rate = (f64::from(self.count) / seconds) as f32;
            self.rate_hz = rate;
            self.window_start_us = Some(t_us);
            self.count = 0;
        }
        self.count += 1;
        self.rate_hz
    }

    /// The rate measured over the last complete window (0 until one completes).
    #[must_use]
    pub fn rate_hz(&self) -> f32 {
        self.rate_hz
    }

    /// Forgets all recorded events.
    pub fn reset(&mut self) {
        self.window_start_us = None;
        self.count = 0;
        self.rate_hz = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_until_first_window_completes() {
        let mut meter = RateMeter::new(1_000_000);
        for t in (0..999_000).step_by(1_000) {
            assert!(meter.record(t).abs() < f32::EPSILON);
        }
    }

    #[test]
    fn measures_1khz() {
        let mut meter = RateMeter::new(1_000_000);
        for t in (0..=1_000_000).step_by(1_000) {
            meter.record(t);
        }
        assert!(
            (meter.rate_hz() - 1000.0).abs() < 0.5,
            "{}",
            meter.rate_hz()
        );
    }

    #[test]
    fn measures_500hz_with_irregular_window_end() {
        let mut meter = RateMeter::new(1_000_000);
        for t in (0..=1_002_000).step_by(2_000) {
            meter.record(t);
        }
        assert!((meter.rate_hz() - 500.0).abs() < 2.0, "{}", meter.rate_hz());
    }

    #[test]
    fn reset_clears_rate() {
        let mut meter = RateMeter::new(1_000);
        for t in 0..10 {
            meter.record(t * 500);
        }
        assert!(meter.rate_hz() > 0.0);
        meter.reset();
        assert!(meter.rate_hz().abs() < f32::EPSILON);
    }

    #[test]
    fn batch_serializes_camel_case() {
        let batch = SampleBatch {
            samples: vec![RawSample::new(5, &[1, -2])],
            stats: StreamStats {
                sample_rate_hz: 1000.0,
                batch_age_ms: 4.5,
            },
        };
        let json = serde_json::to_value(&batch).unwrap();
        assert_eq!(json["samples"][0]["tUs"], 5);
        assert_eq!(json["samples"][0]["axisCount"], 2);
        assert_eq!(json["stats"]["sampleRateHz"], 1000.0);
        assert_eq!(json["stats"]["batchAgeMs"], 4.5);
    }
}
