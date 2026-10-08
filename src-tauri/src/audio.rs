use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{BufferSize, Stream, StreamConfig};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

/// Lock chime: a short, bright 1 kHz ping.
const CHIME_FREQ_HZ: f32 = 1000.0;
const CHIME_GAIN: f32 = 0.3;
const CHIME_DECAY_TIME_S: f32 = 0.08;
const CHIME_ATTACK_TIME_S: f32 = 0.002;

/// Miss cue: soft and low, a sine gliding down from ~G4 to ~C4, clearly apart from the chime.
const MISS_START_FREQ_HZ: f32 = 392.0;
const MISS_END_FREQ_HZ: f32 = 262.0;
const MISS_GLIDE_TIME_S: f32 = 0.06;
const MISS_GAIN: f32 = 0.2;
const MISS_DECAY_TIME_S: f32 = 0.12;
const MISS_ATTACK_TIME_S: f32 = 0.005;

const DENORMAL_THRESHOLD: f32 = 1e-6;
const SLEW_TIME_S: f32 = 0.0025;
/// Gap between the chime and the miss cue of the debug test sounds.
const TEST_SOUNDS_GAP: Duration = Duration::from_millis(400);
pub const DEFAULT_AUDIO_VOLUME: f32 = 0.2;

/// Contains the lock-free shared state read by the audio callback.
pub struct SharedState {
    pub enabled: AtomicBool,
    pub master_volume: AtomicU32, // f32 bits
    pub chime_trigger: AtomicBool,
    pub miss_trigger: AtomicBool,
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
            chime_trigger: AtomicBool::new(false),
            miss_trigger: AtomicBool::new(false),
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

    pub fn chime(&self) {
        self.chime_trigger.store(true, Ordering::Relaxed);
    }

    pub fn miss(&self) {
        self.miss_trigger.store(true, Ordering::Relaxed);
    }
}

/// Takes a pending trigger (lock-free, as the callback may not block).
#[inline]
fn take_trigger(flag: &AtomicBool) -> bool {
    flag.load(Ordering::Relaxed) && flag.swap(false, Ordering::Relaxed)
}

/// A one-shot sine cue: a short linear attack, then an exponential decay, with the pitch
/// optionally gliding exponentially from `start_freq` to `end_freq`. A retrigger restarts the
/// attack from the current envelope and never resets the phase, so it cannot click.
struct Voice {
    gain: f32,
    start_freq: f32,
    end_freq: f32,
    decay: f32,
    attack_step: f32,
    glide_decay: f32,
    freq: f32,
    phase: f32,
    env: f32,
    attacking: bool,
}

impl Voice {
    fn new(
        dt: f32,
        gain: f32,
        start_freq: f32,
        end_freq: f32,
        glide_time_s: f32,
        attack_time_s: f32,
        decay_time_s: f32,
    ) -> Self {
        Self {
            gain,
            start_freq,
            end_freq,
            decay: (-dt / decay_time_s).exp(),
            attack_step: (dt / attack_time_s).clamp(0.0, 1.0),
            glide_decay: (-dt / glide_time_s).exp(),
            freq: start_freq,
            phase: 0.0,
            env: 0.0,
            attacking: false,
        }
    }

    /// The next sample. A disabled synth ignores (but consumes) the trigger and fades the cue
    /// out at the slew rate instead of cutting it.
    fn next(&mut self, dt: f32, triggered: bool, enabled: bool, smooth_factor: f32) -> f32 {
        if !self.phase.is_finite() {
            self.phase = 0.0;
        }
        if !self.env.is_finite() {
            self.env = 0.0;
        }
        if !self.freq.is_finite() {
            self.freq = self.start_freq;
        }

        if !enabled {
            self.attacking = false;
        } else if triggered {
            self.attacking = true;
            self.freq = self.start_freq;
        }

        if self.attacking {
            self.env += self.attack_step;
            if self.env >= 1.0 {
                self.env = 1.0;
                self.attacking = false;
            }
        } else if enabled {
            self.env *= self.decay;
        } else {
            self.env -= self.env * smooth_factor;
        }
        if !self.attacking && self.env.abs() < DENORMAL_THRESHOLD {
            self.env = 0.0;
        }

        if self.env <= 0.0 {
            return 0.0;
        }
        self.freq = self.end_freq + (self.freq - self.end_freq) * self.glide_decay;
        self.phase = (self.phase + self.freq * dt) % 1.0;
        (self.phase * std::f32::consts::TAU).sin() * self.env * self.gain
    }
}

