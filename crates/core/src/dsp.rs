//! Digital signal processing helpers for filtering and numerical differentiation.
//!
//! Provides zero-phase Butterworth low-pass filtering via bidirectional cascading
//! biquads (`filtfilt`) and central difference numerical derivatives.

use std::f64::consts::PI;

/// Quality factor Q for the first 2nd-order section of a 4th-order Butterworth filter.
const BUTTERWORTH_Q1: f64 = 0.541_196_1;

/// Quality factor Q for the second 2nd-order section of a 4th-order Butterworth filter.
const BUTTERWORTH_Q2: f64 = 1.306_563;

/// Filter order of the low-pass Butterworth filter.
const FILTER_ORDER: usize = 4;

/// Multiplier for the extension length calculation (3 × filter order × 10 = 120 samples).
const EXTENSION_MULTIPLIER: usize = 3 * FILTER_ORDER * 10;

/// Biquad Direct Form II Transposed filter coefficients for a 2nd-order section.
#[derive(Debug, Clone, Copy)]
struct BiquadCoeffs {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
}

impl BiquadCoeffs {
    /// Computes low-pass biquad coefficients using bilinear transform with pre-warping.
    fn lowpass(cutoff_hz: f64, sample_rate_hz: f64, q: f64) -> Self {
        let k = (PI * cutoff_hz / sample_rate_hz).tan();
        let k2 = k * k;
        let norm = 1.0 + (k / q) + k2;

        let b0 = k2 / norm;
        let b1 = 2.0 * b0;
        let b2 = b0;
        let a1 = 2.0 * (k2 - 1.0) / norm;
        let a2 = (1.0 - (k / q) + k2) / norm;

        Self { b0, b1, b2, a1, a2 }
    }

    /// Filters a slice forward using Direct Form II Transposed.
    fn filter(&self, x: &[f64]) -> Vec<f64> {
        let mut y = Vec::with_capacity(x.len());
        let mut s1 = 0.0;
        let mut s2 = 0.0;

        for &sample in x {
            let out = self.b0 * sample + s1;
            s1 = self.b1 * sample - self.a1 * out + s2;
            s2 = self.b2 * sample - self.a2 * out;
            y.push(out);
        }

        y
    }
}

/// Applies a zero-phase 4th-order Butterworth low-pass filter using forward-backward filtering.
///
/// The 4th-order filter is implemented as two cascaded biquads with Q = 0.5411961 and Q = 1.306563,
/// transformed via bilinear transform. Edge transients are suppressed using odd extension
/// by `min(len - 1, 3 * 4 * 10 = 120)` samples at both ends before filtering.
///
/// Returns an empty vector if `x` is empty, or a clone of `x` if `x` has a single sample.
#[must_use]
pub fn butterworth_lowpass_filtfilt(x: &[f64], cutoff_hz: f64, sample_rate_hz: f64) -> Vec<f64> {
    let n = x.len();
    if n == 0 {
        return Vec::new();
    }
    if n == 1 || cutoff_hz <= 0.0 || sample_rate_hz <= 0.0 || cutoff_hz >= sample_rate_hz / 2.0 {
        return x.to_vec();
    }

    let biquad1 = BiquadCoeffs::lowpass(cutoff_hz, sample_rate_hz, BUTTERWORTH_Q1);
    let biquad2 = BiquadCoeffs::lowpass(cutoff_hz, sample_rate_hz, BUTTERWORTH_Q2);

    let pad_len = (n - 1).min(EXTENSION_MULTIPLIER);
    let mut padded = Vec::with_capacity(n + 2 * pad_len);

    // Left odd reflection: 2 * x[0] - x[k], for k from pad_len down to 1
    let x0 = x[0];
    for k in (1..=pad_len).rev() {
        padded.push(2.0 * x0 - x[k]);
    }

    padded.extend_from_slice(x);

    // Right odd reflection: 2 * x[n-1] - x[n - 1 - k], for k from 1 to pad_len
    let x_last = x[n - 1];
    for k in 1..=pad_len {
        padded.push(2.0 * x_last - x[n - 1 - k]);
    }

    // Forward pass through biquad 1 then biquad 2
    let fwd1 = biquad1.filter(&padded);
    let mut fwd2 = biquad2.filter(&fwd1);

    // Backward pass
    fwd2.reverse();
    let bwd1 = biquad1.filter(&fwd2);
    let mut bwd2 = biquad2.filter(&bwd1);
    bwd2.reverse();

    // Trim padding
    bwd2[pad_len..pad_len + n].to_vec()
}

