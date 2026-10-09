//! Statistical analysis and metric aggregation for pedal zones.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::csv::{LapTelemetry, TELEMETRY_HZ};
use crate::zones::{BrakeZone, ThrottleExitZone};

/// Rate threshold defining a fast throttle step in %/s (300 %/s).
pub const FAST_STEP_RATE_THRESHOLD: f64 = 300.0;
/// Minimum duration in frames (80 ms at 60 Hz = ~5 frames) to constitute a throttle plateau.
pub const MIN_PLATEAU_FRAMES: usize = 5;

/// Median and Interquartile Range (IQR) for a metric series.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct MetricSummary {
    /// Median (50th percentile).
    pub median: f64,
    /// Interquartile Range (75th percentile minus 25th percentile).
    pub iqr: f64,
}

impl MetricSummary {
    /// Computes median and IQR from a slice of values using linear interpolation.
    #[must_use]
    pub fn compute(values: &[f64]) -> Self {
        if values.is_empty() {
            return Self {
                median: 0.0,
                iqr: 0.0,
            };
        }

        let mut sorted = values.to_vec();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let median = percentile_sorted(&sorted, 0.50);
        let q1 = percentile_sorted(&sorted, 0.25);
        let q3 = percentile_sorted(&sorted, 0.75);
        let iqr = q3 - q1;

        Self { median, iqr }
    }
}

fn percentile_sorted(sorted: &[f64], p: f64) -> f64 {
    let n = sorted.len();
    if n == 0 {
        return 0.0;
    }
    if n == 1 {
        return sorted[0];
    }

    #[expect(
        clippy::cast_precision_loss,
        reason = "slice length safely converts to f64"
    )]
    let pos = p * (n - 1) as f64;
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "percentile rank fits in usize"
    )]
    let idx = pos.floor() as usize;
    let frac = pos - pos.floor();

    if idx + 1 < n {
        sorted[idx] + frac * (sorted[idx + 1] - sorted[idx])
    } else {
        sorted[idx]
    }
}

/// Raw metrics measured from a single throttle corner exit.
#[derive(Clone, Debug, PartialEq)]
pub struct ThrottleExitMetrics {
    /// Throttle percentage reached ~150 ms after onset (`0.0..=100.0`).
    pub initial_stab_level_pct: f64,
    /// Duration from onset to the end of the initial stab in seconds.
    pub time_to_stab_s: f64,
    /// Number of staged steps per zone (1 = single stab then ramp, 2 = stab, plateau, second stab).
    pub fast_steps: usize,
    /// Plateau level(s) in percent between staged steps.
    pub plateau_levels_pct: Vec<f64>,
    /// Progressive ramp rate from end of stab/plateau to 98% in %/s.
    pub progressive_ramp_rate_pct_s: f64,
    /// Total duration from onset to 98% full throttle in seconds.
    pub time_to_full_s: f64,
}

