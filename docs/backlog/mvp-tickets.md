# MVP Backlog (v0.1.0)

Each ticket is a **small, working slice**: when it is done there is something you can run and see or test. Sizes: **S** is about half a day to a day, **M** is 1–3 days. Tickets are listed in rough implementation order. `Deps` lists the tickets that must be finished first.

Status: ⬜ todo · 🟨 in progress · ✅ done

## Milestones

| Milestone | Tickets | Outcome |
|---|---|---|
| **M0 Foundation** | SCT-001 – SCT-003 | Empty app builds locally, CI is green, and a tag produces an `.exe` |
| **M1 See my pedals** (risk spike) | SCT-010 – SCT-015, SCT-020 – SCT-022 | VNM pedals detected and calibrated; live bars and graph; latency measured |
| **M2 First drill** | SCT-030 – SCT-033, SCT-038 | Hold drill with reps, scoring, results and audio |
| **M3 Traces** | SCT-034 – SCT-036 | Trace drills in both views with scoring |
| **M4 Content** | SCT-040 – SCT-045 | GT3, NASCAR and MX-5 presets and the pre-race warm-up |
| **M5 Progress & release** | SCT-023, SCT-050 – SCT-051, SCT-060 – SCT-062 | Personal bests and leaderboard, theme, onboarding, v0.1.0 release |

---

## M0: Foundation

### SCT-001 · Scaffold Tauri 2 + Svelte app ✅ · S
Create the app skeleton. The window title is "SimCurveTrainApp" and the window shows the app version.
- **AC:**
  - `npm run tauri dev` opens the window.
  - Rust workspace layout: `src-tauri/` for the app, plus a `crates/core` library for logic that doesn't depend on the UI.
  - `cargo fmt`, `clippy`, `prettier`, `eslint` and `svelte-check` are configured.
  - The README has a "Development" section.
- **Deps:** none

### SCT-002 · CI checks on push/PR ✅ · S
Add a GitHub Actions workflow on `windows-latest`. It runs fmt check, clippy (`-D warnings`), `cargo test`, `svelte-check`, the frontend unit tests (vitest) and a build.
- **AC:**
  - A PR with a lint error fails CI; a clean PR passes.
  - CI also runs a release build (`tauri build --no-bundle`) and uploads the exe. On desktop, `tauri dev` never applies the CSP, so CSP problems only show up in release builds; CI doesn't launch the app, so smoke-test that exe by hand.
- **Deps:** SCT-001

### SCT-003 · Release workflow ✅ · S
Pushing a `v*` tag runs `tauri-action`, which creates a draft GitHub Release with the NSIS installer and the portable `.exe` attached.
- **AC:**
  - Tagging `v0.0.1` produces a release with both files.
  - The exe starts on a clean Windows 11 machine.
  - The SmartScreen warning is documented (signing decision: see Q-08).
- **Deps:** SCT-001

---

## M1: See my pedals

### SCT-010 · List game controllers (SDL3) ✅ · M
Devices screen lists every connected controller with its name, GUID, axis count and button count. The list updates on hot-plug.
- **AC:**
  - **VNM pedals appear in the list.** This is the main risk check.
  - Plug and unplug updates the list within 1 s.
  - SDL3 is linked statically or bundled so the release exe runs on its own.
  - Results for each tested device are written to `docs/investigations/`.
- **Deps:** SCT-001

### SCT-011 · Raw axis monitor ✅ · S
Select a device to see every axis as a live bar showing the raw value and the normalised value. This is for working out which axis is which.
- **AC:** moving any pedal moves exactly one bar.
- **Deps:** SCT-010

### SCT-012 · 1 kHz input thread + sample stream ✅ · M
A dedicated Rust thread polls the selected device at about 1 kHz and timestamps each sample with QPC. Samples go into a ring buffer. A Tauri `Channel` streams them to the UI in batches every ≤8 ms. A debug HUD shows the actual sample rate (Hz) and the batch latency.
- **AC:**
  - The HUD shows ≥500 Hz with VNM pedals.
  - CPU use is under 3% while idle on the live view.
  - A panic in the input thread is logged and does not kill the app silently. Revisit `panic = "abort"` and `strip = true` in the release profile.
  - The ring buffer has unit tests.
- **Deps:** SCT-010

### SCT-013 · Axis assignment wizard ✅ · S
Prompts: "Press **brake** fully and release", then the same for throttle (clutch can be skipped). The wizard picks the axis that moved the most.
- **AC:** assigns the correct axes on VNM pedals with no manual selection; manual override is possible.
- **Deps:** SCT-011

### SCT-014 · Calibration (min/max, invert, deadzone) ✅ · S
Sweep each pedal to capture its range. Set invert and a deadzone at the low and high ends. Output is 0–100% (the value the game sees).
- **AC:**
  - A released pedal reads 0%, a fully pressed pedal reads 100%.
  - Deadzones work.
  - The normalisation function has unit tests.
- **Deps:** SCT-013

