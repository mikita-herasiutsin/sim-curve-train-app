use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{BufferSize, Stream, StreamConfig};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

/// The error beep: a sine plus a weaker third harmonic, like a parking sensor.
const BEEP_FREQ_HZ: f32 = 784.0;
/// Level of the third harmonic relative to the fundamental.
const BEEP_HARMONIC_GAIN: f32 = 0.2;
/// Peak gain of a beep before the master volume.
const ERROR_GAIN: f32 = 0.15;
const BEEP_ATTACK_TIME_S: f32 = 0.004;
/// Longest flat part of a beep.
const BEEP_HOLD_MAX_S: f32 = 0.032;
/// The beep (attack, hold) never takes more than this share of the interval.
const BEEP_ACTIVE_SHARE: f32 = 0.45;
/// Reserved for the attack when capping the hold.
const BEEP_HOLD_MARGIN_S: f32 = 0.008;
/// Time constant of the exponential release.
const BEEP_RELEASE_TAU_S: f32 = 0.004;

const DENORMAL_THRESHOLD: f32 = 1e-6;
const SLEW_TIME_S: f32 = 0.0025;
/// The debug demo steps the rate from 3 to 11 Hz in 30 steps of 50 ms (1.5 s).
const TEST_DEMO_STEP: Duration = Duration::from_millis(50);
const TEST_DEMO_STEPS: u32 = 30;
const TEST_DEMO_MIN_HZ: f32 = 3.0;
const TEST_DEMO_MAX_HZ: f32 = 11.0;
pub const DEFAULT_AUDIO_VOLUME: f32 = 0.2;

/// Contains the lock-free shared state read by the audio callback.
pub struct SharedState {
    pub enabled: AtomicBool,
    pub master_volume: AtomicU32, // f32 bits
    /// Beeps per second, `0.0` for silence (f32 bits).
    pub pulse_rate: AtomicU32,
}

impl Default for SharedState {
    fn default() -> Self {
        Self::new()
    }
}

impl SharedState {
    #[must_use]
    pub fn new() -> Self {
        Self {
            enabled: AtomicBool::new(true),
            master_volume: AtomicU32::new(DEFAULT_AUDIO_VOLUME.to_bits()),
            pulse_rate: AtomicU32::new(0.0_f32.to_bits()),
        }
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Relaxed);
    }

    pub fn set_volume(&self, volume: f32) {
        let vol = if volume.is_finite() {
            volume.clamp(0.0, 1.0)
        } else {
            0.0
        };
        self.master_volume.store(vol.to_bits(), Ordering::Relaxed);
    }

    pub fn set_pulse_rate(&self, hz: f32) {
        let rate = if hz.is_finite() && hz > 0.0 { hz } else { 0.0 };
        self.pulse_rate.store(rate.to_bits(), Ordering::Relaxed);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    Idle,
    Attack,
    Hold,
    Release,
}

/// The repeating error beep. A countdown in samples spaces the beeps; the rate is read once per
/// beep, so the rhythm never glides. When the rate drops to 0 (or audio is disabled) no new
/// beep starts and a running one moves into its release from its current level.
struct Beeper {
    sample_rate: f32,
    dt: f32,
    attack_step: f32,
    release_decay: f32,
    stage: Stage,
    env: f32,
    phase: f32,
    samples_to_next: u32,
    hold_left: u32,
    /// Beeps started so far (for tests).
    starts: u32,
}

impl Beeper {
    fn new(sample_rate: f32) -> Self {
        let dt = 1.0 / sample_rate;
        Self {
            sample_rate,
            dt,
            attack_step: (dt / BEEP_ATTACK_TIME_S).clamp(0.0, 1.0),
            release_decay: (-dt / BEEP_RELEASE_TAU_S).exp(),
            stage: Stage::Idle,
            env: 0.0,
            phase: 0.0,
            samples_to_next: 0,
            hold_left: 0,
            starts: 0,
        }
    }