/// Analyzes an individual throttle exit zone.
#[must_use]
#[expect(
    clippy::too_many_lines,
    reason = "comprehensive staged throttle exit metrics analysis"
)]
pub fn analyze_throttle_exit(throttles: &[f32], zone: &ThrottleExitZone) -> ThrottleExitMetrics {
    let onset = zone.onset_idx;
    let full = zone.full_idx;

    #[expect(
        clippy::cast_precision_loss,
        reason = "sample count safely converts to f64"
    )]
    let time_to_full_s = (full.saturating_sub(onset)) as f64 / TELEMETRY_HZ;

    if full <= onset {
        return ThrottleExitMetrics {
            initial_stab_level_pct: 0.0,
            time_to_stab_s: 0.0,
            fast_steps: 0,
            plateau_levels_pct: Vec::new(),
            progressive_ramp_rate_pct_s: 0.0,
            time_to_full_s: 0.0,
        };
    }

    // 1. Initial stab level: throttle reached ~150 ms after onset (9 samples at 60 Hz)
    let idx_150ms = (onset + 9).min(full);
    let initial_stab_level_pct = f64::from(throttles[idx_150ms]) * 100.0;

    // 2. Compute derivative rate (%/s) across exit frames
    let num_frames = full - onset;
    let mut rates = Vec::with_capacity(num_frames);
    for i in onset..full {
        let dt = 1.0 / TELEMETRY_HZ;
        let dy = f64::from(throttles[i + 1] - throttles[i]) * 100.0;
        rates.push(dy / dt);
    }

    // 3. Identify fast steps (run of rate > 300 %/s)
    let mut fast_step_runs = Vec::new();
    let mut in_fast = false;
    let mut fast_start = 0;
    for (k, &r) in rates.iter().enumerate() {
        if r > FAST_STEP_RATE_THRESHOLD {
            if !in_fast {
                in_fast = true;
                fast_start = k;
            }
        } else if in_fast {
            in_fast = false;
            fast_step_runs.push((fast_start, k));
        }
    }
    if in_fast {
        fast_step_runs.push((fast_start, rates.len()));
    }

    // 4. Identify plateaus between steps (>= 80 ms = 5 frames at 60 Hz with |rate| < 50 %/s)
    let mut plateau_ranges: Vec<(usize, usize)> = Vec::new();
    let mut in_plateau = false;
    let mut plateau_start = 0;

    for (k, &r) in rates.iter().enumerate() {
        if r.abs() < 50.0 {
            if !in_plateau {
                in_plateau = true;
                plateau_start = k;
            }
        } else if in_plateau {
            in_plateau = false;
            if k - plateau_start >= MIN_PLATEAU_FRAMES {
                plateau_ranges.push((plateau_start, k));
            }
        }
    }
    if in_plateau && rates.len() - plateau_start >= MIN_PLATEAU_FRAMES {
        plateau_ranges.push((plateau_start, rates.len()));
    }

    let mut plateau_levels_pct = Vec::new();
    let mut last_plateau_end_frame = if fast_step_runs.is_empty() {
        onset
    } else {
        onset + fast_step_runs[0].1
    };

    for (p_start, p_end) in plateau_ranges {
        let has_fast_before = fast_step_runs.iter().any(|r| r.1 <= p_start);
        let has_fast_after = fast_step_runs.iter().any(|r| r.0 >= p_end);

        if has_fast_before && has_fast_after && p_end < num_frames {
            let th_at_end = f64::from(throttles[onset + p_end]) * 100.0;
            let th_full = f64::from(throttles[full]) * 100.0;
            if th_full > th_at_end + 5.0 {
                let sum: f64 = (p_start..=p_end)
                    .map(|idx| f64::from(throttles[onset + idx]) * 100.0)
                    .sum();
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "plateau frame count safely converts to f64"
                )]
                let avg = sum / (p_end - p_start + 1) as f64;
                plateau_levels_pct.push(avg);
                last_plateau_end_frame = onset + p_end;
            }
        }
    }

    let fast_steps = fast_step_runs.len();

    #[expect(
        clippy::cast_precision_loss,
        reason = "sample count safely converts to f64"
    )]
    let time_to_stab_s = if fast_step_runs.is_empty() {
        0.0
    } else {
        (fast_step_runs[0].1) as f64 / TELEMETRY_HZ
    };

    let ramp_start_frame = last_plateau_end_frame.min(full);
    let progressive_ramp_rate_pct_s = if full > ramp_start_frame {
        #[expect(
            clippy::cast_precision_loss,
            reason = "sample count safely converts to f64"
        )]
        let dt = (full - ramp_start_frame) as f64 / TELEMETRY_HZ;
        let dy = f64::from(throttles[full] - throttles[ramp_start_frame]) * 100.0;
        if dt > 1e-6 { dy / dt } else { 0.0 }
    } else if time_to_full_s > 1e-6 {
        let dy = f64::from(throttles[full] - throttles[onset]) * 100.0;
        dy / time_to_full_s
    } else {
        0.0
    };

    ThrottleExitMetrics {
        initial_stab_level_pct,
        time_to_stab_s,
        fast_steps,
        plateau_levels_pct,
        progressive_ramp_rate_pct_s,
        time_to_full_s,
    }
}

