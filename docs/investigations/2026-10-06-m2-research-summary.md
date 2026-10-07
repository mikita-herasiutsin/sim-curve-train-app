# M2–M4 research summary

- **Date:** 2026-10-06
- **Author:** Claude. This page condenses four research reports drafted by Gemini and reviewed by Claude. Each report has an editor's note listing what was checked and what is unverified.

| Report | Tickets |
|---|---|
| [Input-to-photon latency in Tauri/WebView2](2026-10-06-webview-latency.md) | SCT-020, SCT-022 |
| [Scoring pedal traces and holds](2026-10-06-trace-scoring.md) | SCT-032, SCT-033, SCT-036, Q-03 |
| [Audio feedback from Rust on Windows](2026-10-06-audio-feedback.md) | SCT-038 |
| [iRacing .ibt zone extraction](2026-10-06-ibt-zone-extraction.md) | SCT-044, SCT-041–043, Q-07 |

## What it means for the backlog

### Latency (SCT-022)

- The measured live view (165 fps, 1000 Hz, batches about 8.5 ms apart) probably lands near the 20 ms target at 165 Hz, but not clearly under it. Only the SCT-022 measurement will tell.
- **Cheap wins, worth trying once SCT-022 has a baseline number:**
  - **`desynchronized: true` on the canvas contexts:** it may save about one frame, but it can tear. Put it behind a setting or flag and check by eye.
  - **4 ms batches instead of 8 ms:** this saves about 2 ms on average for the bars. It costs twice the IPC calls, which is still trivial.
  - **Persistent flash element:** keep the latency flash element in the DOM and toggle its opacity, instead of mounting it on each flash.
- **Don't** pass `--disable-gpu-vsync` or similar flags to WebView2. They don't remove DWM composition in a window and they burn CPU.
- **Load-cell brakes:** a phone camera can't see when a load-cell brake crosses 50%. Use the stomp method in `docs/latency-test.md`. For precise numbers, a photodiode plus an electrical trigger (an Arduino, LDAT-style) would be needed later.

### Scoring (SCT-032, SCT-036, SCT-033)

- **Hold scoring (SCT-032):** the formulas in that PR (time in band plus RMSE on the settled part, a time-to-band ramp, overshoot plus jitter) match the research in spirit. The research suggests counting "settled" from entering the band **and staying in it for 200 ms**. That's a small refinement to consider.
- **Trace scoring (SCT-036): proposed approach**
  - **Lag:** normalised cross-correlation within ±300 ms, with parabolic sub-sample interpolation.
  - **Accuracy on the lag-compensated trace,** so a late but well-shaped trace loses points only once (in timing), not twice.
  - **Asymmetric timing:** small anticipation is forgiven more than lateness. The research proposes a dead zone of -30..+10 ms, then a falloff with a half-score at 80 ms late and 140 ms early.
  - **Smoothness:** jerk on the release phase after a zero-phase low-pass filter at about 12 Hz, plus a peak-overshoot penalty. Differentiating raw 1 kHz data twice amplifies sensor noise, so the filter is mandatory.
  - **No DTW.** It would hide real hesitation, which is exactly what the drill is meant to train.
- **Consistency (SCT-033):** base it on the spread of rep scores (standard deviation or coefficient of variation), mapped to 0–100.

### Audio (SCT-038)

- Use `cpal` directly (Apache-2.0, GPL-compatible), with no higher-level engine. WASAPI shared mode at around 10 ms periods should meet the ~20 ms target. Exclusive mode isn't an option next to a running sim and voice chat.
- **Synth design:** the input thread writes the target pitch and volume into atomics. The audio callback smooths them with a one-pole filter of about 10 ms to avoid zipper noise. The oscillator phase stays continuous, and the chime has an attack/release envelope.
- **Pitch from the error:** use a musical (exponential) mapping, around one octave per 20% error, with silence inside the tolerance band ("bandwidth feedback"). That fits the motor-learning research.
- Warn about Bluetooth headsets: they typically add 100–200 ms.

### .ibt extraction (SCT-044)

- **Reader:** pyirsdk (MIT) can read `.ibt` files offline. A NumPy-based reader is much faster if speed matters. Driver inputs (`Brake`, `Throttle`) are 0–1 at the 60 Hz base rate.
- **Pipeline:** pick the fastest clean lap, then find zones with hysteresis thresholds and a minimum duration. Next, anchor t=0 at zone onset with a few hundred ms of lead-in, resample monotonically (PCHIP), then simplify with RDP on normalised axes. Expect 5–15 points per curve.
- **Output:** it maps directly onto the trace format in `docs/preset-format.md` (to be written as part of SCT-030): `[ms, percent]` points.

## Decisions for the maintainer

These aren't decided here. Each one is yours.

1. **Q-03, the default tolerance band.** The research leans towards a forgiving default that tightens with level (for example ±8% → ±6% → ±4%), following the challenge-point framework.
2. **Q-07, publishing curves derived from telemetry.** The legal section of the `.ibt` report is unverified and is not legal advice. Read the iRacing and Garage 61 terms before shipping presets built from anyone else's laps. Your own laps for hand-tuned presets are the safer start.
3. **Trace scoring weights and constants.** The proposed constants (accuracy/timing/smoothness 50/25/25, the falloff values) are starting points to tune on real attempts.
4. **Audio defaults.** Should the tone be on by default? Silent inside the band? How loud by default?
5. **Latency tweaks.** Try `desynchronized` canvases or 4 ms batching only after a baseline SCT-022 measurement, so the effect can be measured.
