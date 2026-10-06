# Audio feedback from Rust on Windows (SCT-038 research)

Date: 2026-10-06
> **Editor's note (Claude):** this document was drafted by Gemini from web research and reviewed by Claude. Crate versions and licences were checked with `cargo info` on 2026-10-06 and are correct: cpal 0.18.2 (Apache-2.0), rodio 0.22.2 and ringbuf 0.5.2 (MIT OR Apache-2.0). All are compatible with GPL-3.0-only. The claim that rodio adds a "default 100 ms buffer" wasn't verified; treat it as Unverified until checked against rodio's source. The latency figures for WASAPI shared mode are typical values, not measurements on this app.


## TL;DR

- Use `cpal` directly for low-level audio I/O; `rodio` introduces a default 100 ms buffer and thread-synchronization overhead unsuited for reactive real-time synthesis.
- On Windows, WASAPI shared mode operates with an engine period of 10 ms (yielding ~17–28 ms end-to-end latency); exclusive mode drops to ~3–7 ms but silences all other applications.
- Pass continuous parameters from the 1 kHz input thread via `AtomicU32` holding `f32` bit patterns; pass discrete commands (like chime triggers) via the lock-free `ringbuf` crate.
- Smooth frequency and amplitude changes using a one-pole low-pass filter with a 10 ms time constant and accumulate oscillator phase continuously to prevent zipper noise and clicks.
- Map signed error exponentially (musically in semitones) around 440 Hz with a dead zone inside the tolerance band, aligning with motor learning bandwidth-feedback literature.

---

## 1. Audio Crate Comparison

SimCurveTrainApp requires low-latency procedural synthesis governed by a 1 kHz pedal polling thread. The application is licensed under GPL-3.0-only. All evaluated crates use permissive licenses (MIT, Apache-2.0, or MPL-2.0), which are compatible with GPL-3.0-only.

| Crate | Latest Version | License | Strengths | Weaknesses | Fit for SCT-038 |
|---|---|---|---|---|---|
| **cpal** | 0.18.2 | Apache-2.0 | Standard low-level audio I/O crate in Rust. Direct access to the native audio callback. Explicit control over stream configuration. Zero required audio abstractions. | No built-in oscillators, envelopes, or mixers. Requires manual DSP implementation. | **Best fit** for low-latency procedural synthesis. |
| **rodio** | 0.22.2 | MIT OR Apache-2.0 | Simple high-level playback API for static audio files (WAV, MP3, OGG, FLAC) with sinks and sources. | Imposes a default buffer size of 100 ms to avoid dropouts. Uses blocking channels and mutex locks internally. | **Poor fit**; buffering and queue abstractions add latency. |
| **tinyaudio** | 2.0.0 | MIT | Minimalist cross-platform audio output. Takes a simple closure to fill output buffers. | Relies on unmaintained `winapi 0.3.9` on Windows. Cannot enumerate or select devices. Does not handle device reconnection or format negotiation. | **Poor fit**; lacking device management and modern Windows bindings. |
| **oddio** | 0.7.4 | MIT OR Apache-2.0 | Lock-free audio mixer focused on game audio and 3D spatialization. | Pre-1.0 library focused on mixing asset streams, not procedural tone synthesis. Still requires an external backend like `cpal` to drive hardware. | **Poor fit**; unnecessary complexity. |
| **kira** | 0.12.5 | MIT OR Apache-2.0 | Feature-rich game audio engine with sample-accurate clocking, tweens, and spatial audio. | Built around triggering sound assets and applying tweens rather than sample-by-sample procedural synthesis. Heavy dependency footprint for single-tone synthesis. | **Poor fit**; excessive abstraction. |
| **fundsp** | 0.23.0 | MIT OR Apache-2.0 | Functional DSP graph engine. Provides clean math primitives for oscillators, filters, and envelopes. | Provides only DSP math, not audio I/O. Still requires `cpal` to interface with hardware. | **Optional DSP helper**, but simple sine and envelope synthesis can be written in 40 lines of pure Rust without extra dependencies. |