/// Computes the numerical derivative using central differences with one-sided differences at ends.
///
/// For interior points `i`, computes `(x[i+1] - x[i-1]) / (2 * dt)`.
/// At the ends, computes `(x[1] - x[0]) / dt` and `(x[n-1] - x[n-2]) / dt`.
#[must_use]
pub fn central_difference(x: &[f64], dt: f64) -> Vec<f64> {
    let n = x.len();
    if n == 0 {
        return Vec::new();
    }
    if n == 1 || dt <= 0.0 {
        return vec![0.0];
    }
    if n == 2 {
        let diff = (x[1] - x[0]) / dt;
        return vec![diff, diff];
    }

    let mut out = Vec::with_capacity(n);
    out.push((x[1] - x[0]) / dt);

    let two_dt = 2.0 * dt;
    for i in 1..n - 1 {
        out.push((x[i + 1] - x[i - 1]) / two_dt);
    }

    out.push((x[n - 1] - x[n - 2]) / dt);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dc_passes_with_gain_one_after_edges_settle() {
        let sample_rate = 1000.0;
        let cutoff = 12.0;
        let len = 2000;
        let dc_level = 0.75;
        let x = vec![dc_level; len];

        let filtered = butterworth_lowpass_filtfilt(&x, cutoff, sample_rate);
        assert_eq!(filtered.len(), len);

        // After edges settle (middle region well past edge transients)
        for &val in &filtered[500..len - 500] {
            assert!(
                (val - dc_level).abs() <= 1e-6,
                "expected {dc_level}, got {val}"
            );
        }
    }

    #[test]
    fn sine_2hz_passes_with_high_amplitude_and_no_phase_shift() {
        let sample_rate = 1000.0;
        let cutoff = 12.0;
        let len = 2000;
        let freq = 2.0;

        #[expect(clippy::cast_precision_loss, reason = "sample index i fits within f64")]
        let x: Vec<f64> = (0..len)
            .map(|i| (2.0 * PI * freq * (i as f64) / sample_rate).sin())
            .collect();

        let filtered = butterworth_lowpass_filtfilt(&x, cutoff, sample_rate);

        // Amplitude > 0.97 in the interior
        let max_amp = filtered[200..len - 200]
            .iter()
            .copied()
            .fold(0.0_f64, f64::max);
        assert!(max_amp > 0.97, "expected amplitude > 0.97, got {max_amp}");

        // Zero crossings within ±2 samples (no phase shift)
        // For a 2 Hz sine at 1000 Hz, zero crossings occur every 250 samples: 250, 500, 750, 1000...
        for &expected_zero_idx in &[500, 750, 1000, 1250, 1500] {
            // Find sign change near expected_zero_idx
            let mut detected_zero = None;
            for i in (expected_zero_idx - 10)..=(expected_zero_idx + 10) {
                if filtered[i] * filtered[i + 1] <= 0.0 {
                    detected_zero = Some(i);
                    break;
                }
            }

            let zero_idx = detected_zero.expect("zero crossing should exist");
            let diff = (i64::try_from(zero_idx).unwrap()
                - i64::try_from(expected_zero_idx).unwrap())
            .abs();
            assert!(
                diff <= 2,
                "zero crossing at {zero_idx} not within 2 samples of {expected_zero_idx}"
            );
        }
    }

    #[test]
    fn sine_50hz_attenuated_below_two_percent() {
        let sample_rate = 1000.0;
        let cutoff = 12.0;
        let len = 1000;
        let freq = 50.0;

        #[expect(clippy::cast_precision_loss, reason = "sample index i fits within f64")]
        let x: Vec<f64> = (0..len)
            .map(|i| (2.0 * PI * freq * (i as f64) / sample_rate).sin())
            .collect();

        let filtered = butterworth_lowpass_filtfilt(&x, cutoff, sample_rate);

        let max_amp = filtered[150..len - 150]
            .iter()
            .copied()
            .fold(0.0_f64, f64::max);
        assert!(max_amp < 0.02, "expected amplitude < 0.02, got {max_amp}");
    }

    #[test]
    fn ramp_derivative_is_constant() {
        let len = 100;
        let slope = 3.5;
        let dt = 0.01;

        #[expect(clippy::cast_precision_loss, reason = "sample index i fits within f64")]
        let x: Vec<f64> = (0..len).map(|i| slope * (i as f64) * dt).collect();

        let dx = central_difference(&x, dt);
        assert_eq!(dx.len(), len);

        for &val in &dx {
            assert!(
                (val - slope).abs() < 1e-10,
                "expected constant {slope}, got {val}"
            );
        }
    }

    #[test]
    fn edge_cases_empty_and_single() {
        assert_eq!(
            butterworth_lowpass_filtfilt(&[], 12.0, 1000.0),
            Vec::<f64>::new()
        );
        assert_eq!(
            butterworth_lowpass_filtfilt(&[42.0], 12.0, 1000.0),
            vec![42.0]
        );
        assert_eq!(central_difference(&[], 0.01), Vec::<f64>::new());
        assert_eq!(central_difference(&[5.0], 0.01), vec![0.0]);
    }
}
