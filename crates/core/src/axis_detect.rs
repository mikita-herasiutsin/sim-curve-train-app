//! Automatic detection of active pedal axes during configuration wizard sweeps.

use crate::input::MAX_AXES;

/// Minimum axis travel (`max - min`) required for an axis movement to be considered.
///
/// Corresponds to approximately 30% of the full 16-bit range (`65535 * 0.30`).
pub const MIN_TRAVEL: u32 = 19_661;

/// The outcome of an axis detection sweep.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Detection {
    /// No non-excluded axis reached [`MIN_TRAVEL`].
    NoMovement,
    /// Multiple axes moved significantly, or the leading axis travel was less than
    /// twice the travel of the second-best non-excluded axis.
    Ambiguous {
        /// Every non-excluded axis with travel >= [`MIN_TRAVEL`], sorted by descending travel.
        candidates: Vec<usize>,
    },
    /// A single axis decisively won the sweep.
    Axis {
        /// Zero-based index of the winning axis.
        index: usize,
        /// Baseline (resting) value observed on the first sample.
        rest: i16,
        /// Minimum raw value observed during the sweep.
        min: i16,
        /// Maximum raw value observed during the sweep.
        max: i16,
    },
}

/// Detects pedal movement across samples by tracking baseline, min, and max values per axis.
#[derive(Debug, Clone)]
pub struct AxisDetector {
    axis_count: usize,
    sample_count: usize,
    baseline: Vec<i16>,
    min: Vec<i16>,
    max: Vec<i16>,
    initialized: Vec<bool>,
}

impl AxisDetector {
    /// Creates a new `AxisDetector` tracking up to `axis_count` axes (clamped to [`MAX_AXES`]).
    #[must_use]
    pub fn new(axis_count: usize) -> Self {
        let count = axis_count.min(MAX_AXES);
        Self {
            axis_count: count,
            sample_count: 0,
            baseline: vec![0; count],
            min: vec![0; count],
            max: vec![0; count],
            initialized: vec![false; count],
        }
    }

    /// Observes a sample of axis values, ignoring any axes beyond the configured count.
    pub fn observe(&mut self, axes: &[i16]) {
        let count = self.axis_count.min(axes.len());
        for (i, &val) in axes.iter().take(count).enumerate() {
            if self.initialized[i] {
                self.min[i] = self.min[i].min(val);
                self.max[i] = self.max[i].max(val);
            } else {
                self.baseline[i] = val;
                self.min[i] = val;
                self.max[i] = val;
                self.initialized[i] = true;
            }
        }
        self.sample_count += 1;
    }

    /// Returns the per-axis travel (maximum observed minus minimum observed raw value).
    #[must_use]
    pub fn travel(&self) -> Vec<u32> {
        let mut travels = Vec::with_capacity(self.axis_count);
        for i in 0..self.axis_count {
            if self.initialized[i] {
                let diff = i32::from(self.max[i]) - i32::from(self.min[i]);
                travels.push(u32::try_from(diff).unwrap_or(0));
            } else {
                travels.push(0);
            }
        }
        travels
    }

    /// Returns the total number of sample observations recorded.
    #[must_use]
    pub fn sample_count(&self) -> usize {
        self.sample_count
    }