/// Raw metrics measured from a single braking zone.
#[derive(Clone, Debug, PartialEq)]
pub struct BrakeZoneMetrics {
    /// Maximum brake pressure reached in percent `[0.0, 100.0]`.
    pub peak_pct: f64,
    /// Time elapsed from onset to peak in seconds.
    pub time_to_peak_s: f64,
    /// Duration of the trail-off / release phase from peak to release in seconds.
    pub trail_duration_s: f64,
}

/// Analyzes an individual brake zone.
#[must_use]
pub fn analyze_brake_zone(_brakes: &[f32], zone: &BrakeZone) -> BrakeZoneMetrics {
    #[expect(
        clippy::cast_precision_loss,
        reason = "sample count safely converts to f64"
    )]
    let time_to_peak_s = (zone.peak_idx - zone.onset_idx) as f64 / TELEMETRY_HZ;
    #[expect(
        clippy::cast_precision_loss,
        reason = "sample count safely converts to f64"
    )]
    let trail_duration_s = (zone.release_idx - zone.peak_idx) as f64 / TELEMETRY_HZ;

    BrakeZoneMetrics {
        peak_pct: f64::from(zone.peak_pct),
        time_to_peak_s,
        trail_duration_s,
    }
}

/// Raw metrics measured from a single lift zone.
#[derive(Clone, Debug, PartialEq)]
pub struct LiftZoneMetrics {
    /// Minimum throttle percentage reached during the lift (lift depth) `[0.0, 100.0]`.
    pub min_pct: f64,
    /// Duration of the lift in seconds.
    pub duration_s: f64,
}

/// Analyzes an individual lift zone.
#[must_use]
pub fn analyze_lift_zone(zone: &crate::zones::LiftZone) -> LiftZoneMetrics {
    LiftZoneMetrics {
        min_pct: f64::from(zone.min_pct),
        duration_s: zone.duration_s,
    }
}

/// Summary statistics for brake zones across laps.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CarBrakeStats {
    pub zone_count: usize,
    pub peak_pct: MetricSummary,
    pub time_to_peak_s: MetricSummary,
    pub trail_duration_s: MetricSummary,
}

/// Summary statistics for lift zones across laps.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CarLiftStats {
    pub zone_count: usize,
    pub lift_depth_pct: MetricSummary,
    pub duration_s: MetricSummary,
}

/// Summary statistics for throttle exit zones across laps.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CarThrottleStats {
    pub zone_count: usize,
    pub initial_stab_level_pct: MetricSummary,
    pub time_to_stab_s: MetricSummary,
    pub fast_steps: MetricSummary,
    pub plateau_level_pct: MetricSummary,
    pub progressive_ramp_rate_pct_s: MetricSummary,
    pub time_to_full_s: MetricSummary,
}

/// Aggregated telemetry statistics for a car model.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CarStats {
    pub car: String,
    pub laps_count: usize,
    pub brake: CarBrakeStats,
    pub lift: CarLiftStats,
    pub throttle: CarThrottleStats,
}

/// High-level container for multi-car telemetry statistics.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TelemetryReport {
    pub cars: BTreeMap<String, CarStats>,
}

/// Collector to accumulate and aggregate metrics across laps.
#[derive(Default)]
pub struct StatsCollector {
    cars: BTreeMap<String, CarAccumulator>,
}