pub struct Synth {
    dt: f32,
    smooth_factor: f32,
    current_vol: f32,
    chime: Voice,
    miss: Voice,
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
            dt,
            smooth_factor,
            current_vol: 0.0,
            chime: Voice::new(
                dt,
                CHIME_GAIN,
                CHIME_FREQ_HZ,
                CHIME_FREQ_HZ,
                1.0,
                CHIME_ATTACK_TIME_S,
                CHIME_DECAY_TIME_S,
            ),
            miss: Voice::new(
                dt,
                MISS_GAIN,
                MISS_START_FREQ_HZ,
                MISS_END_FREQ_HZ,
                MISS_GLIDE_TIME_S,
                MISS_ATTACK_TIME_S,
                MISS_DECAY_TIME_S,
            ),
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

        self.current_vol += (master_vol - self.current_vol) * self.smooth_factor;
        if master_vol == 0.0 && self.current_vol.abs() < DENORMAL_THRESHOLD {
            self.current_vol = 0.0;
        }

        let chime_triggered = take_trigger(&shared.chime_trigger);
        let miss_triggered = take_trigger(&shared.miss_trigger);
        let chime = self
            .chime
            .next(self.dt, chime_triggered, enabled, self.smooth_factor);
        let miss = self
            .miss
            .next(self.dt, miss_triggered, enabled, self.smooth_factor);

        let out_sample = (chime + miss) * self.current_vol;
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
    pub fn chime_env(&self) -> f32 {
        self.chime.env
    }

    #[must_use]
    pub fn miss_env(&self) -> f32 {
        self.miss.env
    }