    /// Evaluates the sweep results, ignoring any axis indices in `exclude`.
    #[must_use]
    pub fn result(&self, exclude: &[usize]) -> Detection {
        if self.sample_count == 0 {
            return Detection::NoMovement;
        }

        let travels = self.travel();
        let mut ranked = Vec::new();
        for (i, &travel) in travels.iter().enumerate() {
            if !exclude.contains(&i) {
                ranked.push((i, travel));
            }
        }

        ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

        if ranked.is_empty() || ranked[0].1 < MIN_TRAVEL {
            return Detection::NoMovement;
        }

        let best = ranked[0];
        if ranked.len() >= 2 {
            let second_travel = ranked[1].1;
            let best_travel_u64 = u64::from(best.1);
            let second_travel_u64 = u64::from(second_travel);
            if best_travel_u64 < 2 * second_travel_u64 {
                let candidates: Vec<usize> = ranked
                    .into_iter()
                    .filter(|(_, t)| *t >= MIN_TRAVEL)
                    .map(|(idx, _)| idx)
                    .collect();
                return Detection::Ambiguous { candidates };
            }
        }

        let best_idx = best.0;
        Detection::Axis {
            index: best_idx,
            rest: self.baseline[best_idx],
            min: self.min[best_idx],
            max: self.max[best_idx],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_samples_returns_no_movement() {
        let detector = AxisDetector::new(4);
        assert_eq!(detector.sample_count(), 0);
        assert_eq!(detector.travel(), vec![0, 0, 0, 0]);
        assert_eq!(detector.result(&[]), Detection::NoMovement);
    }

    #[test]
    fn small_noise_only_returns_no_movement() {
        let mut detector = AxisDetector::new(3);
        detector.observe(&[100, -200, 50]);
        detector.observe(&[150, -180, 70]);
        detector.observe(&[80, -220, 30]);

        assert_eq!(detector.sample_count(), 3);
        // Travel: axis 0 = 70, axis 1 = 40, axis 2 = 40 (all < MIN_TRAVEL)
        assert_eq!(detector.travel(), vec![70, 40, 40]);
        assert_eq!(detector.result(&[]), Detection::NoMovement);
    }

    #[test]
    fn one_axis_swept_detects_axis_with_correct_rest_min_max() {
        let mut detector = AxisDetector::new(3);
        // Baseline: rest at -30000
        detector.observe(&[-30000, 0, 0]);
        // Swept up to 25000
        detector.observe(&[-10000, 10, 5]);
        detector.observe(&[25000, -10, 0]);
        // Release back to -30000
        detector.observe(&[-30000, 0, 0]);

        assert_eq!(
            detector.result(&[]),
            Detection::Axis {
                index: 0,
                rest: -30000,
                min: -30000,
                max: 25000,
            }
        );
    }

    #[test]
    fn inverted_pedal_sweep() {
        let mut detector = AxisDetector::new(2);
        // Baseline: rest at 32767
        detector.observe(&[32767, 0]);
        // Pressed fully to -32768
        detector.observe(&[-32768, 0]);
        // Released back to 32767
        detector.observe(&[32767, 0]);

        assert_eq!(
            detector.result(&[]),
            Detection::Axis {
                index: 0,
                rest: 32767,
                min: -32768,
                max: 32767,
            }
        );
    }

    #[test]
    fn two_axes_swept_similarly_returns_ambiguous() {
        let mut detector = AxisDetector::new(3);
        detector.observe(&[-30000, -30000, 0]);
        detector.observe(&[20000, 15000, 0]); // axis 0 travel = 50000, axis 1 travel = 45000
        detector.observe(&[-30000, -30000, 0]);

        // 50000 is not >= 2 * 45000 (90000), so Ambiguous
        assert_eq!(
            detector.result(&[]),
            Detection::Ambiguous {
                candidates: vec![0, 1],
            }
        );
    }

    #[test]
    fn exclusion_picks_other_axis() {
        let mut detector = AxisDetector::new(3);
        // Axis 0 has large travel (60000), axis 1 has large travel (40000)
        detector.observe(&[-30000, -20000, 0]);
        detector.observe(&[30000, 20000, 0]);

        // When axis 0 is excluded, axis 1 wins decisively
        assert_eq!(
            detector.result(&[0]),
            Detection::Axis {
                index: 1,
                rest: -20000,
                min: -20000,
                max: 20000,
            }
        );
    }

    #[test]
    fn noise_on_other_axes_does_not_win() {
        let mut detector = AxisDetector::new(3);
        detector.observe(&[-30000, 100, 500]);
        detector.observe(&[25000, 200, 600]); // axis 0 travel 55000, axis 1 travel 100, axis 2 travel 100

        // 55000 >= 2 * 100 (200), so axis 0 wins decisively despite noise on axes 1 and 2
        assert_eq!(
            detector.result(&[]),
            Detection::Axis {
                index: 0,
                rest: -30000,
                min: -30000,
                max: 25000,
            }
        );
    }

    #[test]
    fn more_observed_axes_than_axis_count_are_ignored() {
        let mut detector = AxisDetector::new(2);
        // Provide 4 values, but detector only tracks 2 axes
        detector.observe(&[0, 0, -30000, 30000]);
        detector.observe(&[0, 0, 30000, -30000]);

        // Even though axis 2 had large travel, it was ignored because axis_count == 2
        assert_eq!(detector.travel().len(), 2);
        assert_eq!(detector.travel(), vec![0, 0]);
        assert_eq!(detector.result(&[]), Detection::NoMovement);
    }

    #[test]
    fn axis_count_clamped_to_max_axes() {
        let detector = AxisDetector::new(100);
        assert_eq!(detector.travel().len(), MAX_AXES);
    }
}