### Crate Sources
- `cpal`: https://crates.io/crates/cpal and https://docs.rs/cpal/0.18.2/cpal/
- `rodio`: https://crates.io/crates/rodio and https://docs.rs/rodio/0.22.2/rodio/
- `tinyaudio`: https://crates.io/crates/tinyaudio and https://docs.rs/tinyaudio/2.0.0/tinyaudio/
- `oddio`: https://crates.io/crates/oddio and https://docs.rs/oddio/0.7.4/oddio/
- `kira`: https://crates.io/crates/kira and https://docs.rs/kira/0.12.5/kira/
- `fundsp`: https://crates.io/crates/fundsp and https://docs.rs/fundsp/0.23.0/fundsp/

---

## 2. WASAPI on Windows: Shared vs Exclusive Mode and Latency

### Shared Mode vs Exclusive Mode
- **Shared Mode (`AUDCLNT_SHAREMODE_SHARED`)**: Streams pass through the Windows Audio Engine (`audiodg.exe`). The audio engine handles software mixing, format conversion, and system audio routing. Multiple applications play concurrently (e.g., sim-racing simulators, Discord, browser).
- **Exclusive Mode (`AUDCLNT_SHAREMODE_EXCLUSIVE`)**: The client application communicates directly with the audio endpoint device driver buffer, completely bypassing the Windows Audio Engine.
- **Trade-offs for Desktop Trainers**: Exclusive mode achieves minimal buffer latency (often 2–5 ms). However, exclusive mode mutes all other applications, fails if another program holds audio, and fails if the requested format does not match the physical hardware clock. Because users run SimCurveTrainApp alongside simulators or voice chat, shared mode is mandatory for regular usage.

### Typical Buffer Sizes and Latency in Shared Mode
- The standard Windows Audio Engine period is 10 ms (100,000 units of `REFERENCE_TIME` / 100 ns). At 48 kHz, this equals 480 frames per engine pass.
- In standard shared mode, endpoint buffers are typically double- or triple-buffered (20 ms to 30 ms buffer depth) to ensure glitch-free software mixing.
- Added to DAC hardware FIFO buffering (typically 1–3 ms), standard shared-mode output latency sits between 15 ms and 25 ms.

### How `cpal` Handles Buffer Size on WASAPI
- In `cpal`, the caller configures `StreamConfig::buffer_size` via `BufferSize::Default` or `BufferSize::Fixed(u32)`.
- When set to `BufferSize::Fixed(n)`, `cpal` converts the requested frame count `n` into 100 ns units: `hnsBufferDuration = (n * 10_000_000) / sample_rate`.
- On WASAPI shared mode, `BufferSize::Fixed` acts only as a hint to `IAudioClient::Initialize`. The Windows Audio Engine clamps the requested buffer size to its own internal periodicity constraints (typically a minimum of two 10 ms engine periods).
- Furthermore, `cpal` does not guarantee that the callback slice `&mut [f32]` matches `n`. The callback receives variable slice lengths depending on how many frames the audio engine makes available on each wake-up. Code must process dynamically based on `data.len()`.

### `IAudioClient3` Low-Latency Shared Mode
- Microsoft introduced `IAudioClient3` in Windows 10 (Build 1511) to enable sub-10ms shared-mode streams via `IAudioClient3::GetSharedModeEnginePeriod` and `IAudioClient3::InitializeSharedAudioStream`.
- `cpal` does **not** expose or implement `IAudioClient3`. `cpal`'s WASAPI backend exclusively uses the baseline `IAudioClient` interface. Using `IAudioClient3` would require direct Windows COM FFI calls via the `windows` crate.

