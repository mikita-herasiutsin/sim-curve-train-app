# Investigation: Sim Racing Pedal Trainer (Windows, iRacing-focused)

**Date:** October 6, 2026  
**Goal:** A free, open-source Windows app that trains brake and throttle muscle memory. The user follows a constant target pressure or a telemetry-like target trace and gets a score.  
**Primary audience:** iRacing drivers  
**Primary test hardware:** VNM pedals  
**Source prompt:** [original-prompt.md](original-prompt.md)

---

## 0. TL;DR

- **Similar tools exist, but none fits the brief.** Free and paid trainers already exist: Braking Lab, Baseline Driver Training, trailbraking.uk, HSimRacing Brake Curve Analyzer, SD70 BrakeMaster, SIXTYCM and the VPS Brake Pressure Sounder.
  - Almost all are **browser apps** that use the Gamepad API.
  - The one native Windows app (Baseline) is **paid and closed source**.
- **The gap:** no free, open-source, native, low-latency trainer exists that has car-class trace presets, hold and trace drills, a local leaderboard and gamification.
- **Browser apps have structural problems:**
  - They read input about once per frame (~60 Hz).
  - They only expose some HID devices. Braking Lab **did not detect VNM pedals** in the user's test.
  - A device often appears only after a button press.
- **Recommended stack:** **Tauri 2 + Rust**.
  - The Rust side reads pedals at 500–1000 Hz through SDL3, with DirectInput as fallback.
  - The UI is a web frontend that draws on a canvas.
  - Build: one `.exe` or installer from GitHub Actions, published on GitHub Releases.
- **Developer-only preset pipeline:** Python + `pyirsdk` on `.ibt` files, plus the Garage 61 API.
  - Telemetry access through the Garage 61 API **requires Pro** (about €5/month billed yearly).
  - The pipeline produces averaged curves per car class as JSON presets.

---

## 1. Market Scan: Existing Software

### 1.1 Direct competitors (target following with real pedals)

