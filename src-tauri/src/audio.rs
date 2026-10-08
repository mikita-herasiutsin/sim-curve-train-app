use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{BufferSize, SampleFormat, Stream, StreamConfig};
use sct_core::audio_map::{BASE_FREQ_HZ, ToneTarget};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

const CHIME_FREQ_HZ: f32 = 1000.0;
const CHIME_GAIN: f32 = 0.3;
const CHIME_DECAY_TIME_S: f32 = 0.08;
const DENORMAL_THRESHOLD: f32 = 1e-6;
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
    current_freq: f32,
    current_gain: f32,
    phase: f32,
    chime_phase: f32,
    chime_env: f32,
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
        let smooth_factor = (dt * 150.0).clamp(0.0, 1.0);
        let chime_decay = (-dt / CHIME_DECAY_TIME_S).exp();

        Self {
            dt,
            smooth_factor,
            chime_decay,
            current_freq: BASE_FREQ_HZ,
            current_gain: 0.0,
            phase: 0.0,
            chime_phase: 0.0,
            chime_env: 0.0,
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

        // Chime triggering and gating: enabled gates the chime
        let chime_triggered = shared.chime_trigger.load(Ordering::Relaxed)
            && shared.chime_trigger.swap(false, Ordering::Relaxed);

        if enabled {
            if chime_triggered {
                self.chime_phase = 0.0;
                self.chime_env = 1.0;
            }
        } else {
            self.chime_env = 0.0;
        }

        // Toggling enabled fades gain smoothly (treat disabled as target gain 0)
        let effective_target_gain = if enabled { target_gain } else { 0.0 };

        // Only slew frequency while target gain > 0 (hold the last frequency while fading out)
        if effective_target_gain > 0.0 {
            self.current_freq += (target_freq - self.current_freq) * self.smooth_factor;
        }
        self.current_gain += (effective_target_gain - self.current_gain) * self.smooth_factor;

        // Flush denormals
        if effective_target_gain == 0.0 && self.current_gain.abs() < DENORMAL_THRESHOLD {
            self.current_gain = 0.0;
        }

        self.phase = (self.phase + self.current_freq * self.dt) % 1.0;
        let tone_sample = if self.current_gain > 0.0 {
            (self.phase * std::f32::consts::TAU).sin() * self.current_gain
        } else {
            0.0
        };

        let mut chime_sample = 0.0;
        if enabled && self.chime_env > 0.0 {
            self.chime_phase = (self.chime_phase + CHIME_FREQ_HZ * self.dt) % 1.0;
            chime_sample =
                (self.chime_phase * std::f32::consts::TAU).sin() * self.chime_env * CHIME_GAIN;
            self.chime_env *= self.chime_decay;
            if self.chime_env.abs() < DENORMAL_THRESHOLD {
                self.chime_env = 0.0;
            }
        }

        let out_sample = (tone_sample + chime_sample) * master_vol;
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

#[derive(Clone)]
pub struct AudioFeedback {
    _stream: Option<Arc<Stream>>,
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
        let stream = Self::start_audio_stream(state.clone());

        Self {
            _stream: stream.map(Arc::new),
            state,
            test_tone_generation: Arc::new(AtomicU64::new(0)),
        }
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

    pub fn test_tone(&self) {
        self.chime();
        let generation = self.test_tone_generation.fetch_add(1, Ordering::Relaxed) + 1;
        self.update(ToneTarget {
            frequency_hz: 600.0,
            gain: 0.3,
        });
        let audio = self.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(500));
            if audio.test_tone_generation.load(Ordering::Relaxed) == generation {
                audio.update(ToneTarget {
                    frequency_hz: BASE_FREQ_HZ,
                    gain: 0.0,
                });
            }
        });
    }

    fn start_audio_stream(state: Arc<SharedState>) -> Option<Stream> {
        let host = cpal::default_host();
        let Some(device) = host.default_output_device() else {
            eprintln!("Audio: no default output device available");
            return None;
        };

        let supported_config = match device.default_output_config() {
            Ok(config) => config,
            Err(err) => {
                eprintln!("Audio: failed to get default output config: {err}");
                return None;
            }
        };

        let fixed_frames = match supported_config.buffer_size() {
            cpal::SupportedBufferSize::Range { min, max } => Some((*min).max(256).min(*max)),
            cpal::SupportedBufferSize::Unknown => None,
        };

        let sample_format = supported_config.sample_format();
        let config: StreamConfig = supported_config.into();

        let stream = build_stream_with_retry(&device, config, sample_format, fixed_frames, state)?;

        match stream.play() {
            Ok(()) => Some(stream),
            Err(err) => {
                eprintln!("Audio: failed to play audio stream: {err}");
                None
            }
        }
    }
}