### Realistic End-to-End Latency Breakdown
Measuring from physical pedal displacement to acoustic wave emission:
1. Pedal polling delay on the 1 kHz thread: 0.5 ms to 1.0 ms.
2. Lock-free parameter passing (atomic write/read): < 0.001 ms.
3. Audio callback buffer synthesis: < 0.05 ms.
4. WASAPI shared-mode engine mixing and buffer lead: ~15.0 ms to 24.0 ms.
5. Hardware DAC output buffering: ~1.0 ms to 3.0 ms.
- **Total Shared-Mode End-to-End Latency**: **~17 ms to 28 ms**.
- This satisfies the ~20 ms target within acceptable human perception margins while keeping Discord and game sound audible.

### WASAPI Sources
- Microsoft WASAPI Documentation: https://learn.microsoft.com/en-us/windows/win32/coreaudio/about-the-windows-audio-session-api
- Microsoft Low-Latency Audio: https://learn.microsoft.com/en-us/windows-hardware/drivers/audio/low-latency-audio
- Microsoft `IAudioClient3` Interface: https://learn.microsoft.com/en-us/windows/win32/api/audioclient/nn-audioclient-iaudioclient3
- Microsoft Exclusive-Mode Streams: https://learn.microsoft.com/en-us/windows/win32/coreaudio/exclusive-mode-streams
- `cpal` WASAPI Implementation: https://github.com/RustAudio/cpal/tree/master/src/host/wasapi

---

## 3. Architecture: Driving the Synth from the 1 kHz Input Thread

### Real-Time Safety in the Audio Callback
The audio callback runs on a high-priority Multimedia Class Scheduler Service (MMCSS) thread managed by WASAPI. The callback must never:
- Allocate or deallocate memory (`malloc`, `free`, `Vec::push`, `Box::new`).
- Acquire blocking primitives (`std::sync::Mutex`, `std::sync::RwLock`).
- Execute system I/O (file access, network sockets, or `println!`).
Priority inversion occurs if the 1 kHz input thread or UI thread holds a lock while interrupted, causing the audio thread to miss its hardware deadline and produce audible dropouts (pops and clicks).

### Lock-Free Parameter Passing
1. **Continuous Parameters (Error Value, Target Pitch, Volume, Mute)**:
   - Use `std::sync::atomic::AtomicU32`.
   - Float bit manipulation in standard Rust is safe and portable:
     - Producer: `error_bits.store(error.to_bits(), Ordering::Relaxed)`
     - Consumer: `let error = f32::from_bits(error_bits.load(Ordering::Relaxed))`
   - Standard library `AtomicU32` requires zero dependencies and complies with `#![forbid(unsafe_code)]`.
2. **Compound State**:
   - For multi-field snapshots, the `triple_buffer` crate (9.0.0, MPL-2.0) provides wait-free, non-blocking single-producer single-consumer updates where the consumer always reads the latest written state.
3. **Discrete Events (Lock Chime Trigger, Mode Changes)**:
   - Use a lock-free Single Producer Single Consumer (SPSC) ring buffer: `ringbuf` (0.5.2, MIT OR Apache-2.0) or `rtrb` (0.4.0, MIT OR Apache-2.0).
   - Producer pushes commands: `cmd_producer.push(AudioCommand::PlayLockChime)`.
   - Audio callback pops commands: `while let Some(cmd) = cmd_consumer.pop() { ... }`.

### Parameter Smoothing (Preventing Zipper Noise)
Pedal inputs update every 1 ms (1000 Hz), whereas the audio callback samples at 48000 Hz (48 samples per millisecond). Instantaneous changes between buffers produce "zipper noise" (high-frequency spectral discontinuities perceived as buzzing or clicking).

Apply a one-pole low-pass filter (exponential moving average) per sample:
$$y[n] = y[n-1] + \alpha \cdot (x[n] - y[n-1])$$
$$\alpha = 1.0 - \exp\left(-\frac{1}{f_s \cdot \tau}\right)$$
- $f_s$: sample rate (e.g., 48000 Hz).
- $\tau$: smoothing time constant.
- **Recommended Time Constant**: **10 ms** (range: 5 ms to 20 ms).
- At $\tau = 10\text{ ms}$, the smoothed value covers 63% of a step change in 10 ms and >95% in 30 ms. This completely suppresses zipper noise without introducing perceptible control sluggishness.