    /// Hold length in samples for a beep at `rate` Hz.
    fn hold_samples(&self, rate: f32) -> u32 {
        let hold_s = BEEP_HOLD_MAX_S
            .min(BEEP_ACTIVE_SHARE / rate - BEEP_HOLD_MARGIN_S)
            .max(0.0);
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a small non-negative sample count"
        )]
        let samples = (hold_s * self.sample_rate).round() as u32;
        samples
    }

    /// Samples between two beep starts at `rate` Hz.
    fn period_samples(&self, rate: f32) -> u32 {
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a positive sample count, saturating for absurdly low rates"
        )]
        let samples = (self.sample_rate / rate).round() as u32;
        samples.max(1)
    }

    /// The next sample, before the master volume.
    fn next(&mut self, rate: f32, enabled: bool, smooth_factor: f32) -> f32 {
        if !self.phase.is_finite() {
            self.phase = 0.0;
        }
        if !self.env.is_finite() {
            self.env = 0.0;
            self.stage = Stage::Idle;
        }

        if enabled && rate > 0.0 {
            if self.samples_to_next == 0 {
                if self.env == 0.0 {
                    // The envelope is 0, so restarting the phase cannot click.
                    self.phase = 0.0;
                }
                self.stage = Stage::Attack;
                self.hold_left = self.hold_samples(rate);
                self.samples_to_next = self.period_samples(rate);
                self.starts = self.starts.wrapping_add(1);
            }
            self.samples_to_next = self.samples_to_next.saturating_sub(1);
        } else {
            // The next band exit beeps on the very next sample.
            self.samples_to_next = 0;
            if matches!(self.stage, Stage::Attack | Stage::Hold) {
                self.stage = Stage::Release;
            }
        }

        match self.stage {
            Stage::Idle => {}
            Stage::Attack => {
                self.env += self.attack_step;
                if self.env >= 1.0 {
                    self.env = 1.0;
                    self.stage = if self.hold_left == 0 {
                        Stage::Release
                    } else {
                        Stage::Hold
                    };
                }
            }
            Stage::Hold => {
                self.hold_left = self.hold_left.saturating_sub(1);
                if self.hold_left == 0 {
                    self.stage = Stage::Release;
                }
            }
            Stage::Release => {
                if enabled {
                    self.env *= self.release_decay;
                } else {
                    // Muting fades at the slew rate instead of cutting.
                    self.env -= self.env * smooth_factor;
                }
                if self.env < DENORMAL_THRESHOLD {
                    self.env = 0.0;
                    self.stage = Stage::Idle;
                }
            }
        }

        if self.env <= 0.0 {
            return 0.0;
        }
        self.phase = (self.phase + BEEP_FREQ_HZ * self.dt) % 1.0;
        let angle = self.phase * std::f32::consts::TAU;
        let wave = angle.sin() + BEEP_HARMONIC_GAIN * (3.0 * angle).sin();
        wave * self.env * ERROR_GAIN
    }
}

pub struct Synth {
    smooth_factor: f32,
    current_vol: f32,
    beeper: Beeper,
}

impl Synth {
    #[must_use]
    pub fn new(sample_rate: f32) -> Self {
        let sr = if sample_rate.is_finite() && sample_rate > 0.0 {
            sample_rate
        } else {
            44100.0
        };
        let dt = 1.0 / sr;
        // One-pole slew with a 2.5 ms time constant: fast enough for the ~20 ms response
        // budget, slow enough to keep volume and mute changes click-free.
        let smooth_factor = (dt / SLEW_TIME_S).clamp(0.0, 1.0);

        Self {
            smooth_factor,
            current_vol: 0.0,
            beeper: Beeper::new(sr),
        }
    }

    pub fn next_sample(&mut self, shared: &SharedState) -> f32 {
        if !self.current_vol.is_finite() {
            self.current_vol = 0.0;
        }

        let enabled = shared.enabled.load(Ordering::Relaxed);
        let raw_master_vol = f32::from_bits(shared.master_volume.load(Ordering::Relaxed));
        let master_vol = if raw_master_vol.is_finite() {
            raw_master_vol.clamp(0.0, 1.0)
        } else {
            0.0
        };
        let raw_rate = f32::from_bits(shared.pulse_rate.load(Ordering::Relaxed));
        let rate = if raw_rate.is_finite() && raw_rate > 0.0 {
            raw_rate
        } else {
            0.0
        };

        self.current_vol += (master_vol - self.current_vol) * self.smooth_factor;
        if master_vol == 0.0 && self.current_vol.abs() < DENORMAL_THRESHOLD {
            self.current_vol = 0.0;
        }

        let beep = self.beeper.next(rate, enabled, self.smooth_factor);

        let out_sample = beep * self.current_vol;
        if out_sample.is_finite() {
            out_sample.clamp(-1.0, 1.0)
        } else {
            0.0
        }
    }
}

