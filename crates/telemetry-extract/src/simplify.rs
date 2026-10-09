//! Curve resampling, RDP simplification, and drill generation.

use crate::csv::TELEMETRY_HZ;

/// Number of samples prepended for lead-in padding (300 ms at 60 Hz).
pub const LEAD_IN_SAMPLES: usize = 18;

/// Number of samples appended for lead-out padding (300 ms at 60 Hz).
pub const LEAD_OUT_SAMPLES: usize = 18;

/// Target resampling interval in milliseconds (10 ms = 100 Hz).
pub const RESAMPLE_STEP_MS: f64 = 10.0;

/// Minimum target vertex count for RDP simplification.
pub const MIN_SIMPLIFIED_POINTS: usize = 5;

/// Maximum target vertex count for RDP simplification.
pub const MAX_SIMPLIFIED_POINTS: usize = 15;

/// Detected plateau within a pedal curve suitable for a hold drill.
#[derive(Clone, Debug, PartialEq)]
pub struct Plateau {
    /// Target pedal percentage `[0.0, 100.0]`.
    pub target: f32,
    /// Duration of the plateau in milliseconds (`200..=60000`).
    pub hold_ms: u32,
}

/// Point on a 2D curve.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point2D {
    pub x: f64,
    pub y: f64,
}

/// Computes the perpendicular Euclidean distance from point `p` to chord `[a, b]`.
#[must_use]
pub fn perpendicular_distance(p: Point2D, a: Point2D, b: Point2D) -> f64 {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    let length_sq = dx * dx + dy * dy;

    if length_sq < 1e-12 {
        let px = p.x - a.x;
        let py = p.y - a.y;
        return (px * px + py * py).sqrt();
    }

    let numerator = (dy * p.x - dx * p.y + b.x * a.y - b.y * a.x).abs();
    numerator / length_sq.sqrt()
}

/// Recursively simplifies a 2D polyline using the Ramer-Douglas-Peucker (RDP) algorithm.
#[must_use]
pub fn rdp(points: &[Point2D], epsilon: f64) -> Vec<Point2D> {
    if points.len() <= 2 {
        return points.to_vec();
    }

    let first = points[0];
    let last = points[points.len() - 1];

    let mut max_dist = 0.0;
    let mut max_idx = 0;

    for (i, &p) in points.iter().enumerate().take(points.len() - 1).skip(1) {
        let dist = perpendicular_distance(p, first, last);
        if dist > max_dist {
            max_dist = dist;
            max_idx = i;
        }
    }

    if max_dist > epsilon {
        let left = rdp(&points[..=max_idx], epsilon);
        let right = rdp(&points[max_idx..], epsilon);

        let mut result = left;
        result.pop(); // remove duplicate shared pivot point
        result.extend(right);
        result
    } else {
        vec![first, last]
    }
}

/// Simplifies a normalized `[0, 1] x [0, 1]` curve using adaptive bisection search
/// over `epsilon` to produce between `MIN_SIMPLIFIED_POINTS` and `MAX_SIMPLIFIED_POINTS` vertices.
#[must_use]
pub fn simplify_adaptive(normalized_pts: &[Point2D]) -> Vec<Point2D> {
    if normalized_pts.len() <= MIN_SIMPLIFIED_POINTS {
        return normalized_pts.to_vec();
    }

    let mut eps_low = 0.0001;
    let mut eps_high = 0.50;
    let mut best = normalized_pts.to_vec();

    for _ in 0..20 {
        let eps = f64::midpoint(eps_low, eps_high);
        let simplified = rdp(normalized_pts, eps);
        let n = simplified.len();

        if (MIN_SIMPLIFIED_POINTS..=MAX_SIMPLIFIED_POINTS).contains(&n) {
            return simplified;
        }

        if n > MAX_SIMPLIFIED_POINTS {
            // Too many points -> increase tolerance to simplify more
            eps_low = eps;
            best = simplified;
        } else {
            // Too few points -> decrease tolerance to keep more points
            eps_high = eps;
            best = simplified;
        }
    }

    best
}