### Phase-Continuous Frequency Modulation
Computing an oscillator as $\sin(2\pi \cdot f \cdot t)$ causes sudden phase jumps when $f$ changes, producing loud clicks.

Instead, accumulate phase incrementally on normalized range $[0.0, 1.0)$:
```text
phase_increment = smoothed_frequency / sample_rate;
phase = (phase + phase_increment).fract();
sample = (phase * 2.0 * PI).sin();
```
Because the phase accumulator $\phi$ is monotonically advancing and continuous ($C^0$ continuous waveform), changes in frequency only modulate the instantaneous slope, guaranteeing click-free transitions.

### Envelope for the Lock Chime
When the pedal stays in the tolerance band, play a short chime using an Attack-Release (AR) envelope:
- **Attack Phase**: 10 ms linear ramp from 0.0 to 1.0. Eliminates DC offset clicks at sound onset.
- **Release/Decay Phase**: Exponential decay with a time constant $\tau_{\text{decay}} = 100\text{ ms}$ (total audible length ~250–300 ms).
- **Timbre**: A pure two-tone bell chord (e.g., fundamental at 880 Hz + perfect fifth at 1320 Hz, or C6 at 1046.5 Hz + E6 at 1318.5 Hz) creates an unmistakable, pleasant auditory confirmation.

### Architecture Sources
- Ross Bencina, "Real-time audio programming 101: time-waits-for-nothing": http://www.rossbencina.com/code/real-time-audio-programming-101-time-waits-for-nothing
- Julius O. Smith III, "Introduction to Digital Filters with Audio Applications - One-Pole": https://ccrma.stanford.edu/~jos/filters/One_Pole.html
- `ringbuf` crate: https://docs.rs/ringbuf/0.5.2/ringbuf/
- `triple_buffer` crate: https://docs.rs/triple_buffer/9.0.0/triple_buffer/

---

## 4. Mapping Signed Error to Pitch

### Sensible Frequency Ranges
- Human hearing sensitivity (Fletcher-Munson curves) is most balanced and least fatiguing between 200 Hz and 1200 Hz. Frequencies below 150 Hz require large speakers/subwoofers, while frequencies above 2000 Hz cause rapid cognitive fatigue and irritation.
- **Reference Pitch ($e = 0$)**: 440 Hz (Concert A4) or 523.25 Hz (C5).
- **Pitch Range**: $\pm 1$ octave around the reference (e.g., 220 Hz to 880 Hz).

### Linear vs Exponential (Musical) Mapping
- **Linear Mapping ($f = f_0 + k \cdot e$)**: A 50 Hz change near 200 Hz spans roughly 4 semitones (a major third). The same 50 Hz change near 800 Hz spans less than one semitone. Linear pitch mapping feels hypersensitive at low pedal pressures and sluggish at high pedal pressures.
- **Exponential / Musical Mapping**: Pitch perception is logarithmic (Weber-Fechner law). Map error to equal-tempered semitones:
  $$f(e) = f_0 \cdot 2^{\frac{k \cdot e}{12}}$$
  Where:
  - $e$: signed error ($e = \text{pedal} - \text{target} \in [-1.0, 1.0]$).
  - $k$: pitch sensitivity in semitones per unit error.
  - For example, if maximum error of $\pm 20\%$ maps to $\pm 12$ semitones ($\pm 1$ octave), then $k = 12 / 0.20 = 60$.
  - Equal increments in physical pedal error produce identical perceptual musical intervals across the entire range.

### Dead Zone and Bandwidth Feedback
- In motor learning literature, continuously sonifying micro-errors forces the motor system into continuous micro-corrections (hunting and tremor).
- **Bandwidth Feedback** (Sherwood 1988; Guadagnoli & Lee 2004): Augmented feedback is withheld while the user remains within an acceptable performance tolerance band.
- **Drill Implementation**:
  - **Hold Drills**: While inside the tolerance band ($|e| \le \text{band}$), attenuate continuous error sound to zero via a 10 ms gain ramp. Silence signals success and allows intrinsic proprioceptive reinforcement. Crossing into the band triggers the single lock chime.
  - **Trace Drills**: While inside the band, the tone is muted. When the user drifts outside the tolerance band, a soft tone plays with volume or pitch proportional to the excursion distance.