#[cfg(test)]
impl Synth {
    #[must_use]
    pub fn beep_env(&self) -> f32 {
        self.beeper.env
    }

    #[must_use]
    pub fn beep_starts(&self) -> u32 {
        self.beeper.starts
    }

    #[must_use]
    pub fn beep_hold_samples(&self, rate: f32) -> u32 {
        self.beeper.hold_samples(rate)
    }

    #[must_use]
    pub fn beep_period_samples(&self, rate: f32) -> u32 {
        self.beeper.period_samples(rate)
    }
}

/// First retry delay after the output stream is lost or could not be opened.
const BACKOFF_INITIAL: Duration = Duration::from_millis(500);
/// The retry delay never grows beyond this.
const BACKOFF_MAX: Duration = Duration::from_secs(5);
/// A stream that survived this long counts as healthy and resets the backoff.
const STABLE_AFTER: Duration = Duration::from_secs(10);

/// Retry delays for the stream supervisor: 0.5 s, 1 s, 2 s, 4 s, then 5 s.
#[derive(Debug, Default)]
struct Backoff {
    attempt: u32,
}

impl Backoff {
    fn next_delay(&mut self) -> Duration {
        let factor = 1_u32.checked_shl(self.attempt).unwrap_or(u32::MAX);
        self.attempt = self.attempt.saturating_add(1);
        BACKOFF_INITIAL.saturating_mul(factor).min(BACKOFF_MAX)
    }

    fn reset(&mut self) {
        self.attempt = 0;
    }
}

/// Whether a stream error means the stream must be rebuilt. Only under-runs, a denied realtime
/// priority and an automatic reroute leave the stream running; on WASAPI any other error ends
/// the render thread (device gone, audio service restarted, device taken exclusively, ...).
fn needs_rebuild(kind: cpal::ErrorKind) -> bool {
    !matches!(
        kind,
        cpal::ErrorKind::Xrun | cpal::ErrorKind::RealtimeDenied | cpal::ErrorKind::DeviceChanged
    )
}

/// Audio output. The cpal stream lives on a supervisor thread that rebuilds it when the device
/// goes away or the default device changes; the synth state in [`SharedState`] survives rebuilds.
/// Clones are cheap and drive the same output.
#[derive(Clone)]
pub struct AudioFeedback {
    state: Arc<SharedState>,
    test_tone_generation: Arc<AtomicU64>,
}

impl Default for AudioFeedback {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioFeedback {
    #[must_use]
    pub fn new() -> Self {
        let state = Arc::new(SharedState::new());
        let supervisor_state = state.clone();
        if let Err(err) = std::thread::Builder::new()
            .name("audio-supervisor".into())
            .spawn(move || supervise(&supervisor_state))
        {
            eprintln!("Audio: failed to start supervisor thread: {err}");
        }

        Self {
            state,
            test_tone_generation: Arc::new(AtomicU64::new(0)),
        }
    }

    /// An output with no audio device behind it, for tests of the code that drives it.
    #[cfg(test)]
    pub(crate) fn detached() -> Self {
        Self {
            state: Arc::new(SharedState::new()),
            test_tone_generation: Arc::new(AtomicU64::new(0)),
        }
    }

    /// The beep rate last set, in Hz.
    #[cfg(test)]
    pub(crate) fn pulse_rate(&self) -> f32 {
        f32::from_bits(self.state.pulse_rate.load(Ordering::Relaxed))
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.state.set_enabled(enabled);
    }

    pub fn set_volume(&self, volume: f32) {
        self.state.set_volume(volume);
    }

    /// Sets the error beep rate in Hz; `0.0` (or a non-finite or negative value) is silence.
    pub fn set_pulse_rate(&self, hz: f32) {
        self.state.set_pulse_rate(hz);
    }