    #[must_use]
    pub fn miss_freq(&self) -> f32 {
        self.miss.freq
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

    /// Whether a chime was requested since the last call.
    #[cfg(test)]
    pub(crate) fn take_chime(&self) -> bool {
        self.state.chime_trigger.swap(false, Ordering::Relaxed)
    }

    /// Whether a miss cue was requested since the last call.
    #[cfg(test)]
    pub(crate) fn take_miss(&self) -> bool {
        self.state.miss_trigger.swap(false, Ordering::Relaxed)
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.state.set_enabled(enabled);
    }

    pub fn set_volume(&self, volume: f32) {
        self.state.set_volume(volume);
    }

    pub fn chime(&self) {
        self.state.chime();
    }

    pub fn miss(&self) {
        self.state.miss();
    }

    /// Plays the chime now and the miss cue shortly after. A newer call cancels the pending
    /// miss cue of an older one, so rapid clicks do not stack.
    #[cfg_attr(
        not(debug_assertions),
        expect(dead_code, reason = "test sounds are debug-only")
    )]
    pub fn test_tone(&self) {
        self.chime();
        {
            let generation = self.test_tone_generation.fetch_add(1, Ordering::Relaxed) + 1;
            let state = self.state.clone();
            let latest = self.test_tone_generation.clone();
            std::thread::spawn(move || {
                std::thread::sleep(TEST_SOUNDS_GAP);
                if latest.load(Ordering::Relaxed) == generation {
                    state.miss();
                }
            });
        }
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
    fn silent_without_a_cue() {
        let mut synth = Synth::new(48_000.0);
        let shared = SharedState::new();
        shared.set_volume(1.0);
        for _ in 0..2000 {
            assert_eq!(synth.next_sample(&shared), 0.0);
        }
    }

    #[test]
    fn silent_when_disabled() {
        let mut synth = Synth::new(48000.0);
        let shared = SharedState::new();
        shared.set_enabled(false);
        shared.set_volume(1.0);
        shared.chime();
        shared.miss();

        for _ in 0..1000 {
            let sample = synth.next_sample(&shared);
            assert_eq!(sample, 0.0);
        }
        // The triggers were consumed, so enabling does not play them late.
        assert!(!shared.chime_trigger.load(Ordering::Relaxed));
        assert!(!shared.miss_trigger.load(Ordering::Relaxed));
    }

    #[test]
    fn no_nan_after_nan_input() {
        let mut synth = Synth::new(48000.0);
        let shared = SharedState::new();

        shared
            .master_volume
            .store(f32::NAN.to_bits(), Ordering::Relaxed);
        shared.chime();
        shared.miss();
        for _ in 0..100 {
            let sample = synth.next_sample(&shared);
            assert!(sample.is_finite());
            assert_eq!(sample, 0.0);
        }

        // Now resume normal inputs and verify synth recovers
        shared.set_volume(0.5);
        shared.miss();
        let mut non_zero_seen = false;
        for _ in 0..1000 {
            let sample = synth.next_sample(&shared);
            assert!(sample.is_finite());
            if sample.abs() > 0.01 {
                non_zero_seen = true;
            }
        }
        assert!(non_zero_seen);
    }

    #[test]
    fn corrupted_voice_state_recovers() {
        let mut synth = Synth::new(48_000.0);
        let shared = SharedState::new();
        shared.set_volume(1.0);
        synth.miss.phase = f32::NAN;
        synth.miss.env = f32::NAN;
        synth.miss.freq = f32::NAN;
        synth.current_vol = f32::NAN;
        for _ in 0..100 {
            assert!(synth.next_sample(&shared).is_finite());
        }
        shared.miss();
        for _ in 0..1000 {
            assert!(synth.next_sample(&shared).is_finite());
        }
        assert!(synth.miss_env() > 0.0);
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
    fn miss_decays_to_zero_at_48khz_and_96khz() {
        for rate in [48_000.0_f32, 96_000.0] {
            let mut synth = Synth::new(rate);
            let shared = SharedState::new();
            shared.set_volume(1.0);
            shared.miss();
            // 5 ms attack, then 0.12 s of decay: about 1/e of the peak.
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "small positive sample counts"
            )]
            let (attack, decay_tau, total) = (
                (rate * MISS_ATTACK_TIME_S) as usize,
                (rate * MISS_DECAY_TIME_S) as usize,
                (rate * 2.0) as usize,
            );
            for _ in 0..attack + decay_tau {
                synth.next_sample(&shared);
            }
            assert!(
                (synth.miss_env() - (-1.0_f32).exp()).abs() < 0.01,
                "env at {rate} Hz: {}",
                synth.miss_env()
            );
            for _ in 0..total {
                synth.next_sample(&shared);
            }
            assert_eq!(synth.miss_env(), 0.0, "at {rate} Hz");
        }
    }

    #[test]
    fn miss_pitch_glides_down() {
        let mut synth = Synth::new(48_000.0);
        let shared = SharedState::new();
        shared.set_volume(1.0);
        shared.miss();
        synth.next_sample(&shared);
        assert!(synth.miss_freq() <= MISS_START_FREQ_HZ);
        assert!(synth.miss_freq() > 380.0);
        for _ in 0..48_000 / 2 {
            synth.next_sample(&shared);
        }
        assert!((synth.miss_freq() - MISS_END_FREQ_HZ).abs() < 1.0);
        // A retrigger restarts the glide from the top.
        shared.miss();
        synth.next_sample(&shared);
        assert!(synth.miss_freq() > 380.0);
    }

    /// Both cues respond within ~20 ms of the trigger (SCT-038), at 48 kHz and 96 kHz.
    #[test]
    fn cues_respond_within_20ms() {
        for rate in [48_000.0_f32, 96_000.0] {
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "20 ms of samples is a small positive count"
            )]
            let samples = (rate * 0.020) as usize;
            for miss in [false, true] {
                let mut synth = Synth::new(rate);
                let shared = SharedState::new();
                shared.set_volume(1.0);
                if miss {
                    shared.miss();
                } else {
                    shared.chime();
                }
                let mut peak = 0.0_f32;
                for _ in 0..samples {
                    peak = peak.max(synth.next_sample(&shared).abs());
                }
                let env = if miss {
                    synth.miss_env()
                } else {
                    synth.chime_env()
                };
                assert!(env > 0.5, "env (miss: {miss}) at {rate} Hz: {env}");
                assert!(peak > 0.05, "peak (miss: {miss}) at {rate} Hz: {peak}");
            }
        }
    }

    /// The miss cue peaks below the chime and below the headroom of the master volume.
    #[test]
    fn miss_is_softer_than_the_chime() {
        const { assert!(MISS_GAIN < CHIME_GAIN) };
        const { assert!(MISS_START_FREQ_HZ < CHIME_FREQ_HZ) };
        let peak = |miss: bool| {
            let mut synth = Synth::new(48_000.0);
            let shared = SharedState::new();
            shared.set_volume(1.0);
            if miss {
                shared.miss();
            } else {
                shared.chime();
            }
            (0..4800)
                .map(|_| synth.next_sample(&shared).abs())
                .fold(0.0, f32::max)
        };
        assert!(peak(true) < peak(false));
    }

    /// Largest per-sample jump allowed at 48 kHz. A 0.3-amplitude 1 kHz chime has a natural
    /// slope of about 0.039 per sample; the 0.2-amplitude miss cue about 0.01, so the two
    /// overlapping stay below 0.05.
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

    fn cue(shared: &SharedState, miss: bool) {
        if miss {
            shared.miss();
        } else {
            shared.chime();
        }
    }

    fn env(synth: &Synth, miss: bool) -> f32 {
        if miss {
            synth.miss_env()
        } else {
            synth.chime_env()
        }
    }

    #[test]
    fn enable_disable_has_no_clicks() {
        for miss in [false, true] {
            let mut synth = Synth::new(48_000.0);
            let shared = SharedState::new();
            shared.set_volume(1.0);
            let mut meter = StepMeter::new();
            cue(&shared, miss);
            meter.run(&mut synth, &shared, 1000);
            shared.set_enabled(false);
            meter.run(&mut synth, &shared, 6000);
            shared.set_enabled(true);
            cue(&shared, miss);
            meter.run(&mut synth, &shared, 6000);
            meter.assert_smooth("enable/disable");
        }
    }

    #[test]
    fn volume_steps_have_no_clicks() {
        for miss in [false, true] {
            let mut synth = Synth::new(48_000.0);
            let shared = SharedState::new();
            shared.set_volume(0.0);
            let mut meter = StepMeter::new();
            cue(&shared, miss);
            meter.run(&mut synth, &shared, 500);
            shared.set_volume(1.0);
            meter.run(&mut synth, &shared, 2000);
            shared.set_volume(0.0);
            meter.run(&mut synth, &shared, 6000);
            shared.set_volume(1.0);
            cue(&shared, miss);
            meter.run(&mut synth, &shared, 6000);
            meter.assert_smooth("volume step");
        }
    }

    #[test]
    fn mute_during_a_cue_fades_out() {
        for miss in [false, true] {
            let mut synth = Synth::new(48_000.0);
            let shared = SharedState::new();
            shared.set_volume(1.0);
            let mut meter = StepMeter::new();
            meter.run(&mut synth, &shared, 3000);
            cue(&shared, miss);
            meter.run(&mut synth, &shared, 300);
            assert!(env(&synth, miss) > 0.5);
            shared.set_enabled(false);
            meter.run(&mut synth, &shared, 1);
            assert!(env(&synth, miss) > 0.0, "cue must fade, not cut");
            meter.run(&mut synth, &shared, 8000);
            meter.assert_smooth("mute during cue");
            assert_eq!(env(&synth, miss), 0.0);
        }
    }

    #[test]
    fn cue_retrigger_has_no_clicks() {
        for miss in [false, true] {
            let mut synth = Synth::new(48_000.0);
            let shared = SharedState::new();
            shared.set_volume(1.0);
            let mut meter = StepMeter::new();
            meter.run(&mut synth, &shared, 3000);
            cue(&shared, miss);
            meter.run(&mut synth, &shared, 1500);
            let before = env(&synth, miss);
            assert!(before < 0.9 && before > 0.1, "env {before}");
            cue(&shared, miss);
            meter.run(&mut synth, &shared, 300);
            assert!(
                env(&synth, miss) > before,
                "retrigger must raise the envelope"
            );
            meter.run(&mut synth, &shared, 6000);
            meter.assert_smooth("cue retrigger");
        }
    }

    #[test]
    fn miss_overlapping_chime_has_no_clicks() {
        let mut synth = Synth::new(48_000.0);
        let shared = SharedState::new();
        shared.set_volume(1.0);
        let mut meter = StepMeter::new();
        meter.run(&mut synth, &shared, 1000);
        shared.chime();
        meter.run(&mut synth, &shared, 200);
        shared.miss();
        meter.run(&mut synth, &shared, 8000);
        assert!(synth.chime_env() < 0.5 && synth.miss_env() > 0.0);
        meter.assert_smooth("miss over chime");
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