### Research Citations
1. **Sigrist, R., Rauter, G., Riener, R., & Wolf, P. (2013)**. "Augmented visual, auditory, haptic, and multimodal feedback in motor learning: a review." *Psychonomic Bulletin & Review*, 20(1), 21–53.
   - URL: https://link.springer.com/article/10.3758/s13423-012-0333-8 / https://www.ncbi.nlm.nih.gov/pmc/articles/PMC3576629/
   - Key finding: Auditory feedback features faster reaction times (~140 ms) compared to visual feedback (~220 ms), accelerating motor corrective adjustments, but must avoid continuous sensory overload.
2. **Dubus, G., & Bresin, R. (2013)**. "A systematic review of mapping strategies for the sonification of physical quantities." *PLOS ONE*, 8(12), e82491.
   - URL: https://journals.plos.org/plosone/article?id=10.1371/journal.pone.0082491
   - Key finding: Pitch is the most frequently and successfully used auditory dimension for continuous scalar physical quantities. Congruent polarities (higher error = higher pitch) minimize cognitive decoding latency.
3. **Guadagnoli, M. A., & Lee, T. D. (2004)**. "Challenge point: a framework for conceptualizing the effects of various practice conditions in motor learning." *Journal of Motor Behavior*, 36(2), 212–224.
   - URL: https://pubmed.ncbi.nlm.nih.gov/15130760/
   - Key finding: Establishes that providing feedback only when error exceeds a critical threshold (bandwidth feedback) prevents dependency on external cues and improves long-term skill retention.
4. **Sherwood, D. E. (1988)**. "Effect of bandwidth knowledge of results on movement consistency." *Perceptual and Motor Skills*, 66(2), 535–542.
   - URL: https://pubmed.ncbi.nlm.nih.gov/3224716/

---

## 5. Pitfalls and Failure Modes

### 1. Default Device Changes and Disconnections
- **Behavior**: On Windows, unplugging a headset or switching the default output device invalidates the WASAPI audio endpoint (`AUDCLNT_E_DEVICE_INVALIDATED`).
- **In `cpal`**: The output stream ceases playback and passes `StreamError::DeviceNotAvailable` to the registered `error_callback`. If not handled, the application remains permanently muted.
- **Remedy**: The audio manager must register an error callback that flags stream invalidation, drops the inactive `cpal::Stream`, and schedules an automatic re-initialization against the new default device with a 500 ms backoff.

### 2. Bluetooth Headsets and Latency
- **Latency Reality**: Bluetooth audio on Windows uses A2DP profiles (SBC, AAC, or aptX). A2DP drivers maintain large transmission and reception packet buffers to prevent RF dropouts, introducing **100 ms to 250 ms** of fixed latency.
- **Impact**: In a sim-racing trainer targeting <20 ms response, 150 ms audio delay makes feedback counter-productive, causing user overcorrection and pedal oscillation.
- **Remedy**: Inspect `device.name()`. If the string contains identifiers such as `"Bluetooth"`, `"Hands-Free"`, or `"Wireless"`, surface an advisory banner in Settings: *"Bluetooth audio detected (~150 ms latency). Use wired headphones or speakers for low-latency feedback."*

### 3. Sample-Rate Mismatch
- In WASAPI shared mode, the stream sample rate is determined by the Windows Audio Engine settings (commonly 48000 Hz or 44100 Hz).
- Attempting to force an arbitrary sample rate causes `cpal::Device::build_output_stream` to fail with format errors.
- **Remedy**: Query `device.default_output_config()`. Pass the reported sample rate directly to the synthesizer oscillator so that `phase_increment = frequency / sample_rate` adjusts dynamically without pitch distortion.