    /// Plays a 1.5 s demo: the beep rate steps from 3 to 11 Hz, then falls silent. A newer
    /// call supersedes an older demo, so rapid clicks do not stack.
    #[cfg_attr(
        not(debug_assertions),
        expect(dead_code, reason = "test sounds are debug-only")
    )]
    pub fn test_tone(&self) {
        let generation = self.test_tone_generation.fetch_add(1, Ordering::Relaxed) + 1;
        let state = self.state.clone();
        let latest = self.test_tone_generation.clone();
        std::thread::spawn(move || {
            for step in 0..TEST_DEMO_STEPS {
                if latest.load(Ordering::Relaxed) != generation {
                    return;
                }
                #[expect(clippy::cast_precision_loss, reason = "tiny step counts")]
                let k = step as f32 / (TEST_DEMO_STEPS - 1) as f32;
                state.set_pulse_rate(TEST_DEMO_MIN_HZ + (TEST_DEMO_MAX_HZ - TEST_DEMO_MIN_HZ) * k);
                std::thread::sleep(TEST_DEMO_STEP);
            }
            if latest.load(Ordering::Relaxed) == generation {
                state.set_pulse_rate(0.0);
            }
        });
    }
}

/// Owns the cpal stream (it is not `Send` on every platform): opens it, waits for a fatal
/// error from the stream's error callback, then drops it and opens a new one with backoff.
/// Also retries when there was no usable output device at startup.
fn supervise(state: &Arc<SharedState>) {
    let (tx, rx) = mpsc::channel::<()>();
    let mut backoff = Backoff::default();
    loop {
        let started = Instant::now();
        // A failed open can leave its stream's error message queued; it must not end the
        // next, healthy stream.
        while rx.try_recv().is_ok() {}
        match open_stream(state, &tx) {
            Ok(stream) => {
                // The error callback sends one message per fatal error; `tx` stays alive here,
                // so this blocks until the stream dies.
                let _ = rx.recv();
                drop(stream);
                while rx.try_recv().is_ok() {}
                if started.elapsed() >= STABLE_AFTER {
                    backoff.reset();
                }
                eprintln!("Audio: output stream lost, rebuilding");
            }
            Err(err) => eprintln!("Audio: {err}"),
        }
        std::thread::sleep(backoff.next_delay());
    }
}

