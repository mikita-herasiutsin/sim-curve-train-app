use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{BufferSize, Stream, StreamConfig};
use sct_core::audio_map::{BASE_FREQ_HZ, ToneTarget};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

const CHIME_FREQ_HZ: f32 = 1000.0;
const CHIME_GAIN: f32 = 0.3;
const CHIME_DECAY_TIME_S: f32 = 0.08;
const CHIME_ATTACK_TIME_S: f32 = 0.002;
const DENORMAL_THRESHOLD: f32 = 1e-6;
const SLEW_TIME_S: f32 = 0.0025;
pub const DEFAULT_AUDIO_VOLUME: f32 = 0.2;

#[inline]
fn pack_freq_gain(freq: f32, gain: f32) -> u64 {
    let freq_bits = u64::from(freq.to_bits());
    let gain_bits = u64::from(gain.to_bits());
    (freq_bits << 32) | gain_bits
}

#[inline]
fn unpack_freq_gain(packed: u64) -> (f32, f32) {
    let freq_bits = (packed >> 32) as u32;
    #[expect(clippy::cast_possible_truncation, reason = "lower 32 bits fit in u32")]
    let gain_bits = packed as u32;
    (f32::from_bits(freq_bits), f32::from_bits(gain_bits))
}

/// Contains the lock-free shared state read by the audio callback.
pub struct SharedState {
    pub enabled: AtomicBool,
    pub master_volume: AtomicU32,    // f32 bits
    pub target_freq_gain: AtomicU64, // high 32: freq, low 32: gain
    pub chime_trigger: AtomicBool,
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
            target_freq_gain: AtomicU64::new(pack_freq_gain(BASE_FREQ_HZ, 0.0)),
            chime_trigger: AtomicBool::new(false),
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

    pub fn update(&self, target: ToneTarget) {
        let freq = if target.frequency_hz.is_finite() {
            target.frequency_hz
        } else {
            BASE_FREQ_HZ
        };
        let gain = if target.gain.is_finite() {
            target.gain.clamp(0.0, 1.0)
        } else {
            0.0
        };
        self.target_freq_gain
            .store(pack_freq_gain(freq, gain), Ordering::Relaxed);
    }

    pub fn chime(&self) {
        self.chime_trigger.store(true, Ordering::Relaxed);
    }
}

pub struct Synth {
    dt: f32,
    smooth_factor: f32,
    chime_decay: f32,
    chime_attack_step: f32,
    current_freq: f32,
    current_gain: f32,
    current_vol: f32,
    phase: f32,
    chime_phase: f32,
    chime_env: f32,
    chime_attacking: bool,
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
        // budget, slow enough to keep gain and pitch changes click-free.
        let smooth_factor = (dt / SLEW_TIME_S).clamp(0.0, 1.0);
        let chime_decay = (-dt / CHIME_DECAY_TIME_S).exp();
        let chime_attack_step = (dt / CHIME_ATTACK_TIME_S).clamp(0.0, 1.0);