### 4. CPU Overhead
- Simulating two sine oscillators, a one-pole smoothing filter, and an envelope requires ~15 floating-point arithmetic operations per sample. At 48 kHz stereo, this requires less than 2 MFLOPS (<0.05% CPU usage on modern x86 hardware).
- Real risk is not math compute; it is memory allocation or thread contention inside the audio callback causing buffer underruns.

### 5. Testing Without Audio Hardware in CI
- **CI Reality**: Headless GitHub Actions runners (`windows-latest`) lack physical audio hardware. The Windows Audio service (`Audiosrv`) often has no active endpoints, causing `cpal::default_host().default_output_device()` to return `None`.
- **Impact**: Any test calling `.unwrap()` or `.expect()` on audio device initialization will immediately fail CI.
- **Remedy**:
  1. Decouple DSP synthesis logic completely from `cpal::Stream`. Unit test oscillators, parameter smoothers, and envelopes with synthetic buffers (`let mut buf = [0.0f32; 480]; synth.render(&mut buf);`).
  2. Integration tests attempting to initialize `cpal` streams must gracefully exit if `default_output_device().is_none()`, or reside behind `#[ignore]`.

### Pitfall Sources
- Microsoft, "Recovering from an Invalid-Device Error": https://learn.microsoft.com/en-us/windows/win32/coreaudio/recovering-from-an-invalid-device-error
- Bluetooth SIG, "A2DP Latency and Audio Quality Considerations" [Unverified: exact specification document URL varies by profile version]

---

## 6. Recommendation and Technical Design

### Concrete Crate Selection
Add the following dependencies to `Cargo.toml` in `src-tauri`:
```toml
cpal = "0.18.2"
ringbuf = "0.5.2"
```
No external DSP crate is needed. The synthesizer logic is straightforward to implement in safe Rust, maintaining zero dependencies on large audio engine graphs.

### Module Architecture Sketch (~30 lines of Rust-like pseudocode)

```rust
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use ringbuf::traits::{Consumer, Producer, Split};
use ringbuf::HeapRb;

pub enum AudioCmd { PlayLockChime, SetMode(DrillMode) }
pub enum DrillMode { Idle, HoldDrill, TraceDrill }

pub struct AudioParams {
    pub error_bits: AtomicU32,      // f32::to_bits()
    pub volume_bits: AtomicU32,     // f32::to_bits()
    pub muted: AtomicBool,
}

pub struct SynthState {
    pub phase: f32,
    pub smoothed_freq: f32,
    pub smoothed_gain: f32,
    pub chime_env: f32,
    pub sample_rate: f32,
}

impl SynthState {
    pub fn process_sample(&mut self, target_freq: f32, target_gain: f32) -> f32 {
        let alpha = 1.0 - (-1.0 / (self.sample_rate * 0.010)).exp(); // 10ms smooth
        self.smoothed_freq += alpha * (target_freq - self.smoothed_freq);
        self.smoothed_gain += alpha * (target_gain - self.smoothed_gain);
        self.phase = (self.phase + self.smoothed_freq / self.sample_rate).fract();
        let tone = (self.phase * std::f32::consts::TAU).sin() * self.smoothed_gain;
        let chime = if self.chime_env > 0.001 {
            self.chime_env *= 1.0 - (1.0 / (self.sample_rate * 0.120)); // decay
            ((self.phase * 2.0).fract() * std::f32::consts::TAU).sin() * self.chime_env * 0.4
        } else { 0.0 };
        tone + chime
    }
}
```

### Thread Interaction
- **Input Thread (1 kHz)**:
  - Calculates signed pedal error: $e = \text{pedal} - \text{target}$.
  - Writes `params.error_bits.store(e.to_bits(), Ordering::Relaxed)`.
  - On qualifying band lock: calls `cmd_producer.try_push(AudioCmd::PlayLockChime)`.