#[derive(Default)]
struct CarAccumulator {
    laps_count: usize,
    // Brake metrics
    brake_peaks: Vec<f64>,
    brake_times_to_peak: Vec<f64>,
    brake_trail_durations: Vec<f64>,
    // Lift metrics
    lift_depths: Vec<f64>,
    lift_durations: Vec<f64>,
    // Throttle metrics
    throttle_stabs: Vec<f64>,
    throttle_times_to_stab: Vec<f64>,
    throttle_fast_steps: Vec<f64>,
    throttle_plateau_levels: Vec<f64>,
    throttle_ramp_rates: Vec<f64>,
    throttle_times_to_full: Vec<f64>,
}

impl StatsCollector {
    /// Ingests a lap and its detected zones.
    pub fn add_lap(
        &mut self,
        lap: &LapTelemetry,
        brakes: &[BrakeZone],
        lifts: &[crate::zones::LiftZone],
        throttles: &[ThrottleExitZone],
    ) {
        let car_name = lap.car_name().to_string();
        let acc = self.cars.entry(car_name).or_default();
        acc.laps_count += 1;

        for b_zone in brakes {
            let m = analyze_brake_zone(&lap.brake, b_zone);
            acc.brake_peaks.push(m.peak_pct);
            acc.brake_times_to_peak.push(m.time_to_peak_s);
            acc.brake_trail_durations.push(m.trail_duration_s);
        }

        for l_zone in lifts {
            let m = analyze_lift_zone(l_zone);
            acc.lift_depths.push(m.min_pct);
            acc.lift_durations.push(m.duration_s);
        }

        for t_zone in throttles {
            let m = analyze_throttle_exit(&lap.throttle, t_zone);
            acc.throttle_stabs.push(m.initial_stab_level_pct);
            acc.throttle_times_to_stab.push(m.time_to_stab_s);
            #[expect(
                clippy::cast_precision_loss,
                reason = "small integer count safely converts to f64"
            )]
            acc.throttle_fast_steps.push(m.fast_steps as f64);
            acc.throttle_plateau_levels.extend(&m.plateau_levels_pct);
            acc.throttle_ramp_rates.push(m.progressive_ramp_rate_pct_s);
            acc.throttle_times_to_full.push(m.time_to_full_s);
        }
    }

    /// Builds the consolidated report.
    #[must_use]
    pub fn build_report(self) -> TelemetryReport {
        let mut report_cars = BTreeMap::new();

        for (car, acc) in self.cars {
            let brake_count = acc.brake_peaks.len();
            let lift_count = acc.lift_depths.len();
            let throttle_count = acc.throttle_stabs.len();

            let brake = CarBrakeStats {
                zone_count: brake_count,
                peak_pct: MetricSummary::compute(&acc.brake_peaks),
                time_to_peak_s: MetricSummary::compute(&acc.brake_times_to_peak),
                trail_duration_s: MetricSummary::compute(&acc.brake_trail_durations),
            };

            let lift = CarLiftStats {
                zone_count: lift_count,
                lift_depth_pct: MetricSummary::compute(&acc.lift_depths),
                duration_s: MetricSummary::compute(&acc.lift_durations),
            };

            let throttle = CarThrottleStats {
                zone_count: throttle_count,
                initial_stab_level_pct: MetricSummary::compute(&acc.throttle_stabs),
                time_to_stab_s: MetricSummary::compute(&acc.throttle_times_to_stab),
                fast_steps: MetricSummary::compute(&acc.throttle_fast_steps),
                plateau_level_pct: MetricSummary::compute(&acc.throttle_plateau_levels),
                progressive_ramp_rate_pct_s: MetricSummary::compute(&acc.throttle_ramp_rates),
                time_to_full_s: MetricSummary::compute(&acc.throttle_times_to_full),
            };

            report_cars.insert(
                car.clone(),
                CarStats {
                    car,
                    laps_count: acc.laps_count,
                    brake,
                    lift,
                    throttle,
                },
            );
        }

        TelemetryReport { cars: report_cars }
    }
}

