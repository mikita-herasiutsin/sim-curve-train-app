use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{BufferSize, SampleFormat, Stream, StreamConfig};
use sct_core::audio_map::ToneTarget;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// Contains the lock-free shared state read by the audio callback.
struct SharedState {
    enabled: AtomicBool,
    master_volume: AtomicU32, // f32 bits
    target_freq: AtomicU32,   // f32 bits
    target_gain: AtomicU32,   // f32 bits
    chime_trigger: AtomicBool,
}

impl SharedState {
    fn new() -> Self {
        Self {
            enabled: AtomicBool::new(true),
            master_volume: AtomicU32::new(0.5f32.to_bits()),
            target_freq: AtomicU32::new(0.0f32.to_bits()),
            target_gain: AtomicU32::new(0.0f32.to_bits()),
            chime_trigger: AtomicBool::new(false),
        }
    }
}

#[derive(Clone)]
pub struct AudioFeedback {
    _stream: Option<Arc<Stream>>,
    state: Arc<SharedState>,
}

impl AudioFeedback {
    pub fn new() -> Self {
        let state = Arc::new(SharedState::new());
        let stream = Self::start_audio_stream(state.clone());

        Self {
            _stream: stream.map(Arc::new),
            state,
        }
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.state.enabled.store(enabled, Ordering::Relaxed);
    }

    pub fn set_volume(&self, volume: f32) {
        self.state
            .master_volume
            .store(volume.to_bits(), Ordering::Relaxed);
    }

    pub fn update(&self, target: ToneTarget) {
        self.state
            .target_freq
            .store(target.frequency_hz.to_bits(), Ordering::Relaxed);
        self.state
            .target_gain
            .store(target.gain.to_bits(), Ordering::Relaxed);
    }

    pub fn chime(&self) {
        self.state.chime_trigger.store(true, Ordering::Relaxed);
    }

    #[expect(
        clippy::too_many_lines,
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        reason = "Stream setup is inherently lengthy, and precision loss is acceptable for audio samples"
    )]
    fn start_audio_stream(state: Arc<SharedState>) -> Option<Stream> {
        let host = cpal::default_host();
        let device = host.default_output_device()?;

        let mut supported_configs = device.supported_output_configs().ok()?;
        // Try to find a float config, else fallback
        let config_format = supported_configs
            .find(|c| c.sample_format() == SampleFormat::F32)
            .or_else(|| device.supported_output_configs().ok()?.next())?;

        // Smallest buffer size for latency
        let mut config: StreamConfig = config_format.with_max_sample_rate().into();
        if let cpal::SupportedBufferSize::Range { min, max } = config_format.buffer_size() {
            // aim for small, e.g. 256 frames
            let frames = (*min).max(256).min(*max);
            config.buffer_size = BufferSize::Fixed(frames);
        }

        let sample_rate = config.sample_rate as f32;
        let channels = config.channels as usize;

        let mut current_freq = 0.0f32;
        let mut current_gain = 0.0f32;
        let mut phase = 0.0f32;

        let mut chime_phase = 0.0f32;
        let mut chime_env = 0.0f32;
        let chime_freq = 1000.0f32; // 1kHz lock chime

        let err_fn = |err| eprintln!("Audio output error: {err}");

        // Define the sample processing closure
        let state_f32 = state.clone();
        let mut process = move |data: &mut [f32]| {
            let enabled = state_f32.enabled.load(Ordering::Relaxed);
            let master_vol = f32::from_bits(state_f32.master_volume.load(Ordering::Relaxed));
            let target_freq = f32::from_bits(state_f32.target_freq.load(Ordering::Relaxed));
            let target_gain = f32::from_bits(state_f32.target_gain.load(Ordering::Relaxed));

            if state_f32.chime_trigger.swap(false, Ordering::Relaxed) {
                chime_phase = 0.0;
                chime_env = 1.0;
            }

            // Smoothing factor for 20ms transition
            let dt = 1.0 / sample_rate;
            let smooth_factor = (dt * 150.0_f32).clamp(0.0_f32, 1.0_f32); // Simple exponential smoothing

            for frame in data.chunks_mut(channels) {
                current_freq += (target_freq - current_freq) * smooth_factor;
                current_gain += (target_gain - current_gain) * smooth_factor;

                phase = (phase + current_freq * dt) % 1.0;

                let mut out_sample = 0.0;
                if enabled && current_gain > 0.001 {
                    out_sample = (phase * std::f32::consts::TAU).sin() * current_gain;
                }

                if chime_env > 0.001 {
                    chime_phase = (chime_phase + chime_freq * dt) % 1.0;
                    out_sample += (chime_phase * std::f32::consts::TAU).sin() * chime_env * 0.3;
                    chime_env *= 0.999; // Exp decay, adjust as needed
                }

                out_sample *= master_vol;
                let out_sample = out_sample.clamp(-1.0, 1.0);

                for sample in frame.iter_mut() {
                    *sample = out_sample;
                }
            }
        };

        // We only support f32 natively for simplicity; the user guarantees it runs on Windows.
        // If not f32, we do the basic type conversion.
        let stream = match config_format.sample_format() {
            SampleFormat::F32 => device.build_output_stream(
                config,
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| process(data),
                err_fn,
                None,
            ),
            SampleFormat::I16 => {
                // A closure just like process but for i16
                let mut current_freq = 0.0f32;
                let mut current_gain = 0.0f32;
                let mut phase = 0.0f32;
                let mut chime_phase = 0.0f32;
                let mut chime_env = 0.0f32;
                let chime_freq = 1000.0f32;

                device.build_output_stream(
                    config,
                    move |data: &mut [i16], _: &cpal::OutputCallbackInfo| {
                        let enabled = state.enabled.load(Ordering::Relaxed);
                        let master_vol =
                            f32::from_bits(state.master_volume.load(Ordering::Relaxed));
                        let target_freq = f32::from_bits(state.target_freq.load(Ordering::Relaxed));
                        let target_gain = f32::from_bits(state.target_gain.load(Ordering::Relaxed));

                        if state.chime_trigger.swap(false, Ordering::Relaxed) {
                            chime_phase = 0.0;
                            chime_env = 1.0;
                        }

                        let dt = 1.0 / sample_rate;
                        let smooth_factor = (dt * 150.0_f32).clamp(0.0_f32, 1.0_f32);

                        for frame in data.chunks_mut(channels) {
                            current_freq += (target_freq - current_freq) * smooth_factor;
                            current_gain += (target_gain - current_gain) * smooth_factor;

                            phase = (phase + current_freq * dt) % 1.0;

                            let mut out_sample = 0.0;
                            if enabled && current_gain > 0.001 {
                                out_sample = (phase * std::f32::consts::TAU).sin() * current_gain;
                            }

                            if chime_env > 0.001 {
                                chime_phase = (chime_phase + chime_freq * dt) % 1.0;
                                out_sample +=
                                    (chime_phase * std::f32::consts::TAU).sin() * chime_env * 0.3;
                                chime_env *= 0.999;
                            }

                            out_sample *= master_vol;
                            let out_sample = out_sample.clamp(-1.0, 1.0);
                            let i16_sample = (out_sample * 32767.0) as i16;

                            for sample in frame.iter_mut() {
                                *sample = i16_sample;
                            }
                        }
                    },
                    err_fn,
                    None,
                )
            }
            _ => return None,
        }
        .ok()?;

        stream.play().ok()?;
        Some(stream)
    }
}