/// Evaluates monotonic PCHIP (Piecewise Cubic Hermite) interpolation over raw samples.
#[must_use]
pub fn pchip_resample(raw_samples: &[f32], total_duration_ms: f64) -> Vec<(f64, f64)> {
    let n = raw_samples.len();
    if n == 0 {
        return Vec::new();
    }
    if n == 1 {
        return vec![(0.0, f64::from(raw_samples[0]))];
    }

    #[expect(
        clippy::cast_precision_loss,
        reason = "sample count safely converts to f64"
    )]
    let dt_sample = (total_duration_ms / (n - 1) as f64).max(1e-6);

    // Compute finite-difference slopes
    let mut deltas = Vec::with_capacity(n - 1);
    for window in raw_samples.windows(2) {
        let dy = f64::from(window[1] - window[0]);
        deltas.push(dy / dt_sample);
    }

    // Compute tangents using standard Fritsch-Carlson monotonic harmonic mean
    let mut tangents = vec![0.0; n];
    tangents[0] = deltas[0];
    tangents[n - 1] = deltas[n - 2];

    for i in 1..n - 1 {
        let d0 = deltas[i - 1];
        let d1 = deltas[i];
        if (d0 > 0.0 && d1 > 0.0) || (d0 < 0.0 && d1 < 0.0) {
            tangents[i] = (2.0 * d0 * d1) / (d0 + d1);
        } else {
            tangents[i] = 0.0;
        }
    }

    // Resample on uniform RESAMPLE_STEP_MS grid
    let step = RESAMPLE_STEP_MS;
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "number of 10ms steps for <60s trace fits in usize"
    )]
    let num_steps = (total_duration_ms / step).ceil() as usize + 1;
    let mut resampled = Vec::with_capacity(num_steps);

    for s in 0..num_steps {
        #[expect(
            clippy::cast_precision_loss,
            reason = "step index safely converts to f64"
        )]
        let t = (s as f64 * step).min(total_duration_ms);
        let sample_pos = t / dt_sample;
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "sample index within bounds fits in usize"
        )]
        let idx = (sample_pos.floor() as usize).min(n - 2);

        #[expect(
            clippy::cast_precision_loss,
            reason = "sample index safely converts to f64"
        )]
        let t0 = idx as f64 * dt_sample;
        let h = dt_sample;
        let u = (t - t0) / h;

        let y0 = f64::from(raw_samples[idx]);
        let y1 = f64::from(raw_samples[idx + 1]);
        let m0 = tangents[idx];
        let m1 = tangents[idx + 1];

        // Hermite basis functions
        let h00 = 2.0 * u * u * u - 3.0 * u * u + 1.0;
        let h10 = u * u * u - 2.0 * u * u + u;
        let h01 = -2.0 * u * u * u + 3.0 * u * u;
        let h11 = u * u * u - u * u;

        let val = (h00 * y0 + h10 * h * m0 + h01 * y1 + h11 * h * m1).clamp(0.0, 1.0);
        resampled.push((t, val));

        if (t - total_duration_ms).abs() < 1e-6 {
            break;
        }
    }

    resampled
}

/// Prepares and simplifies a pedal segment into drill trace points `[(t_ms, target_percent)]`.
///
/// Anchors `t=0` at the lead-in start, resamples with monotonic PCHIP, normalizes coordinates,
/// runs RDP simplification to 5–15 points, and verifies strict monotonicity of timestamps.
#[must_use]
pub fn process_trace_segment(
    samples: &[f32],
    onset_idx: usize,
    release_idx: usize,
) -> Vec<(u32, f32)> {
    if samples.is_empty() || onset_idx > release_idx {
        return Vec::new();
    }

    let start_idx = onset_idx.saturating_sub(LEAD_IN_SAMPLES);
    let end_idx = (release_idx + LEAD_OUT_SAMPLES).min(samples.len() - 1);
    let slice = &samples[start_idx..=end_idx];

    #[expect(
        clippy::cast_precision_loss,
        reason = "sample count safely converts to f64"
    )]
    let total_duration_ms = ((end_idx - start_idx) as f64 / TELEMETRY_HZ) * 1000.0;

    // 1. Monotonic PCHIP resample onto uniform grid
    let resampled = pchip_resample(slice, total_duration_ms);
    if resampled.len() < 2 {
        return Vec::new();
    }

    // 2. Normalize onto dimensionless unit square [0, 1] x [0, 1]
    let normalized: Vec<Point2D> = resampled
        .iter()
        .map(|&(t, v)| Point2D {
            x: if total_duration_ms > 0.0 {
                t / total_duration_ms
            } else {
                0.0
            },
            y: v,
        })
        .collect();

    // 3. Adaptive RDP simplification to 5..=15 points
    let simplified = simplify_adaptive(&normalized);

    // 4. Map back to milliseconds and percentage, enforcing preset schema rules
    let mut points: Vec<(u32, f32)> = Vec::with_capacity(simplified.len());
    let mut last_t = 0;

    for (i, pt) in simplified.iter().enumerate() {
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "un-normalized millisecond fits in u32"
        )]
        let mut t_ms = (pt.x * total_duration_ms).round() as u32;

        if i == 0 {
            t_ms = 0;
        } else if t_ms <= last_t {
            t_ms = last_t + 1;
        }
        last_t = t_ms;

        #[expect(
            clippy::cast_possible_truncation,
            reason = "percentage clamped to 0..100 fits in f32"
        )]
        let val_pct = ((pt.y * 1000.0).round() / 10.0).clamp(0.0, 100.0) as f32;

        points.push((t_ms, val_pct));
    }

    // Ensure at least 2 points and duration <= 60000 ms
    if points.len() == 1 {
        points.push((last_t + 10, points[0].1));
    }

    points
}