### SCT-015 · Persist device profile ✅ · S
Store axis assignment and calibration in SQLite, keyed by device GUID. On launch or reconnect, restore the profile automatically.
- **AC:** after an app restart the pedals work without any setup; a "Reset profile" button clears it.
- **Deps:** SCT-014

### SCT-020 · Live pedal bars ✅ · S
Main screen shows vertical bars for brake and throttle with large numbers from 0 to 100%, drawn on a canvas.
- **AC:** visibly no lag; holds 144 fps on a 144 Hz monitor (frame-time HUD).
- **Deps:** SCT-012, SCT-014

### SCT-021 · Scrolling pedal graph ✅ · M
A canvas graph shows brake and throttle over the last N seconds, scrolling right to left. A slider sets the window from 3 to 10 s (default 5 s).
- **AC:** smooth scrolling with no visible stutter; the window setting is persisted.
- **Deps:** SCT-020

### SCT-022 · Latency check mode ✅ · S
A full-screen flash appears when the brake crosses 50%. `docs/latency-test.md` explains how to film it with a slow-motion phone camera and work out the latency.
- **AC:** VNM pedals measured and the result recorded. **Target: <50 ms at 60 Hz, <20 ms at 144 Hz.**
- **Closed without filming:** the maintainer decided a filmed measurement isn't worth it and will check latency by hand with the flash. The tool and `docs/latency-test.md` shipped in #17.
- **Deps:** SCT-020

---

## M2: First drill

### SCT-030 · Drill/preset schema + loader ✅ · S
Define a JSON schema for presets and drills (types: hold, trace, sequence later). A Rust loader with validation reads the bundled presets folder. One sample preset is included.
- **AC:** an invalid preset gives a clear error; unit tests cover parsing and validation; `docs/preset-format.md` is written as part of SCT-030 to document the format.
- **Deps:** SCT-001

### SCT-031 · Hold drill, single rep ⬜ · M
A countdown runs, then the screen shows the target % (**as a large number**) and a tolerance band on the bar and the graph. The user holds the pedal inside the band for the set time. The band turns green inside and red outside.
- **AC:** works for both brake and throttle (from the `pedal` field); rep timing comes from the sample timestamps, not the UI clock.
- **Deps:** SCT-021, SCT-030

### SCT-032 · Hold scoring + result card 🟨 · M
Score each rep in Rust on the full-rate samples:
- **Accuracy:** time in the band and RMSE
- **Timing:** time until the pedal is first inside the band
- **Smoothness:** overshoot and jitter

These combine into a total from 0 to 100 and a grade (S ≥ 95 / A ≥ 85 / B ≥ 70 / C ≥ 55 / D). A result card shows the total and the breakdown.
- **AC:** scoring has unit tests on synthetic sample series (a perfect hold scores ≥ 98; a 10% offset scores below C).
- **Deps:** SCT-031

### SCT-033 · Reps + set summary ⬜ · S
A drill runs N reps (from the preset; default 5) with a short pause between reps. The set summary shows the score for each rep and adds a **consistency** sub-score (spread of the rep scores).
- **AC:** you can abort mid-set; the summary shows the best, average and consistency.
- **Deps:** SCT-032

### SCT-038 · Audio feedback ⬜ · M
Audio comes from the Rust side (`cpal`/`rodio`):
- **Hold drills:** the pitch follows the signed error, and a "lock" chime plays once the pedal is held in the band.
- **Trace drills:** a soft tone plays while out of the band (hooked up once SCT-034 is done).

Settings: on/off and volume.
- **AC:** the tone responds within about 20 ms with no audible glitches; mute is persisted.
- **Deps:** SCT-031

---

## M3: Traces

### SCT-034 · Trace drill: fixed curve + playhead view ⬜ · M
The whole target curve is drawn with its tolerance band. A playhead sweeps across it and the user's trace is drawn on top. The **current target % is shown as a number** next to the current %.
- **AC:** curve interpolation is linear between points; works for brake and throttle; there is a lead-in countdown.
- **Deps:** SCT-031

### SCT-035 · Trace drill: scrolling ghost view + toggle ⬜ · M
The target curve scrolls right to left towards a fixed "now" line, so you see what's coming next. The numeric target % is shown at the now-line. A toggle switches between ghost and playhead view, and the choice is persisted.
- **AC:** both views use the same drill engine and score the same.
- **Deps:** SCT-034

### SCT-036 · Trace scoring ✅ · M
- **Accuracy:** time in the band and RMSE
- **Timing:** lag, measured by cross-correlation
- **Smoothness:** release jerk and peak overshoot

These give the total and grade; reps and consistency reuse SCT-033.
- **AC:** unit tests on synthetic traces (exact copy, 100 ms delay, noisy, overshoot) give the expected order of scores.
- **Deps:** SCT-034, SCT-033

---

## M4: Content