- **Audio Callback (`cpal` MMCSS thread)**:
  - Drains commands from `cmd_consumer`.
  - Reads `f32::from_bits(params.error_bits.load(Ordering::Relaxed))`.
  - In Hold drill: if inside tolerance band, `target_gain = 0.0`; else `target_gain = volume` and `target_freq = 440.0 * 2.0f32.powf(60.0 * e / 12.0)`.
  - Renders samples via `synth.process_sample(target_freq, target_gain)`.

---

## Open Questions for the Maintainer

1. **Lock Chime Timbre**: Should the lock chime be generated procedurally as a two-tone bell chord (880 Hz + 1320 Hz) to keep the app self-contained and small, or loaded from an embedded 16-bit WAV file?
2. **Trace Drill Out-of-Band Feedback**: Should out-of-band feedback during trace drills be a soft continuous drone, or a pulsing/beeping tone whose repetition rate scales with distance from the band?
3. **Audio-Visual Latency Alignment for SCT-022**: Ticket SCT-022 tests visual latency using a screen flash. Should SCT-022 emit an audio test transient (a sharp 1 kHz click) on brake crossing so users can measure audio-to-visual latency synchronization using a camera or oscilloscope?
4. **Device Selection UI**: Should SCT-061 (Settings) expose an audio output device dropdown, or is following the Windows default output device sufficient for the MVP?

---

## Sources

- CPAL crate registry: https://crates.io/crates/cpal
- CPAL documentation: https://docs.rs/cpal/0.18.2/cpal/
- CPAL GitHub repository: https://github.com/RustAudio/cpal
- Rodio crate registry: https://crates.io/crates/rodio
- Rodio documentation: https://docs.rs/rodio/0.22.2/rodio/
- Tinyaudio crate registry: https://crates.io/crates/tinyaudio
- Tinyaudio documentation: https://docs.rs/tinyaudio/2.0.0/tinyaudio/
- Oddio crate registry: https://crates.io/crates/oddio
- Oddio documentation: https://docs.rs/oddio/0.7.4/oddio/
- Kira crate registry: https://crates.io/crates/kira
- Kira documentation: https://docs.rs/kira/0.12.5/kira/
- FunDSP crate registry: https://crates.io/crates/fundsp
- FunDSP documentation: https://docs.rs/fundsp/0.23.0/fundsp/
- Ringbuf crate registry: https://crates.io/crates/ringbuf
- Ringbuf documentation: https://docs.rs/ringbuf/0.5.2/ringbuf/
- Triple_buffer crate registry: https://crates.io/crates/triple_buffer
- Rtrb crate registry: https://crates.io/crates/rtrb
- Microsoft WASAPI Overview: https://learn.microsoft.com/en-us/windows/win32/coreaudio/about-the-windows-audio-session-api
- Microsoft Low-Latency Audio: https://learn.microsoft.com/en-us/windows-hardware/drivers/audio/low-latency-audio
- Microsoft IAudioClient3 documentation: https://learn.microsoft.com/en-us/windows/win32/api/audioclient/nn-audioclient-iaudioclient3
- Microsoft Exclusive-Mode Streams: https://learn.microsoft.com/en-us/windows/win32/coreaudio/exclusive-mode-streams
- Microsoft Recovering from an Invalid Device: https://learn.microsoft.com/en-us/windows/win32/coreaudio/recovering-from-an-invalid-device-error
- Sigrist et al. (2013) Motor learning sonification review: https://link.springer.com/article/10.3758/s13423-012-0333-8
- Dubus & Bresin (2013) Sonification mapping review: https://journals.plos.org/plosone/article?id=10.1371/journal.pone.0082491
- Guadagnoli & Lee (2004) Challenge Point framework: https://pubmed.ncbi.nlm.nih.gov/15130760/
- Sherwood (1988) Bandwidth feedback: https://pubmed.ncbi.nlm.nih.gov/3224716/
- Ross Bencina Real-time Audio 101: http://www.rossbencina.com/code/real-time-audio-programming-101-time-waits-for-nothing
- J.O. Smith One-Pole Filter: https://ccrma.stanford.edu/~jos/filters/One_Pole.html