| Tool | Platform | Price | What it does | Gaps vs. this project |
|---|---|---|---|---|
| **Braking Lab** ([brakinglab.com](https://www.brakinglab.com/en)) | Web plus a Windows "Capture" companion that only syncs telemetry | Free / Plus €39/yr / Ultra €89/yr | Exercises: 66 in 15 collections (technique, car and coach-academy sets). <br>Techniques: threshold, progressive, trail, throttle and standing start. <br>Live tolerance band (±8%). <br>"Brake Master" licences (Rookie→A, 11 tracks, 120+ exercises). <br>Also: AI race engineer, track notes, Garage 61 CSV import. | Browser input; **it could not see the user's VNM pedals**. Closed source, freemium, account and cloud based. Its design and gamification are worth taking inspiration from. |
| **Baseline Driver Training** ([baselinedrivertraining.dk](https://baselinedrivertraining.dk/)) | **Native Windows app** | €19/mo or €119/yr, 7-day trial | Isolated brake and throttle practice. <br>Gauges in kg and %. <br>4 phases: Foundation → Consistency → Speed → Mastery. <br>Leaderboards, cloud sync, data import, works with any pedal set. | **Closest competitor**, but paid, closed source and cloud-based. No car-class presets advertised. No latency claims. |
| **Trail Braking Trainer** ([trailbraking.uk](https://trailbraking.uk/)) | Web (Gamepad API) | Free | Follow a ghost trace. <br>Tolerance rails. <br>RMSE "repeatability" score. <br>Workouts by corner type (hairpin, 90°, sweeper, random "Ultimate"). <br>Local history. | Brake only. Presets by corner type, not by car. Browser input. The repo is not public, though the author offered to share it. |
| **SD70 BrakeMaster** ([brakemaster.live](https://brakemaster.live/)) | Web (Chrome/Edge for pedals) | Free, with an optional account | Pressure hold, trail braking, consistency, reaction time and "blind" muscle-memory drills. Global records in each category. | Brake only. No car presets. Browser input. |
| **SIXTYCM** ([sixtycm.com](https://sixtycm.com/)) | Web | Free (Patreon) | Precision Tunnel, Target Gates and Brake Release modes. <br>Learn, Recall and Blind modes. <br>Adaptive difficulty. <br>Pedal smoothing for coarse pedals. | Abstract shapes, not car-derived traces. Browser input. |
| **HSimRacing Brake Curve Analyzer** ([hsimracing.com](https://hsimracing.com/brake-curve-analyzer/)) | Web | Free, plus paid training mode | One brake test scored on Shape, Smoothness, Attack and Control (0–100). Community leaderboard and a coach tips unlock. | Brake only. A single test rather than a training app. |
| **VPS Brake Pressure Sounder** ([velocityprosims.com](https://www.velocityprosims.com/pages/brake-pressure-sounder-trainer)) | Web | Free | Beeps when you reach a target from 1–100% (one-shot or continuous), with hysteresis. | No trace, no graph, no score. The audio-feedback idea is worth reusing. |
| **InputTrainer** ([inputtrainer.app](https://inputtrainer.app/)) | — | Waitlist | "F1 25 input training tool". | Not released; game-specific. |
| "Learn to Trail Brake" (Etsy listing) | Unknown | Paid | Could not be checked (403). | — |

### 1.2 Adjacent tools (none has target drills or scoring)

- **Live input overlays:**
  - RaceLab, SimHub, iOverlay, Kapps.
  - Open source: [TinyPedal](https://github.com/TinyPedal/TinyPedal) (rF2/LMU), [irdashies](https://github.com/tariknz/irdashies) (iRacing), [SimTrace](https://github.com/LinyL4/SimTrace).
- **In-sim coaching and telemetry:** Track Titan, Coach Dave Delta, VRS, Trophi.ai, Garage 61, Popometer. They all need you to drive in the sim. None offers isolated, off-track pedal drills.
- **GitHub projects:**
  - [Jamesdavies1403/Sim-racing-app](https://github.com/Jamesdavies1403/Sim-racing-app): Python, 0 stars, early. It scores `.ibt` corners and has a synthetic trace trainer.
  - DIY pedal firmware projects: ChrGri FFB pedal, BrakeBox, HX711 load-cell builds. These are hardware projects, not trainers.
- **Not found:** an open-source, standalone, native pedal-trace trainer.

### 1.3 Gap matrix

| Requirement | Braking Lab | Baseline | trailbraking.uk | BrakeMaster / SIXTYCM | **This project** |
|---|---|---|---|---|---|
| Native Windows `.exe`, no browser | ✗ | ✓ | ✗ | ✗ | ✓ |
| Free + open source | ✗ (freemium) | ✗ | free, closed | free, closed | ✓ GPL-3 |
| Robust detection of DirectInput devices (VNM etc.) | ✗ (failed for the user) | probably | ✗ | ✗ | ✓ |
| <50 ms input→display latency | ✗ (frame-bound) | unknown | ✗ | ✗ | ✓ target <20 ms at 144 Hz |
| Hold + trace drills | ✓ | ✓ | trace | ✓ | ✓ |
| Brake **and** throttle | ✓ | ✓ | ✗ | partial | ✓ |
| Car-class presets | partial | ✗ | ✗ | ✗ | ✓ (GT3, NASCAR, Road) |
| Local personal leaderboard | cloud | cloud | local history | global | ✓ |
| Gamification | ✓✓ | ✓ | ✗ | partial | ✓ |
| Offline, no account | ✗ | ✗ | ✓ | ✓ | ✓ |

### 1.4 Why browser trainers fall short

1. **Polling.** Chrome and Edge only update `navigator.getGamepads()` about once per animation frame, roughly 60 Hz. A native app can read at 500–1000 Hz and draw at the monitor's refresh rate.
2. **Device coverage.**
   - The Gamepad API only exposes HID devices with joystick or gamepad usages.
   - Pedal boxes with unusual descriptors, extra axes or vendor usages may not appear at all. This matches the user's Braking Lab result.
   - Steam Input can also grab devices.
3. **Gesture gating.** A device only appears after the user presses a button or moves an axis. Some pedals have no buttons.

---

## 2. Technical Architecture (Tauri 2 + Rust)

```
┌──────────────────────── Rust core (Tauri backend) ───────────────────────┐
│ Input thread (500–1000 Hz)                                               │
│   SDL3 joystick (RawInput / HIDAPI / DirectInput)  ──┐                   │
│   fallback: DirectInput8 via `windows` crate      ───┤→ calibration →    │
│                                                      │  ring buffer      │
│ Drill engine: target curve(t), timing, tolerance, scoring (full rate)    │
│ Audio: tone/pitch from the error (cpal/rodio, low-latency WASAPI)         │
│ Storage: SQLite (rusqlite) in %APPDATA%\<app>                            │
└───────────────┬──────────────────────────────────────────────────────────┘
                │ Tauri v2 Channel: batched samples every ~4–8 ms
┌───────────────▼──────── Web UI (WebView2) ───────────────────────────────┐
│ Svelte (or React) + TS; canvas drawing in requestAnimationFrame           │
│ live 0–100% bars · scrolling 5–10 s graph · target ghost/playhead ·      │
│ numeric target % · scores · XP/licences · leaderboard · theme switch     │
└──────────────────────────────────────────────────────────────────────────┘
```

### 2.1 Input layer
- **Primary: the SDL3 joystick subsystem** through the [`sdl3`](https://docs.rs/sdl3/latest/sdl3/) crate.
  - Version 0.20.0 was released 2026-09-07; bindings track SDL 3.4.x and the crate is active.
  - It needs no SDL window.
  - On Windows it combines RawInput, HIDAPI, DirectInput and WGI, so it is the most widely compatible option.
  - Axis values arrive as `i16` and are normalised to 0–1.
- **Fallback: DirectInput8** through the [`windows`](https://crates.io/crates/windows) crate.
  - iRacing reads pedals through DirectInput, so **any pedal that works in iRacing is guaranteed to appear here**.
  - The fallback exists in case SDL misses a device. Make it switchable in settings.
- **Reading:**
  - Use a dedicated thread at 1 kHz (or wait on events), and stamp each sample with `Instant` (QPC).
  - Store `(t, brake, throttle, clutch?, steer?)` in a lock-free ring buffer.
- **Calibration and mapping:**
  - Device picker, then "press the brake" auto-detection of the axis.
  - Min/max sweep, invert, deadzone.
  - Per-device profiles are saved and restored automatically.
- **Unit:** game-visible axis **%**. Curves set in the pedal's own software (e.g. VNM) are already applied, so the app trains the same value iRacing receives.

### 2.2 Rendering and latency
- **Frontend:** Svelte + TypeScript.
  - Svelte suits this: little runtime, so frames stay smooth. React would also work.
  - Draw the graph on a single `<canvas>`, either custom 2D or [uPlot](https://github.com/leeoniya/uPlot), redrawn on each `requestAnimationFrame`. Do not use DOM-based chart libraries.
- **IPC:** a Tauri v2 `Channel` streams sample batches. Scoring runs in Rust on full-rate data, so how often the UI draws does not affect scores.
- **Latency budget:**

| Stage | 60 Hz monitor | 144 Hz monitor |
|---|---|---|
| USB poll (device) | 1–4 ms | 1–4 ms |
| App read | ≤1 ms | ≤1 ms |
| IPC batch | ≤4–8 ms | ≤4 ms |
| Wait for next frame | ≤16.7 ms | ≤6.9 ms |
| Compositor / scanout | ~8–17 ms | ~3–7 ms |
| **Total (typical)** | **~20–40 ms** | **~10–20 ms** |

  - Add a **latency check mode**: a flash on the screen when the pedal crosses 50%, to be filmed with a slow-motion phone camera.
  - Turn off vsync-like throttling where possible, and keep the WebView in the foreground.

### 2.3 Data model
```jsonc
// preset (built-in JSON, also used later for user presets and sharing)
{
  "id": "gt3", "name": "GT3", "class": "gt3",          // road | gt3 | nascar | lmp | formula | custom
  "drills": [
    { "id": "gt3-hairpin", "type": "trace",              // hold | trace | sequence
      "pedal": "brake",                                  // brake | throttle | both
      "curve": [[0,0],[0.08,0.92],[0.6,0.85],[1.2,0.45],[1.8,0.1],[2.1,0]],  // [seconds, 0–1]
      "tolerance": 0.06, "reps": 5, "lead_in": 1.5 },
    { "id": "gt3-hold-70", "type": "hold", "pedal": "brake",
      "target": 0.70, "hold": 2.0, "tolerance": 0.04, "reps": 5 }
  ]
}
```
- **SQLite tables:**
  - `devices` and `calibrations`
  - `attempts`: drill, preset, timestamp, sub-scores, total, a compressed sample blob for replay
  - `personal_bests`
  - `profile`: XP, licence tier, streak

### 2.4 Scoring (0–100 total + letter grade + sub-scores)
| Sub-score | Metric |
|---|---|
| **Accuracy** | % of time inside the tolerance band, and RMSE against the target |
| **Timing** | Lag: cross-correlation offset or time-to-target. Hold drills: time until inside the band. |
| **Smoothness** | Jerk / sign changes of the derivative during release; penalty for overshooting the peak |
| **Consistency** | Spread of the scores (and traces) across the reps of a set |

Weights depend on the drill type. Grades: S ≥ 95, A ≥ 85, B ≥ 70, C ≥ 55, D otherwise.

### 2.5 Audio feedback
- **Hold drills:**
  - A pitch tone follows the signed error: above target sounds higher, below sounds lower.
  - A short "lock" chime plays once you stay inside the band.
- **Trace drills:** a soft tone while you are out of the band, and silence while you are in it.

> **Superseded (2026-10-09):** the pitch-following tone and the lock chime described here were replaced by parking-sensor beeps; see D-20 and [2026-10-08-error-sound.md](2026-10-08-error-sound.md).
- **Implementation:** `cpal`/`rodio` on the Rust side, so audio does not add WebView latency.
- **"Blind" variant** (as in SIXTYCM/BrakeMaster): the graph is hidden and only audio feedback plays. It is a good test of muscle memory.

### 2.6 Gamification (inspired by Braking Lab)
- XP for each attempt, scaled by score. Level-ups.
- **Licence tiers** (Rookie → D → C → B → A → Pro), unlocked by reaching set scores on set drills.
- Daily warm-up streak.
- Personal-best fanfare.
- Badges, e.g. "10× S-grade hold", "perfect trail".
- All stored locally. No account needed.

### 2.7 Distribution
- **Build and release:** a GitHub Actions workflow using [`tauri-apps/tauri-action`](https://github.com/tauri-apps/tauri-action) on `windows-latest`.
  - Tagging `v*` builds a release with an **NSIS installer `.exe`** and a portable `.exe`.
  - WebView2 is preinstalled on Windows 10 21H2+ and 11. The installer can bootstrap it otherwise.
- **SmartScreen:** unsigned builds show a "Windows protected your PC" warning. Options:
  - [SignPath.io](https://signpath.org/) free signing for open-source projects, or Azure Trusted Signing (about $10/mo).
  - Or accept the warning at first and document it.
- **Optional:** `tauri-plugin-updater` reading `latest.json` from GitHub Releases.
- **Expected size:** about 8–15 MB.

---

## 3. Preset Generation Pipeline (developer only, not shipped to users)

**Goal:** turn real fast-driver telemetry into realistic target traces for each car class.

1. **Sources:**
   - **Your own `.ibt` files.** iRacing writes them to `Documents\iRacing\telemetry` when recording (Alt+L). Channels: `Brake`, `Throttle`, `Speed`, `LapDistPct`, `SteeringWheelAngle`, at 60 Hz.
   - **Garage 61 API** ([developer portal](https://garage61.net/developer/endpoints/v1/findLaps)):
     - `GET /api/v1/laps` can filter by car and track. Then fetch the lap telemetry CSV.
     - **Telemetry, ghost laps and setups require the calling user to have Pro.** That is about €5/month billed yearly, and gives access to 300M+ laps.
     - The free account is enough for your own data only.
     - Use a personal API token. **Do not scrape.**
     - Existing helpers: the [`garage61api`](https://pypi.org/project/garage61api/) Python wrapper, and the [Garage61MCP](https://github.com/tqhdesilva/Garage61MCP) server, so an agent can query it directly.
2. **Selection:** for each car and track, take the top-N laps by lap time. Filter to high-iRating drivers if the API exposes that; otherwise use top lap times as the proxy.
3. **Segmentation:**
   - Find brake zones (brake > 5% → < 2%) and throttle pick-up zones (lift → full throttle).
   - Classify each zone by shape:
     - threshold + long trail (hairpin)
     - stab + short trail (chicane)
     - light trail/lift (fast corner)
     - throttle pick-up ramp
     - partial-throttle hold
4. **Normalisation:** align the zones at brake onset, then take the median curve per class and shape. Simplify to about 10–20 points (Ramer–Douglas–Peucker). Set tolerance from the spread between drivers.
5. **Output:** preset JSON (§2.3), committed to the repo. Tools: Python + `pyirsdk` + `numpy`, run as needed.
6. **Licensing note:** ship only **derived, aggregated** curves, never raw laps from other drivers. Check the Garage 61 terms before publishing presets built from Pro data. Fallback: hand-tune curves from your own laps.

**MVP note:** the first presets (GT3, NASCAR, Road/MX-5) can be hand-built from a few of your own laps. The pipeline then refines them.

---

## 4. Scope: MVP vs. Later

### MVP (v0.1)
- Device picker, auto axis detection, calibration, saved profiles (SDL3 with DirectInput fallback)
- Live view: brake and throttle 0–100% bars plus a scrolling graph with an adjustable 5–10 s window
- **Hold drills** (fixed or random target %) and **trace drills** for brake and throttle
- Both trace views, user-selectable: a **scrolling ghost** and a **fixed curve with playhead**. The **numeric target % to hit** is always shown next to the current %.
- Visual band colouring plus **audio feedback**
- Scoring: total, grade and 4 sub-scores
- Each drill is N reps
- One-click **pre-race warm-up** (3–5 min routine)
- Presets: **GT3**, **NASCAR** and **Road (MX-5)**, each separate
- Local attempt history and **personal leaderboard** / personal bests
- **Dark/light theme switch**
- GitHub Actions release (`.exe`)

### v1.x
- Gamification layer (XP, licences, streaks, badges). Some of it could come early in the MVP if it's cheap.
- Presets generated by the telemetry pipeline; more classes (GTP/LMP, Formula, GT4)
- Combined brake→throttle sequence drills (a full corner)
- Blind (audio-only) mode, replay of an attempt against the target, auto-updater, code signing

### Later / maybe (the "questionable" list)
- **Steering + trail-braking combination:** a 2D envelope of brake% against wheel angle. SDL3 already reads the wheel axis, so this is mostly a UI and scoring feature.
- **Career mode:** a structured curriculum from beginner to advanced.
- **Preset sharing and import:** the JSON format already supports it; file-based import is easy.
- **Online leaderboard:** needs a backend and anti-cheat thinking, which goes against the "offline" principle. Lowest priority.

---

## 5. Product Decisions (answered 2026-10-06)

| Topic | Decision |
|---|---|
| Differentiators | Native low latency; free and open source; robust pedal detection; gamification similar to Braking Lab |
| Stack | Tauri 2 + Rust |
| Look | Gamified, with a **dark/light HUD theme switch** |
| Feedback | **Visual + audio**; no overlay mode |
| Trace drills | **Both views, selectable** (scrolling ghost / fixed curve + playhead), **numeric target % always shown** |
| Scoring | Total 0–100 and grade, plus a breakdown (accuracy, timing, smoothness, consistency) |
| Units | Game-visible axis **% only** |
| MVP presets | **GT3**, **NASCAR**, **Road (MX-5)** as separate presets |
| Session flow | N reps per drill + one-click pre-race warm-up |
| Repo | **GPL-3.0**, fully offline, **no telemetry or crash reporting** |
| Distribution | Public GitHub, `.exe` on Releases, built by CI |
| Garage 61 | Free account now; upgrade to Pro when the preset pipeline needs it |

### Remaining minor questions
1. **Throttle drills:** which matters most: the pick-up ramp (exit), a partial-throttle hold (mid-corner), or modulation and lift (traction)?
2. **Leaderboard granularity:** per drill, per preset, per warm-up, or all three?
3. **Default tolerance band:** forgiving (±8%, like Braking Lab) and tightening as licence tiers rise, or strict (±4%) from the start?
4. **Repo and app name.** Some ideas: "PedalForge", "Trail Lab", "BrakePoint", "Pedal Dojo".
5. **Test hardware:** VNM pedals are the primary target. Can you test anything else (another pedal set, a wheel, a gamepad) to check the SDL3/DirectInput fallback?
6. **Warm-up content:** a fixed routine for each preset, or adaptive (it focuses on the drills with the lowest recent scores)?

---

## 6. Next Steps

1. Answer the §5 minor questions.
2. Create the repo with GPL-3.0, a Tauri 2 + Svelte template and the CI release workflow.
3. **Spike 1:** an SDL3 input thread plus a canvas graph. Measure latency with VNM pedals and check that they are detected. **This is the main risk to rule out early.**
4. **Spike 2:** the drill engine plus scoring for hold drills, then trace drills.
5. Hand-built GT3/NASCAR/MX-5 curves from your own `.ibt` laps, then the MVP UI polish and the v0.1 release.

---

## Sources

- Braking Lab: https://www.brakinglab.com/en · telemetry import docs: https://www.brakinglab.com/en/docs/features/telemetry-import
- Baseline Driver Training: https://baselinedrivertraining.dk/
- Trail Braking Trainer: https://trailbraking.uk/ · write-up: https://boxthislap.org/train-your-trailbraking-muscle-memory-with-this-free-browser-tool/
- SD70 BrakeMaster: https://brakemaster.live/
- SIXTYCM: https://sixtycm.com/
- HSimRacing Brake Curve Analyzer: https://hsimracing.com/brake-curve-analyzer/
- VPS Brake Pressure Sounder: https://www.velocityprosims.com/pages/brake-pressure-sounder-trainer
- InputTrainer: https://inputtrainer.app/
- Etsy "Learn to Trail Brake": https://www.etsy.com/listing/1016691771/learn-to-trail-brake-properly-software (not accessible)
- Jamesdavies1403/Sim-racing-app: https://github.com/Jamesdavies1403/Sim-racing-app
- SimTrace: https://github.com/LinyL4/SimTrace · TinyPedal: https://github.com/TinyPedal/TinyPedal · irdashies: https://github.com/tariknz/irdashies
- Garage 61 API: https://garage61.net/developer/endpoints/v1/findLaps · Pro: https://garage61.net/pro · garage61api: https://pypi.org/project/garage61api/ · Garage61MCP: https://github.com/tqhdesilva/Garage61MCP
- pyirsdk: https://github.com/kutu/pyirsdk
- sdl3 crate: https://docs.rs/sdl3/latest/sdl3/
- Gamepad API polling and buffering: https://github.com/w3c/gamepad/issues/46 · https://gpadtester.org/latency-test
- Market scan cross-check: Antigravity (Gemini) agent, with URLs checked by curl. Its unverified claims were excluded or marked.
