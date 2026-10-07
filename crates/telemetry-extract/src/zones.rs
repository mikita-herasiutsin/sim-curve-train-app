//! Detection of braking zones and corner exit throttle zones.

use crate::csv::TELEMETRY_HZ;

/// Hysteresis onset threshold for braking (5% pedal travel).
pub const BRAKE_ONSET_THRESHOLD: f32 = 0.05;

/// Hysteresis release threshold for braking (2% pedal travel).
pub const BRAKE_RELEASE_THRESHOLD: f32 = 0.02;

/// Minimum duration in seconds for a valid braking zone (100 ms).
pub const BRAKE_MIN_DURATION_S: f64 = 0.10;

/// Minimum peak pedal percentage for a valid braking zone (20%).
pub const BRAKE_MIN_PEAK: f32 = 0.20;

/// Maximum gap between braking pulses in samples (~250 ms at 60 Hz) to merge.
pub const BRAKE_MERGE_GAP_SAMPLES: usize = 15;

/// Threshold considered "full throttle" (98%).
pub const THROTTLE_FULL_THRESHOLD: f32 = 0.98;

/// Minimum consecutive frames at or above 98% throttle to consider it sustained.
pub const THROTTLE_SUSTAINED_FRAMES: usize = 5;

/// Minimum duration in seconds for a corner exit throttle zone (150 ms).
pub const THROTTLE_MIN_DURATION_S: f64 = 0.15;

/// Detected braking event.
#[derive(Clone, Debug, PartialEq)]
pub struct BrakeZone {
    /// Frame index where braking begins (crosses `BRAKE_ONSET_THRESHOLD`).
    pub onset_idx: usize,
    /// Frame index where braking ends (drops below `BRAKE_RELEASE_THRESHOLD`).
    pub release_idx: usize,
    /// Frame index where peak braking occurred.
    pub peak_idx: usize,
    /// Peak brake value in percent `[0.0, 100.0]`.
    pub peak_pct: f32,
    /// Zone duration in seconds from onset to release.
    pub duration_s: f64,
}

/// Detected throttle exit acceleration event following a braking zone.
#[derive(Clone, Debug, PartialEq)]
pub struct ThrottleExitZone {
    /// Frame index of the throttle minimum prior to application.
    pub min_idx: usize,
    /// Frame index where throttle application begins towards corner exit.
    pub onset_idx: usize,
    /// Frame index where throttle first reaches $\ge 98\%$ and stays sustained.
    pub full_idx: usize,
    /// Duration in seconds from onset to full throttle.
    pub duration_s: f64,
}

/// Detects all valid braking zones in a telemetry trace using dual-threshold hysteresis,
/// transient filtering, and gap merging.
#[must_use]
pub fn detect_brake_zones(brakes: &[f32]) -> Vec<BrakeZone> {
    if brakes.is_empty() {
        return Vec::new();
    }

    // 1. Raw hysteresis state machine: collect candidate [start, end] intervals.
    let mut raw_intervals: Vec<(usize, usize)> = Vec::new();
    let mut in_brake = false;
    let mut onset = 0;

    for (i, &b) in brakes.iter().enumerate() {
        if !in_brake {
            if b >= BRAKE_ONSET_THRESHOLD {
                in_brake = true;
                onset = i;
            }
        } else if b < BRAKE_RELEASE_THRESHOLD {
            in_brake = false;
            raw_intervals.push((onset, i));
        }
    }

    if in_brake {
        raw_intervals.push((onset, brakes.len().saturating_sub(1)));
    }

    if raw_intervals.is_empty() {
        return Vec::new();
    }

    // 2. Merge adjacent intervals separated by less than BRAKE_MERGE_GAP_SAMPLES.
    let mut merged_intervals: Vec<(usize, usize)> = Vec::new();
    let mut current = raw_intervals[0];

    for &next in &raw_intervals[1..] {
        if next.0.saturating_sub(current.1) < BRAKE_MERGE_GAP_SAMPLES {
            current.1 = next.1;
        } else {
            merged_intervals.push(current);
            current = next;
        }
    }
    merged_intervals.push(current);

    // 3. Filter by duration and peak, and construct BrakeZone.
    let mut zones = Vec::new();
    for (start, end) in merged_intervals {
        if end <= start {
            continue;
        }

        #[expect(
            clippy::cast_precision_loss,
            reason = "sample count safely converts to f64"
        )]
        let duration_s = (end - start) as f64 / TELEMETRY_HZ;
        if duration_s < BRAKE_MIN_DURATION_S {
            continue;
        }

        // Find peak within [start, end]
        let mut peak_idx = start;
        let mut peak_val = brakes[start];
        for (i, &val) in brakes.iter().enumerate().take(end + 1).skip(start) {
            if val > peak_val {
                peak_val = val;
                peak_idx = i;
            }
        }

        if peak_val < BRAKE_MIN_PEAK {
            continue;
        }

        zones.push(BrakeZone {
            onset_idx: start,
            release_idx: end,
            peak_idx,
            peak_pct: peak_val * 100.0,
            duration_s,
        });
    }

    zones
}