/// Detects a sustained plateau within a segment of pedal travel.
///
/// A plateau is defined as a contiguous region lasting $\ge 500$ ms where
/// the pedal travel variation is within 6 percentage points and average travel is $\ge 20\%$.
#[must_use]
pub fn detect_plateau(samples: &[f32], onset_idx: usize, release_idx: usize) -> Option<Plateau> {
    const MIN_FRAMES: usize = 30; // 500 ms at 60 Hz

    if onset_idx >= release_idx || release_idx >= samples.len() {
        return None;
    }
    let slice = &samples[onset_idx..=release_idx];

    if slice.len() < MIN_FRAMES {
        return None;
    }

    let mut best_start = 0;
    let mut best_len = 0;
    let mut best_mean = 0.0;

    let mut left = 0;
    let mut current_sum = 0.0;
    let mut min_dq = std::collections::VecDeque::<usize>::new();
    let mut max_dq = std::collections::VecDeque::<usize>::new();

    for right in 0..slice.len() {
        let val = slice[right];
        current_sum += val;

        while let Some(&idx) = min_dq.back() {
            if slice[idx] >= val {
                min_dq.pop_back();
            } else {
                break;
            }
        }
        min_dq.push_back(right);

        while let Some(&idx) = max_dq.back() {
            if slice[idx] <= val {
                max_dq.pop_back();
            } else {
                break;
            }
        }
        max_dq.push_back(right);

        while left <= right {
            let cmin = slice[*min_dq.front().unwrap()];
            let cmax = slice[*max_dq.front().unwrap()];
            if cmax - cmin <= 0.06 {
                break;
            }
            current_sum -= slice[left];
            if min_dq.front() == Some(&left) {
                min_dq.pop_front();
            }
            if max_dq.front() == Some(&left) {
                max_dq.pop_front();
            }
            left += 1;
        }

        let mut cur_left = left;
        let mut cur_sum = current_sum;
        let mut len = right - cur_left + 1;

        if slice[*max_dq.front().unwrap()] >= 0.20 {
            while len >= MIN_FRAMES && len > best_len {
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "window frame count under 1000 fits in f32"
                )]
                let len_f32 = len as f32;
                let mean = cur_sum / len_f32;
                if mean >= 0.20 {
                    best_len = len;
                    best_start = cur_left;
                    best_mean = mean;
                    break;
                }
                cur_sum -= slice[cur_left];
                cur_left += 1;
                len -= 1;
            }
        }
    }

    if best_len >= MIN_FRAMES {
        #[expect(
            clippy::cast_precision_loss,
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "hold duration under 60000ms fits in u32"
        )]
        let hold_ms = ((best_len as f64 / TELEMETRY_HZ) * 1000.0).round() as u32;
        let hold_ms = hold_ms.clamp(200, 60000);
        let target = ((best_mean * 1000.0).round() / 10.0).clamp(0.0, 100.0);

        let _ = best_start;
        Some(Plateau { target, hold_ms })
    } else {
        None
    }
}