fn open_stream(state: &Arc<SharedState>, lost: &mpsc::Sender<()>) -> Result<Stream, String> {
    let device = cpal::default_host()
        .default_output_device()
        .ok_or("no default output device available")?;
    let mut config: StreamConfig = device
        .default_output_config()
        .map_err(|err| format!("failed to get default output config: {err}"))?
        .into();
    // WASAPI opens shared-mode streams with AUTOCONVERTPCM, so f32 works whatever the
    // device's native format is.
    config.buffer_size = BufferSize::Default;

    #[expect(clippy::cast_precision_loss, reason = "sample rate fits in f32")]
    let mut synth = Synth::new(config.sample_rate as f32);
    let channels = usize::from(config.channels).max(1);
    let callback_state = state.clone();
    let lost = lost.clone();

    let stream = device
        .build_output_stream(
            config,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                for frame in data.chunks_mut(channels) {
                    let sample = synth.next_sample(&callback_state);
                    frame.fill(sample);
                }
            },
            move |err: cpal::Error| {
                eprintln!("Audio output error: {err}");
                if needs_rebuild(err.kind()) {
                    let _ = lost.send(());
                }
            },
            None,
        )
        .map_err(|err| format!("failed to build output stream: {err}"))?;
    stream
        .play()
        .map_err(|err| format!("failed to play audio stream: {err}"))?;
    Ok(stream)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_volume_clamps_and_maps_non_finite() {
        let shared = SharedState::new();
        shared.set_volume(1.5);
        assert_eq!(
            f32::from_bits(shared.master_volume.load(Ordering::Relaxed)),
            1.0
        );

        shared.set_volume(-0.5);
        assert_eq!(
            f32::from_bits(shared.master_volume.load(Ordering::Relaxed)),
            0.0
        );

        shared.set_volume(f32::NAN);
        assert_eq!(
            f32::from_bits(shared.master_volume.load(Ordering::Relaxed)),
            0.0
        );

        shared.set_volume(f32::INFINITY);
        assert_eq!(
            f32::from_bits(shared.master_volume.load(Ordering::Relaxed)),
            0.0
        );
    }

    #[test]
    fn set_pulse_rate_maps_bad_input_to_zero() {
        let audio = AudioFeedback::detached();
        audio.set_pulse_rate(7.5);
        assert_eq!(audio.pulse_rate(), 7.5);
        for bad in [f32::NAN, f32::INFINITY, -3.0] {
            audio.set_pulse_rate(7.5);
            audio.set_pulse_rate(bad);
            assert_eq!(audio.pulse_rate(), 0.0);
        }
    }

    #[test]
    fn silent_at_rate_zero() {
        let mut synth = Synth::new(48_000.0);
        let shared = SharedState::new();
        shared.set_volume(1.0);
        for _ in 0..5000 {
            assert_eq!(synth.next_sample(&shared), 0.0);
        }
        assert_eq!(synth.beep_starts(), 0);
    }

    #[test]
    fn silent_when_disabled() {
        let mut synth = Synth::new(48_000.0);
        let shared = SharedState::new();
        shared.set_enabled(false);
        shared.set_volume(1.0);
        shared.set_pulse_rate(11.0);
        for _ in 0..5000 {
            assert_eq!(synth.next_sample(&shared), 0.0);
        }
        assert_eq!(synth.beep_starts(), 0);
    }

    #[test]
    fn first_beep_starts_on_the_first_sample() {
        for sr in [48_000.0_f32, 96_000.0] {
            let mut synth = Synth::new(sr);
            let shared = SharedState::new();
            shared.set_volume(1.0);
            for _ in 0..100 {
                synth.next_sample(&shared);
            }
            shared.set_pulse_rate(3.0);
            synth.next_sample(&shared);
            assert_eq!(synth.beep_starts(), 1, "at {sr} Hz");
            assert!(synth.beep_env() > 0.0);
            // Audible within 20 ms.
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "20 ms of samples is a small positive count"
            )]
            let samples = (sr * 0.020) as usize;
            let mut peak = 0.0_f32;
            for _ in 0..samples {
                peak = peak.max(synth.next_sample(&shared).abs());
            }
            assert!(peak > 0.05, "peak {peak} at {sr} Hz");
        }
    }

    /// The sample indexes at which beeps start over `samples` samples at a fixed `rate`.
    fn start_indexes(sr: f32, rate: f32, samples: usize) -> Vec<usize> {
        let mut synth = Synth::new(sr);
        let shared = SharedState::new();
        shared.set_volume(1.0);
        shared.set_pulse_rate(rate);
        let mut starts = Vec::new();
        let mut seen = 0;
        for i in 0..samples {
            synth.next_sample(&shared);
            if synth.beep_starts() != seen {
                seen = synth.beep_starts();
                starts.push(i);
            }
        }
        starts
    }

    #[test]
    fn beeps_are_spaced_by_the_rounded_period() {
        for sr in [48_000.0_f32, 96_000.0] {
            for rate in [3.0_f32, 7.0, 11.0] {
                let synth = Synth::new(sr);
                let period = synth.beep_period_samples(rate) as usize;
                #[expect(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    reason = "small positive count"
                )]
                let expected = (sr / rate).round() as usize;
                assert_eq!(period, expected);
                let starts = start_indexes(sr, rate, period * 5 + 10);
                assert_eq!(starts.len(), 6, "{rate} Hz at {sr}");
                assert_eq!(starts[0], 0);
                for pair in starts.windows(2) {
                    assert_eq!(pair[1] - pair[0], period, "{rate} Hz at {sr}");
                }
            }
        }
    }

    #[test]
    fn hold_length_follows_the_formula() {
        let synth = Synth::new(48_000.0);
        // 3 Hz: capped at 32 ms. 11 Hz: 0.45 / 11 - 8 ms = 32.9 ms, still capped.
        assert_eq!(synth.beep_hold_samples(3.0), 1536);
        assert_eq!(synth.beep_hold_samples(11.0), 1536);
        // 20 Hz: 0.45 / 20 - 8 ms = 14.5 ms.
        assert_eq!(synth.beep_hold_samples(20.0), 696);
        // Very high rates clamp at 0.
        assert_eq!(synth.beep_hold_samples(100.0), 0);

        // The envelope really stays at 1 for hold + 1 samples (the sample that ends the attack
        // plus the hold itself).
        for rate in [3.0_f32, 11.0] {
            let mut synth = Synth::new(48_000.0);
            let shared = SharedState::new();
            shared.set_volume(1.0);
            shared.set_pulse_rate(rate);
            let mut full = 0;
            for _ in 0..4000 {
                synth.next_sample(&shared);
                if synth.beep_env() >= 1.0 {
                    full += 1;
                }
            }
            assert_eq!(full, synth.beep_hold_samples(rate) + 1, "{rate} Hz");
        }
    }

    #[test]
    fn release_reaches_exactly_zero() {
        for sr in [48_000.0_f32, 96_000.0] {
            let mut synth = Synth::new(sr);
            let shared = SharedState::new();
            shared.set_volume(1.0);
            shared.set_pulse_rate(3.0);
            synth.next_sample(&shared);
            // Stop further beeps and let the first one die away.
            shared.set_pulse_rate(0.0);
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "small positive count"
            )]
            let samples = (sr * 0.2) as usize;
            for _ in 0..samples {
                synth.next_sample(&shared);
            }
            assert_eq!(synth.beep_env(), 0.0, "at {sr} Hz");
            assert_eq!(synth.beep_starts(), 1);
        }
    }

    #[test]
    fn rate_change_applies_from_the_next_beep() {
        let mut synth = Synth::new(48_000.0);
        let shared = SharedState::new();
        shared.set_volume(1.0);
        shared.set_pulse_rate(3.0);
        let slow = synth.beep_period_samples(3.0) as usize;
        let fast = synth.beep_period_samples(11.0) as usize;
        let mut starts = Vec::new();
        let mut seen = 0;
        for i in 0..slow + fast * 3 + 10 {
            if i == 1000 {
                shared.set_pulse_rate(11.0);
            }
            synth.next_sample(&shared);
            if synth.beep_starts() != seen {
                seen = synth.beep_starts();
                starts.push(i);
            }
        }
        assert_eq!(starts[0], 0);
        assert_eq!(starts[1] - starts[0], slow, "old interval finishes first");
        assert_eq!(starts[2] - starts[1], fast);
        assert_eq!(starts[3] - starts[2], fast);
    }

    #[test]
    fn rate_drop_to_zero_resets_the_countdown() {
        let mut synth = Synth::new(48_000.0);
        let shared = SharedState::new();
        shared.set_volume(1.0);
        shared.set_pulse_rate(3.0);
        for _ in 0..1000 {
            synth.next_sample(&shared);
        }
        shared.set_pulse_rate(0.0);
        for _ in 0..20_000 {
            synth.next_sample(&shared);
        }
        let before = synth.beep_starts();
        shared.set_pulse_rate(3.0);
        synth.next_sample(&shared);
        assert_eq!(synth.beep_starts(), before + 1);
    }

    #[test]
    fn no_nan_after_nan_input() {
        let mut synth = Synth::new(48_000.0);
        let shared = SharedState::new();
        shared.set_volume(1.0);
        shared.set_pulse_rate(11.0);
        for _ in 0..500 {
            assert!(synth.next_sample(&shared).is_finite());
        }
        shared
            .pulse_rate
            .store(f32::NAN.to_bits(), Ordering::Relaxed);
        for _ in 0..5000 {
            assert!(synth.next_sample(&shared).is_finite());
        }
        shared
            .master_volume
            .store(f32::NAN.to_bits(), Ordering::Relaxed);
        for _ in 0..2000 {
            assert!(synth.next_sample(&shared).is_finite());
        }
        // Normal input resumes the beeps.
        shared.set_volume(0.5);
        shared.set_pulse_rate(11.0);
        let mut non_zero_seen = false;
        for _ in 0..3000 {
            let sample = synth.next_sample(&shared);
            assert!(sample.is_finite());
            non_zero_seen |= sample.abs() > 0.005;
        }
        assert!(non_zero_seen);
    }

    #[test]
    fn corrupted_state_recovers() {
        let mut synth = Synth::new(48_000.0);
        let shared = SharedState::new();
        shared.set_volume(1.0);
        shared.set_pulse_rate(5.0);
        synth.beeper.phase = f32::NAN;
        synth.beeper.env = f32::NAN;
        synth.current_vol = f32::NAN;
        for _ in 0..2000 {
            assert!(synth.next_sample(&shared).is_finite());
        }
        assert!(synth.beep_starts() > 0);
    }

    /// Largest per-sample jump allowed at 48 kHz: a 0.15-peak, 784 Hz beep with its third
    /// harmonic has a natural slope of about 0.03 per sample.
    const MAX_STEP: f32 = 0.05;

    struct StepMeter {
        prev: f32,
        max_step: f32,
    }

    impl StepMeter {
        fn new() -> Self {
            Self {
                prev: 0.0,
                max_step: 0.0,
            }
        }

        fn run(&mut self, synth: &mut Synth, shared: &SharedState, samples: usize) {
            for _ in 0..samples {
                let s = synth.next_sample(shared);
                self.max_step = self.max_step.max((s - self.prev).abs());
                self.prev = s;
            }
        }

        fn assert_smooth(&self, what: &str) {
            assert!(
                self.max_step <= MAX_STEP,
                "{what}: per-sample step {} exceeds {MAX_STEP}",
                self.max_step
            );
        }
    }

    #[test]
    fn rate_to_zero_mid_hold_has_no_clicks() {
        let mut synth = Synth::new(48_000.0);
        let shared = SharedState::new();
        shared.set_volume(1.0);
        let mut meter = StepMeter::new();
        shared.set_pulse_rate(5.0);
        // Attack is 192 samples; stop well inside the hold.
        meter.run(&mut synth, &shared, 500);
        assert!(synth.beep_env() >= 1.0);
        shared.set_pulse_rate(0.0);
        meter.run(&mut synth, &shared, 6000);
        meter.assert_smooth("rate to zero");
        assert_eq!(synth.beep_env(), 0.0);
    }

    #[test]
    fn enable_disable_mid_beep_has_no_clicks() {
        let mut synth = Synth::new(48_000.0);
        let shared = SharedState::new();
        shared.set_volume(1.0);
        shared.set_pulse_rate(11.0);
        let mut meter = StepMeter::new();
        meter.run(&mut synth, &shared, 600);
        shared.set_enabled(false);
        meter.run(&mut synth, &shared, 6000);
        assert_eq!(synth.beep_env(), 0.0);
        shared.set_enabled(true);
        meter.run(&mut synth, &shared, 600);
        // Mute and un-mute within one beep.
        shared.set_enabled(false);
        meter.run(&mut synth, &shared, 50);
        shared.set_enabled(true);
        meter.run(&mut synth, &shared, 6000);
        meter.assert_smooth("enable/disable");
    }

    #[test]
    fn volume_steps_have_no_clicks() {
        let mut synth = Synth::new(48_000.0);
        let shared = SharedState::new();
        shared.set_volume(0.0);
        shared.set_pulse_rate(11.0);
        let mut meter = StepMeter::new();
        meter.run(&mut synth, &shared, 500);
        shared.set_volume(1.0);
        meter.run(&mut synth, &shared, 2000);
        shared.set_volume(0.0);
        meter.run(&mut synth, &shared, 6000);
        shared.set_volume(1.0);
        meter.run(&mut synth, &shared, 6000);
        meter.assert_smooth("volume step");
    }

    #[test]
    fn backoff_grows_and_caps() {
        let mut backoff = Backoff::default();
        let ms: Vec<u128> = (0..8).map(|_| backoff.next_delay().as_millis()).collect();
        assert_eq!(ms, [500, 1000, 2000, 4000, 5000, 5000, 5000, 5000]);
        backoff.reset();
        assert_eq!(backoff.next_delay(), Duration::from_millis(500));
    }

    #[test]
    fn backoff_never_overflows() {
        let mut backoff = Backoff::default();
        for _ in 0..200 {
            assert!(backoff.next_delay() <= BACKOFF_MAX);
        }
    }

    #[test]
    fn every_error_but_transient_ones_triggers_a_rebuild() {
        use cpal::ErrorKind;
        for kind in [
            ErrorKind::DeviceNotAvailable,
            ErrorKind::StreamInvalidated,
            ErrorKind::HostUnavailable,
            ErrorKind::DeviceBusy,
            ErrorKind::BackendError,
            ErrorKind::Other,
        ] {
            assert!(needs_rebuild(kind), "{kind:?}");
        }
        for kind in [
            ErrorKind::Xrun,
            ErrorKind::RealtimeDenied,
            ErrorKind::DeviceChanged,
        ] {
            assert!(!needs_rebuild(kind), "{kind:?}");
        }
    }
}