/// Detects throttle exit zones associated with each braking zone.
///
/// A corner exit throttle zone starts from the throttle minimum after or near
/// the braking zone, begins rising, and terminates when throttle reaches
/// and stays at $\ge 98\%$.
#[must_use]
pub fn detect_throttle_exit_zones(
    throttles: &[f32],
    brake_zones: &[BrakeZone],
) -> Vec<ThrottleExitZone> {
    if throttles.is_empty() || brake_zones.is_empty() {
        return Vec::new();
    }

    let mut exit_zones = Vec::new();

    for (k, brake_zone) in brake_zones.iter().enumerate() {
        // Search window: from brake peak up to the next brake zone onset or end of data
        let window_start = brake_zone.peak_idx;
        let window_end = if k + 1 < brake_zones.len() {
            brake_zones[k + 1].onset_idx
        } else {
            throttles.len()
        };

        if window_end <= window_start + 10 {
            continue;
        }

        // 1. Find throttle minimum in the window before any subsequent full throttle
        let mut min_idx = window_start;
        let mut min_val = throttles[window_start];

        // Search through the release phase for the true corner apex minimum
        let search_limit = (brake_zone.release_idx + 120).min(window_end);
        for (i, &t) in throttles
            .iter()
            .enumerate()
            .take(search_limit)
            .skip(window_start)
        {
            if t <= min_val {
                min_val = t;
                min_idx = i;
            }
        }

        // 2. Find onset where throttle begins rising from minimum
        let onset_threshold = (min_val + 0.02).max(0.05);
        let mut onset_idx = None;
        for (i, &t) in throttles.iter().enumerate().take(window_end).skip(min_idx) {
            if t >= onset_threshold {
                onset_idx = Some(i);
                break;
            }
        }

        let Some(onset) = onset_idx else {
            continue;
        };

        // 3. Find where throttle reaches >= 98% and stays sustained
        let mut full_idx = None;
        for (i, &t) in throttles.iter().enumerate().take(window_end).skip(onset) {
            if t >= THROTTLE_FULL_THRESHOLD {
                // Check if it stays sustained or reaches the end of the window
                let sustained_end = (i + THROTTLE_SUSTAINED_FRAMES).min(window_end);
                let is_sustained = throttles[i..sustained_end].iter().all(|&val| val >= 0.95);
                if is_sustained {
                    full_idx = Some(i);
                    break;
                }
            }
        }

        let Some(full) = full_idx else {
            continue;
        };

        if full <= onset {
            continue;
        }

        #[expect(
            clippy::cast_precision_loss,
            reason = "sample count safely converts to f64"
        )]
        let duration_s = (full - onset) as f64 / TELEMETRY_HZ;
        if duration_s < THROTTLE_MIN_DURATION_S {
            continue;
        }

        exit_zones.push(ThrottleExitZone {
            min_idx,
            onset_idx: onset,
            full_idx: full,
            duration_s,
        });
    }

    exit_zones
}
