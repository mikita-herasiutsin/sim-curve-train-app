//! Summary of a drill set: several reps of the same drill.
//!
//! The summary reports the best and average rep totals plus a **consistency** sub-score: how
//! close the rep totals are to each other. Consistency uses the population standard deviation
//! of the totals, mapped linearly so that identical reps score 100 and a spread of
//! [`CONSISTENCY_ZERO_STD`] points or more scores 0. An aborted set is summarised from the reps
//! that were completed.

use serde::Serialize;

use crate::scoring::Grade;

/// Standard deviation of rep totals (in score points) at which consistency reaches 0.
pub const CONSISTENCY_ZERO_STD: f32 = 20.0;

/// Best, average and consistency of a set of rep totals.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetSummary {
    /// Rep totals in the order they were completed.
    pub rep_totals: Vec<f32>,
    pub best: f32,
    pub average: f32,
    /// Grade of the average total.
    pub grade: Grade,
    /// 0–100; 100 when all reps scored the same. `None` with fewer than two reps.
    pub consistency: Option<f32>,
    /// Population standard deviation of the rep totals.
    pub std_dev: f32,
}

/// Summarises the totals of the completed reps. Returns `None` if no rep was completed.
#[must_use]
pub fn summarize_set(rep_totals: &[f32]) -> Option<SetSummary> {
    if rep_totals.is_empty() {
        return None;
    }
    let count = f64::from(u32::try_from(rep_totals.len()).unwrap_or(u32::MAX));
    let values = rep_totals.iter().map(|&t| f64::from(t));
    let mean = values.clone().sum::<f64>() / count;
    let variance = values.map(|t| (t - mean).powi(2)).sum::<f64>() / count;
    #[expect(
        clippy::cast_possible_truncation,
        reason = "scores are within 0..=100; f32 is exact enough"
    )]
    let (average, std_dev) = (mean as f32, variance.sqrt() as f32);
    let best = rep_totals.iter().copied().fold(f32::MIN, f32::max);
    let consistency = (rep_totals.len() >= 2)
        .then(|| (100.0 * (1.0 - std_dev / CONSISTENCY_ZERO_STD)).clamp(0.0, 100.0));
    Some(SetSummary {
        rep_totals: rep_totals.to_vec(),
        best,
        average,
        grade: Grade::from_total(average),
        consistency,
        std_dev,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    #[test]
    fn no_reps_gives_none() {
        assert_eq!(summarize_set(&[]), None);
    }

    #[test]
    fn single_rep_has_no_consistency() {
        let s = summarize_set(&[88.0]).unwrap();
        assert!(close(s.best, 88.0) && close(s.average, 88.0));
        assert_eq!(s.consistency, None);
        assert_eq!(s.grade, Grade::A);
    }

    #[test]
    fn identical_reps_are_fully_consistent() {
        let s = summarize_set(&[90.0, 90.0, 90.0]).unwrap();
        assert!(close(s.consistency.unwrap(), 100.0));
        assert!(close(s.std_dev, 0.0));
    }

    #[test]
    fn best_average_and_spread() {
        // Mean 80, population std 10 -> consistency 50.
        let s = summarize_set(&[70.0, 90.0]).unwrap();
        assert!(close(s.best, 90.0));
        assert!(close(s.average, 80.0));
        assert!(close(s.std_dev, 10.0));
        assert!(close(s.consistency.unwrap(), 50.0));
        assert_eq!(s.grade, Grade::B);
        assert_eq!(s.rep_totals, vec![70.0, 90.0]);
    }

    #[test]
    fn large_spread_floors_at_zero() {
        let s = summarize_set(&[10.0, 100.0]).unwrap();
        assert!(close(s.consistency.unwrap(), 0.0));
    }

    #[test]
    fn steadier_set_is_more_consistent() {
        let steady = summarize_set(&[84.0, 86.0, 85.0, 85.0]).unwrap();
        let erratic = summarize_set(&[70.0, 98.0, 80.0, 92.0]).unwrap();
        assert!(steady.consistency.unwrap() > erratic.consistency.unwrap());
    }

    #[test]
    fn serializes_camel_case() {
        let json = serde_json::to_value(summarize_set(&[80.0, 90.0]).unwrap()).unwrap();
        assert_eq!(json["repTotals"], serde_json::json!([80.0, 90.0]));
        assert!(json.get("stdDev").is_some());
    }
}