### SCT-040 · Preset picker ⬜ · S
A home screen shows the preset cards (GT3, NASCAR and Road/MX-5). Opening a card lists its drills with type, pedal and best score; clicking a drill starts it.
- **AC:** presets are loaded from bundled JSON; the last preset used is remembered.
- **Deps:** SCT-030, SCT-033

### SCT-044 · Dev tool: `.ibt` zone extractor ⬜ · M
A Python script at `tools/ibt-extract/` (using pyirsdk) does the following:
1. Reads `.ibt` laps.
2. Finds brake and throttle zones.
3. Time-normalises them.
4. Simplifies the curves (Ramer–Douglas–Peucker).
5. Outputs draft drill JSON.

This is a developer tool only and is not shipped in the app.
- **AC:** running it on your own laps produces valid drill JSON that the app loads; there is a README with usage.
- **Deps:** SCT-030

### SCT-041 · GT3 preset ⬜ · S
Five or six drills hand-tuned from your own GT3 laps:
- Two brake holds (e.g. 70% and 90%)
- Two brake traces (hairpin, medium corner)
- One or two throttle drills (see Q-01)
- **AC:** every drill can be played and scored.
- **Deps:** SCT-044, SCT-036

### SCT-042 · NASCAR preset ⬜ · S
Same structure as GT3, using NASCAR-style traces (lower peak, long trail, partial-throttle holds).
- **Deps:** SCT-044, SCT-036

### SCT-043 · Road (MX-5) preset ⬜ · S
Same structure, using MX-5 traces (low grip, gentle threshold, early throttle).
- **Deps:** SCT-044, SCT-036

### SCT-045 · Pre-race warm-up ⬜ · M
One button on each preset card runs a 3–5 minute routine: a chain of the preset's drills with fewer reps. It ends with a summary.
- **AC:** the routine is defined in the preset JSON (see Q-05); you can skip a drill; the summary shows a score per drill and an overall warm-up score.
- **Deps:** SCT-040, at least one preset

---

## M5: Progress & release

### SCT-050 · Persist attempts ⬜ · S
Save every finished rep and set to SQLite: drill, preset, time, total score, sub-scores, and the compressed sample blob for replay later.
- **AC:** data survives a restart; schema migrations are in place (`refinery` or a simple versioned SQL).
- **Deps:** SCT-033

### SCT-051 · Personal bests + leaderboard ⬜ · S
- PB badges on drill cards.
- A leaderboard screen showing your own top 10 attempts per drill, with date and grade.
- A "New PB!" moment on the result card.
- **AC:** the PB updates right away; the leaderboard level is set by Q-02.
- **Deps:** SCT-050

### SCT-023 · Dark/light theme switch ⬜ · S
Theme tokens (CSS variables) for dark and light HUD palettes, including the canvas colours. The toggle is in the header and the choice is persisted.
- **AC:** every screen and canvas is readable in both themes.
- **Deps:** SCT-020

### SCT-060 · First-run onboarding ⬜ · S
On first launch: welcome screen, then device selection, then the axis wizard, then calibration, then a first easy hold drill.
- **AC:** a new user goes from installing to their first score with no docs.
- **Deps:** SCT-015, SCT-033

### SCT-061 · Settings screen ⬜ · S
One place for:
- Graph window length
- Audio on/off and volume
- Theme
- Default trace view
- Device profile reset
- "Copy diagnostics" (app version, devices, sample rate)
- **AC:** every setting is persisted and applied live.
- **Deps:** SCT-021, SCT-038

### SCT-062 · v0.1.0 release ⬜ · S
User README (install, SmartScreen note, first steps, screenshots), a CHANGELOG, a `v0.1.0` tag and the published release.
- **AC:** a fresh download installs and runs on a clean machine.
- **Deps:** all of the above

---

## Post-MVP backlog (v1.x and later, not scheduled)

- **SCT-016** DirectInput8 fallback backend. Moved into the MVP if SCT-010 finds devices that SDL misses.
- **SCT-070** Gamification: XP, levels, licence tiers (Rookie → Pro), daily streak, badges (Q-06)
- **SCT-071** Attempt replay: your trace drawn over the target, from the saved samples
- **SCT-072** "Blind" mode: the graph is hidden and only audio feedback plays
- **SCT-073** Sequence drills: brake → trail → throttle for a full corner
- **SCT-074** Presets from the telemetry pipeline (Garage 61 API, Pro); classes GTP/LMP, Formula and GT4
- **SCT-075** Auto-update (`tauri-plugin-updater`, opt-in)
- **SCT-076** Code signing (Q-08)
- **SCT-077** Security pass before sharing builds with users: a CI dependency audit gate (`cargo deny` or `cargo audit`, plus `npm audit --audit-level=high`), all findings fixed in one batch, and a CSP review. Pairs with SCT-076.
- **SCT-080** Combo drills with steering and trail braking (brake % against wheel angle)
- **SCT-081** Import and export presets as files
- **SCT-082** Career mode
- **SCT-083** Online leaderboard (needs a separate privacy decision)