/// Renders the statistics report as a formatted table string.
#[must_use]
pub fn format_stats_table(report: &TelemetryReport) -> String {
    use std::fmt::Write;

    if report.cars.is_empty() {
        return "No telemetry data found matching criteria.\n".to_string();
    }

    let mut out = String::new();

    let _ = writeln!(
        out,
        "========================================================================================================"
    );
    let _ = writeln!(
        out,
        "                                 TELEMETRY ZONE STATISTICS REPORT (MEDIAN / IQR)                         "
    );
    let _ = writeln!(
        out,
        "========================================================================================================\n"
    );

    for (car_name, stats) in &report.cars {
        let _ = writeln!(
            out,
            "CAR: {car_name} ({laps} lap(s), {bz} brake zone(s), {lz} lift zone(s), {tz} throttle exit(s))",
            laps = stats.laps_count,
            bz = stats.brake.zone_count,
            lz = stats.lift.zone_count,
            tz = stats.throttle.zone_count
        );
        let _ = writeln!(
            out,
            "--------------------------------------------------------------------------------------------------------"
        );
        let _ = writeln!(out, "  BRAKE ZONES:");
        let _ = writeln!(
            out,
            "    Peak Pressure:            {:>6.1}%  (IQR: {:>5.1}%)",
            stats.brake.peak_pct.median, stats.brake.peak_pct.iqr
        );
        let _ = writeln!(
            out,
            "    Time to Peak:             {:>6.3}s  (IQR: {:>5.3}s)",
            stats.brake.time_to_peak_s.median, stats.brake.time_to_peak_s.iqr
        );
        let _ = writeln!(
            out,
            "    Trail/Release Duration:   {:>6.3}s  (IQR: {:>5.3}s)",
            stats.brake.trail_duration_s.median, stats.brake.trail_duration_s.iqr
        );

        if stats.lift.zone_count > 0 {
            let _ = writeln!(out, "  LIFT ZONES:");
            let _ = writeln!(
                out,
                "    Lift Depth (Min Throttle):{:>6.1}%  (IQR: {:>5.1}%)",
                stats.lift.lift_depth_pct.median, stats.lift.lift_depth_pct.iqr
            );
            let _ = writeln!(
                out,
                "    Duration:                 {:>6.3}s  (IQR: {:>5.3}s)",
                stats.lift.duration_s.median, stats.lift.duration_s.iqr
            );
        }

        let _ = writeln!(out, "  THROTTLE EXITS:");
        let _ = writeln!(
            out,
            "    Initial Stab Level (~150ms): {:>5.1}%  (IQR: {:>5.1}%)",
            stats.throttle.initial_stab_level_pct.median, stats.throttle.initial_stab_level_pct.iqr
        );
        let _ = writeln!(
            out,
            "    Time to Stab:                {:>5.3}s  (IQR: {:>5.3}s)",
            stats.throttle.time_to_stab_s.median, stats.throttle.time_to_stab_s.iqr
        );
        let _ = writeln!(
            out,
            "    Steps per Zone:              {:>5.1}   (IQR: {:>5.1})",
            stats.throttle.fast_steps.median, stats.throttle.fast_steps.iqr
        );
        if stats.throttle.plateau_level_pct.median > 0.0 {
            let _ = writeln!(
                out,
                "    Plateau Level:               {:>5.1}%  (IQR: {:>5.1}%)",
                stats.throttle.plateau_level_pct.median, stats.throttle.plateau_level_pct.iqr
            );
        }
        let _ = writeln!(
            out,
            "    Progressive Ramp Rate:       {:>5.1}%/s (IQR: {:>5.1}%/s)",
            stats.throttle.progressive_ramp_rate_pct_s.median,
            stats.throttle.progressive_ramp_rate_pct_s.iqr
        );
        let _ = writeln!(
            out,
            "    Time to Full (98%):          {:>5.3}s  (IQR: {:>5.3}s)",
            stats.throttle.time_to_full_s.median, stats.throttle.time_to_full_s.iqr
        );
        let _ = writeln!(
            out,
            "--------------------------------------------------------------------------------------------------------\n"
        );
    }

    out
}