fn try_build_f32_stream(
    device: &cpal::Device,
    config: &StreamConfig,
    state: Arc<SharedState>,
) -> Result<Stream, cpal::Error> {
    #[expect(clippy::cast_precision_loss, reason = "sample rate fits in f32")]
    let sample_rate = config.sample_rate as f32;
    let mut synth = Synth::new(sample_rate);
    let channels = config.channels as usize;
    let err_fn = |err| eprintln!("Audio output error: {err}");

    device.build_output_stream(
        *config,
        move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
            for frame in data.chunks_mut(channels) {
                let sample = synth.next_sample(&state);
                for s in frame.iter_mut() {
                    *s = sample;
                }
            }
        },
        err_fn,
        None,
    )
}

fn try_build_i16_stream(
    device: &cpal::Device,
    config: &StreamConfig,
    state: Arc<SharedState>,
) -> Result<Stream, cpal::Error> {
    #[expect(clippy::cast_precision_loss, reason = "sample rate fits in f32")]
    let sample_rate = config.sample_rate as f32;
    let mut synth = Synth::new(sample_rate);
    let channels = config.channels as usize;
    let err_fn = |err| eprintln!("Audio output error: {err}");

    device.build_output_stream(
        *config,
        move |data: &mut [i16], _: &cpal::OutputCallbackInfo| {
            for frame in data.chunks_mut(channels) {
                let sample = synth.next_sample(&state);
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "clamped audio sample fits in i16"
                )]
                let i16_sample = (sample.clamp(-1.0, 1.0) * 32767.0).round() as i16;
                for s in frame.iter_mut() {
                    *s = i16_sample;
                }
            }
        },
        err_fn,
        None,
    )
}

fn build_stream_with_retry(
    device: &cpal::Device,
    mut config: StreamConfig,
    sample_format: SampleFormat,
    fixed_frames: Option<u32>,
    state: Arc<SharedState>,
) -> Option<Stream> {
    if let Some(frames) = fixed_frames {
        config.buffer_size = BufferSize::Fixed(frames);
        let res = match sample_format {
            SampleFormat::F32 => try_build_f32_stream(device, &config, state.clone()),
            SampleFormat::I16 => try_build_i16_stream(device, &config, state.clone()),
            other => {
                eprintln!("Audio: unsupported sample format: {other:?}");
                return None;
            }
        };
        match res {
            Ok(stream) => return Some(stream),
            Err(err) => {
                eprintln!(
                    "Audio: build_output_stream failed with fixed buffer size ({err}), retrying with default buffer size"
                );
            }
        }
    }

    config.buffer_size = BufferSize::Default;
    let res = match sample_format {
        SampleFormat::F32 => try_build_f32_stream(device, &config, state),
        SampleFormat::I16 => try_build_i16_stream(device, &config, state),
        other => {
            eprintln!("Audio: unsupported sample format: {other:?}");
            return None;
        }
    };
    match res {
        Ok(stream) => Some(stream),
        Err(err) => {
            eprintln!("Audio: failed to build output stream: {err}");
            None
        }
    }
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
        let sample = synth.next_sample(&shared);
        assert!(synth.current_gain() > 0.3);
        assert!(sample.abs() > 0.0 || synth.current_gain() > 0.0);

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

        // Run until completely faded
        for _ in 0..5000 {
            synth.next_sample(&shared);
            assert_eq!(synth.current_freq(), held_freq);
        }
        assert_eq!(synth.current_gain(), 0.0);
    }

    #[test]
    fn chime_decays_to_zero_at_48khz_and_96khz() {
        // Test at 48 kHz
        let mut synth48 = Synth::new(48_000.0);
        let shared = SharedState::new();
        shared.chime();

        // Sample 0: triggers chime
        let s0 = synth48.next_sample(&shared);
        assert!(s0.is_finite());
        assert!((synth48.chime_env() - synth48.chime_decay).abs() < 1e-5);

        // At 0.08s (48000 * 0.08 = 3840 samples), envelope should be around 1/e (~0.368)
        for _ in 1..3840 {
            synth48.next_sample(&shared);
        }
        let env_48_80ms = synth48.chime_env();
        let expected_e_decay = (-1.0_f32).exp();
        assert!((env_48_80ms - expected_e_decay).abs() < 0.01);

        // At ~1.15s (55200 samples total), envelope should be flushed to 0.0
        for _ in 3840..55_200 {
            synth48.next_sample(&shared);
        }
        assert_eq!(synth48.chime_env(), 0.0);

        // Test at 96 kHz
        let mut synth96 = Synth::new(96_000.0);
        shared.chime();

        synth96.next_sample(&shared);
        // At 0.08s (96000 * 0.08 = 7680 samples), envelope should be around 1/e (~0.368)
        for _ in 1..7680 {
            synth96.next_sample(&shared);
        }
        let env_96_80ms = synth96.chime_env();
        assert!((env_96_80ms - expected_e_decay).abs() < 0.01);

        // At ~1.15s (110400 samples total), envelope should be flushed to 0.0
        for _ in 7680..110_400 {
            synth96.next_sample(&shared);
        }
        assert_eq!(synth96.chime_env(), 0.0);
    }
}
