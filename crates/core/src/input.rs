//! Input sample definitions and limits for game controllers.

use serde::Serialize;

/// Maximum number of joystick axes supported per sample.
pub const MAX_AXES: usize = 8;

/// A single timestamped sample containing raw joystick axis readings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RawSample {
    /// Monotonic timestamp in microseconds.
    pub t_us: u64,
    /// Number of active or used axes in [`axes`](Self::axes).
    pub axis_count: u8,
    /// Raw SDL joystick axis values (`-32768..=32767`), with unused axes zeroed.
    pub axes: [i16; MAX_AXES],
}

impl RawSample {
    /// Creates a new `RawSample` by copying at most [`MAX_AXES`] values from `axes`.
    ///
    /// Any unused entries in the internal axis array are set to zero.
    #[must_use]
    pub fn new(t_us: u64, axes: &[i16]) -> Self {
        let count = axes.len().min(MAX_AXES);
        let mut buffer = [0i16; MAX_AXES];
        buffer[..count].copy_from_slice(&axes[..count]);
        let axis_count = u8::try_from(count).unwrap_or_default();
        Self {
            t_us,
            axis_count,
            axes: buffer,
        }
    }

    /// Returns a slice containing only the used axes.
    #[must_use]
    pub fn axes(&self) -> &[i16] {
        let count = usize::from(self.axis_count).min(MAX_AXES);
        &self.axes[..count]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_with_empty_axes() {
        let sample = RawSample::new(100, &[]);
        assert_eq!(sample.t_us, 100);
        assert_eq!(sample.axis_count, 0);
        assert_eq!(sample.axes(), &[] as &[i16]);
        assert_eq!(sample.axes, [0; MAX_AXES]);
    }

    #[test]
    fn new_with_fewer_than_max_axes() {
        let values = [1000, -2000, 3000];
        let sample = RawSample::new(50, &values);
        assert_eq!(sample.t_us, 50);
        assert_eq!(sample.axis_count, 3);
        assert_eq!(sample.axes(), &[1000, -2000, 3000]);
        assert_eq!(sample.axes[3..], [0; 5]);
    }

    #[test]
    fn new_with_exact_max_axes() {
        let values = [1, 2, 3, 4, 5, 6, 7, 8];
        let sample = RawSample::new(42, &values);
        assert_eq!(sample.axis_count, 8);
        assert_eq!(sample.axes(), &values);
        assert_eq!(sample.axes, values);
    }

    #[test]
    fn new_with_more_than_max_axes() {
        let values = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
        let sample = RawSample::new(42, &values);
        assert_eq!(sample.axis_count, 8);
        assert_eq!(sample.axes(), &[1, 2, 3, 4, 5, 6, 7, 8]);
    }

    #[test]
    fn serialization_uses_camel_case() {
        let sample = RawSample::new(1234, &[10, 20]);
        let val = serde_json::to_value(sample).unwrap();
        assert_eq!(val["tUs"], 1234);
        assert_eq!(val["axisCount"], 2);
        assert_eq!(val["axes"], serde_json::json!([10, 20, 0, 0, 0, 0, 0, 0]));
    }
}