        Self {
            dt,
            smooth_factor,
            chime_decay,
            chime_attack_step,
            current_freq: BASE_FREQ_HZ,
            current_gain: 0.0,
            current_vol: 0.0,
            phase: 0.0,
            chime_phase: 0.0,
            chime_env: 0.0,
            chime_attacking: false,
        }
    }

    pub fn next_sample(&mut self, shared: &SharedState) -> f32 {
        if !self.phase.is_finite() {
            self.phase = 0.0;
        }
        if !self.current_freq.is_finite() {
            self.current_freq = BASE_FREQ_HZ;
        }
        if !self.current_gain.is_finite() {
            self.current_gain = 0.0;
        }
        if !self.chime_phase.is_finite() {
            self.chime_phase = 0.0;
        }
        if !self.current_vol.is_finite() {
            self.current_vol = 0.0;
        }
        if !self.chime_env.is_finite() {
            self.chime_env = 0.0;
        }

        let enabled = shared.enabled.load(Ordering::Relaxed);
        let raw_master_vol = f32::from_bits(shared.master_volume.load(Ordering::Relaxed));
        let packed = shared.target_freq_gain.load(Ordering::Relaxed);
        let (raw_target_freq, raw_target_gain) = unpack_freq_gain(packed);

        let master_vol = if raw_master_vol.is_finite() {
            raw_master_vol.clamp(0.0, 1.0)
        } else {
            0.0
        };
        let target_freq = if raw_target_freq.is_finite() {
            raw_target_freq.clamp(20.0, 20_000.0)
        } else {
            BASE_FREQ_HZ
        };
        let target_gain = if raw_target_gain.is_finite() {
            raw_target_gain.clamp(0.0, 1.0)
        } else {
            0.0
        };

        // Chime triggering: a disabled synth ignores (but consumes) the trigger. The envelope
        // ramps up quickly instead of jumping, and never resets the chime phase, so a retrigger
        // cannot click.
        let chime_triggered = shared.chime_trigger.load(Ordering::Relaxed)
            && shared.chime_trigger.swap(false, Ordering::Relaxed);
        if !enabled {
            self.chime_attacking = false;
        } else if chime_triggered {
            self.chime_attacking = true;
        }

        // Toggling enabled fades gain smoothly (treat disabled as target gain 0)
        let effective_target_gain = if enabled { target_gain } else { 0.0 };

        // A new tone starts at its own pitch: snap the frequency while silent, otherwise slew it
        // (and hold the last frequency while fading out).
        if self.current_gain == 0.0 {
            self.current_freq = target_freq;
        } else if effective_target_gain > 0.0 {
            self.current_freq += (target_freq - self.current_freq) * self.smooth_factor;
        }
        self.current_gain += (effective_target_gain - self.current_gain) * self.smooth_factor;
        self.current_vol += (master_vol - self.current_vol) * self.smooth_factor;

        // Flush denormals
        if master_vol == 0.0 && self.current_vol.abs() < DENORMAL_THRESHOLD {
            self.current_vol = 0.0;
        }
        if effective_target_gain == 0.0 && self.current_gain.abs() < DENORMAL_THRESHOLD {
            self.current_gain = 0.0;
        }

        self.phase = (self.phase + self.current_freq * self.dt) % 1.0;
        let tone_sample = if self.current_gain > 0.0 {
            (self.phase * std::f32::consts::TAU).sin() * self.current_gain
        } else {
            0.0
        };

        if self.chime_attacking {
            self.chime_env += self.chime_attack_step;
            if self.chime_env >= 1.0 {
                self.chime_env = 1.0;
                self.chime_attacking = false;
            }
        } else if enabled {
            self.chime_env *= self.chime_decay;
        } else {
            // Muted: fade the chime out at the gain slew rate instead of cutting it.
            self.chime_env -= self.chime_env * self.smooth_factor;
        }
        if !self.chime_attacking && self.chime_env.abs() < DENORMAL_THRESHOLD {
            self.chime_env = 0.0;
        }

        let mut chime_sample = 0.0;
        if self.chime_env > 0.0 {
            self.chime_phase = (self.chime_phase + CHIME_FREQ_HZ * self.dt) % 1.0;
            chime_sample =
                (self.chime_phase * std::f32::consts::TAU).sin() * self.chime_env * CHIME_GAIN;
        }

        let out_sample = (tone_sample + chime_sample) * self.current_vol;
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
    pub fn current_freq(&self) -> f32 {
        self.current_freq
    }

    #[must_use]
    pub fn current_gain(&self) -> f32 {
        self.current_gain
    }

    #[must_use]
    pub fn chime_env(&self) -> f32 {
        self.chime_env
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

    /// The tone last set with [`update`](Self::update).
    #[cfg(test)]
    pub(crate) fn tone(&self) -> ToneTarget {
        let (frequency_hz, gain) =
            unpack_freq_gain(self.state.target_freq_gain.load(Ordering::Relaxed));
        ToneTarget { frequency_hz, gain }
    }

    /// Whether a chime was requested since the last call.
    #[cfg(test)]
    pub(crate) fn take_chime(&self) -> bool {
        self.state.chime_trigger.swap(false, Ordering::Relaxed)
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.state.set_enabled(enabled);
    }

    pub fn set_volume(&self, volume: f32) {
        self.state.set_volume(volume);
    }

    pub fn update(&self, target: ToneTarget) {
        self.state.update(target);
    }

    pub fn chime(&self) {
        self.state.chime();
    }

    #[cfg_attr(
        not(debug_assertions),
        expect(dead_code, reason = "test tone is debug-only")
    )]
    pub fn test_tone(&self) {
        self.chime();
        let generation = self.test_tone_generation.fetch_add(1, Ordering::Relaxed) + 1;
        self.update(ToneTarget {
            frequency_hz: 600.0,
            gain: 0.3,
        });
        let state = self.state.clone();
        let latest = self.test_tone_generation.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(500));
            if latest.load(Ordering::Relaxed) == generation {
                state.update(ToneTarget {
                    frequency_hz: BASE_FREQ_HZ,
                    gain: 0.0,
                });
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
    fn pack_and_unpack_freq_gain_roundtrip() {
        let (freq, gain) = (440.0_f32, 0.25_f32);
        let packed = pack_freq_gain(freq, gain);
        let (u_freq, u_gain) = unpack_freq_gain(packed);
        assert_eq!(freq, u_freq);
        assert_eq!(gain, u_gain);
    }

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
    fn silent_when_disabled() {
        let mut synth = Synth::new(48000.0);
        let shared = SharedState::new();
        shared.set_enabled(false);
        shared.set_volume(1.0);
        shared.update(ToneTarget {
            frequency_hz: 600.0,
            gain: 0.5,
        });
        shared.chime();

        for _ in 0..1000 {
            let sample = synth.next_sample(&shared);
            assert_eq!(sample, 0.0);
        }
    }

    #[test]
    fn disable_fades_gain_smoothly() {
        let mut synth = Synth::new(48000.0);
        let shared = SharedState::new();
        shared.set_volume(1.0);
        shared.update(ToneTarget {
            frequency_hz: 440.0,
            gain: 0.5,
        });

        // Run until gain stabilizes near 0.5
        for _ in 0..2000 {
            synth.next_sample(&shared);
        }
        assert!(synth.current_gain() > 0.45);

        // Mute
        shared.set_enabled(false);

        // Next sample should NOT instantly drop to 0 (smooth fade)
        synth.next_sample(&shared);
        assert!(synth.current_gain() > 0.3);

        // Run until flush denormals takes it to 0
        for _ in 0..6000 {
            synth.next_sample(&shared);
        }
        assert_eq!(synth.current_gain(), 0.0);
    }

    #[test]
    fn no_nan_after_nan_input() {
        let mut synth = Synth::new(48000.0);
        let shared = SharedState::new();

        // Feed NaN inputs
        shared
            .master_volume
            .store(f32::NAN.to_bits(), Ordering::Relaxed);
        shared
            .target_freq_gain
            .store(pack_freq_gain(f32::NAN, f32::NAN), Ordering::Relaxed);

        for _ in 0..100 {
            let sample = synth.next_sample(&shared);
            assert!(!sample.is_nan());
            assert!(sample.is_finite());
            assert_eq!(sample, 0.0);
        }

        // Now resume normal inputs and verify synth recovers
        shared.set_volume(0.5);
        shared.update(ToneTarget {
            frequency_hz: 500.0,
            gain: 0.3,
        });

        let mut non_zero_seen = false;
        for _ in 0..1000 {
            let sample = synth.next_sample(&shared);
            assert!(!sample.is_nan());
            assert!(sample.is_finite());
            if sample.abs() > 0.01 {
                non_zero_seen = true;
            }
        }
        assert!(non_zero_seen);
    }

    #[test]
    fn frequency_held_while_fading() {
        let mut synth = Synth::new(48000.0);
        let shared = SharedState::new();
        shared.set_volume(1.0);
        shared.update(ToneTarget {
            frequency_hz: 700.0,
            gain: 0.5,
        });

        // Run until frequency slews close to 700.0
        for _ in 0..2000 {
            synth.next_sample(&shared);
        }
        assert!((synth.current_freq() - 700.0).abs() < 1.0);
        let held_freq = synth.current_freq();

        // Now set target gain to 0.0 with target frequency set to BASE_FREQ_HZ (440.0)
        shared.update(ToneTarget {
            frequency_hz: BASE_FREQ_HZ,
            gain: 0.0,
        });

        // While fading out, frequency must stay exactly held
        for _ in 0..500 {
            synth.next_sample(&shared);
            assert_eq!(synth.current_freq(), held_freq);
        }
        assert!(synth.current_gain() < 0.5);

        // Run until completely faded; the frequency stays held while any gain is left
        for _ in 0..5000 {
            synth.next_sample(&shared);
            assert_eq!(synth.current_freq(), held_freq);
            if synth.current_gain() == 0.0 {
                break;
            }
        }
        assert_eq!(synth.current_gain(), 0.0);
    }

    #[test]
    fn chime_decays_to_zero_at_48khz_and_96khz() {
        // Test at 48 kHz
        let mut synth48 = Synth::new(48_000.0);
        let shared = SharedState::new();
        shared.chime();

        // Sample 0: starts the attack ramp instead of jumping to full level
        let s0 = synth48.next_sample(&shared);
        assert!(s0.is_finite());
        assert!(synth48.chime_env() < 0.05);

        // 2 ms attack (96 samples), then 0.08 s (3840 samples) of decay: envelope is about 1/e
        for _ in 1..(96 + 3840) {
            synth48.next_sample(&shared);
        }
        let env_48_80ms = synth48.chime_env();
        let expected_e_decay = (-1.0_f32).exp();
        assert!((env_48_80ms - expected_e_decay).abs() < 0.01);

        // At ~1.15s (55200 samples total), envelope should be flushed to 0.0
        for _ in (96 + 3840)..55_200 {
            synth48.next_sample(&shared);
        }
        assert_eq!(synth48.chime_env(), 0.0);

        // Test at 96 kHz
        let mut synth96 = Synth::new(96_000.0);
        shared.chime();

        synth96.next_sample(&shared);
        for _ in 1..(192 + 7680) {
            synth96.next_sample(&shared);
        }
        let env_96_80ms = synth96.chime_env();
        assert!((env_96_80ms - expected_e_decay).abs() < 0.01);

        // At ~1.15s (110400 samples total), envelope should be flushed to 0.0
        for _ in (192 + 7680)..110_400 {
            synth96.next_sample(&shared);
        }
        assert_eq!(synth96.chime_env(), 0.0);
    }
    #[test]
    fn new_tone_starts_at_its_own_pitch() {
        let mut synth = Synth::new(48_000.0);
        let shared = SharedState::new();
        shared.set_volume(1.0);
        shared.update(ToneTarget {
            frequency_hz: 800.0,
            gain: 0.3,
        });
        synth.next_sample(&shared);
        assert_eq!(synth.current_freq(), 800.0);

        // Fade out completely, then start a different tone: no glide from the old pitch.
        shared.update(ToneTarget {
            frequency_hz: 800.0,
            gain: 0.0,
        });
        for _ in 0..20_000 {
            synth.next_sample(&shared);
        }
        assert_eq!(synth.current_gain(), 0.0);
        shared.update(ToneTarget {
            frequency_hz: 300.0,
            gain: 0.3,
        });
        synth.next_sample(&shared);
        assert_eq!(synth.current_freq(), 300.0);
    }

    /// The tone follows a new target within ~20 ms (SCT-038), at 48 kHz and 96 kHz.
    #[test]
    fn responds_within_20ms() {
        for rate in [48_000.0_f32, 96_000.0] {
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "20 ms of samples is a small positive count"
            )]
            let samples = (rate * 0.020) as usize;
            let mut synth = Synth::new(rate);
            let shared = SharedState::new();
            shared.set_volume(1.0);

            // Silence to tone: gain is near its target.
            shared.update(ToneTarget {
                frequency_hz: 440.0,
                gain: 0.2,
            });
            for _ in 0..samples {
                synth.next_sample(&shared);
            }
            assert!(synth.current_gain() >= 0.9 * 0.2, "gain at {rate} Hz");

            // Pitch change while sounding: frequency is near the new pitch.
            shared.update(ToneTarget {
                frequency_hz: 660.0,
                gain: 0.2,
            });
            for _ in 0..samples {
                synth.next_sample(&shared);
            }
            assert!(
                (synth.current_freq() - 660.0).abs() <= 0.1 * 220.0,
                "freq at {rate} Hz: {}",
                synth.current_freq()
            );

            // Chime: the envelope has risen and started its decay.
            shared.chime();
            for _ in 0..samples / 4 {
                synth.next_sample(&shared);
            }
            assert!(synth.chime_env() > 0.5, "chime at {rate} Hz");
        }
    }

    /// Largest per-sample jump allowed at 48 kHz. A 0.3-amplitude 1 kHz chime has a natural
    /// slope of about 0.039 per sample, and a 0.3-amplitude 600 Hz tone about 0.024.
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

    fn tone_600(gain: f32) -> ToneTarget {
        ToneTarget {
            frequency_hz: 600.0,
            gain,
        }
    }

    #[test]
    fn enable_disable_has_no_clicks() {
        let mut synth = Synth::new(48_000.0);
        let shared = SharedState::new();
        shared.set_volume(1.0);
        shared.update(tone_600(0.3));
        let mut meter = StepMeter::new();
        meter.run(&mut synth, &shared, 6000);
        shared.set_enabled(false);
        meter.run(&mut synth, &shared, 6000);
        shared.set_enabled(true);
        meter.run(&mut synth, &shared, 6000);
        meter.assert_smooth("enable/disable");
    }

    #[test]
    fn volume_steps_have_no_clicks() {
        let mut synth = Synth::new(48_000.0);
        let shared = SharedState::new();
        shared.set_volume(0.0);
        shared.update(tone_600(0.3));
        let mut meter = StepMeter::new();
        meter.run(&mut synth, &shared, 3000);
        shared.set_volume(1.0);
        meter.run(&mut synth, &shared, 6000);
        shared.set_volume(0.0);
        meter.run(&mut synth, &shared, 6000);
        meter.assert_smooth("volume step");
    }

    #[test]
    fn mute_during_chime_fades_out() {
        let mut synth = Synth::new(48_000.0);
        let shared = SharedState::new();
        shared.set_volume(1.0);
        let mut meter = StepMeter::new();
        meter.run(&mut synth, &shared, 3000);
        shared.chime();
        meter.run(&mut synth, &shared, 200);
        assert!(synth.chime_env() > 0.5);
        shared.set_enabled(false);
        meter.run(&mut synth, &shared, 1);
        assert!(synth.chime_env() > 0.0, "chime must fade, not cut");
        meter.run(&mut synth, &shared, 8000);
        meter.assert_smooth("mute during chime");
        assert_eq!(synth.chime_env(), 0.0);
    }

    #[test]
    fn chime_retrigger_has_no_clicks() {
        let mut synth = Synth::new(48_000.0);
        let shared = SharedState::new();
        shared.set_volume(1.0);
        let mut meter = StepMeter::new();
        meter.run(&mut synth, &shared, 3000);
        shared.chime();
        meter.run(&mut synth, &shared, 1500);
        let before = synth.chime_env();
        assert!(before < 0.9 && before > 0.1);
        shared.chime();
        meter.run(&mut synth, &shared, 300);
        assert!(
            synth.chime_env() > before,
            "retrigger must raise the envelope"
        );
        meter.run(&mut synth, &shared, 6000);
        meter.assert_smooth("chime retrigger");
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
