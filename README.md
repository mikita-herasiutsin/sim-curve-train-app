# SimCurveTrainApp

[![CI](https://github.com/mikita-herasiutsin/sim-curve-train-app/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/mikita-herasiutsin/sim-curve-train-app/actions/workflows/ci.yml)
[![License: GPL-3.0](https://img.shields.io/badge/license-GPL--3.0-blue.svg)](LICENSE)

Free, open-source Windows app for sim racers to train brake and throttle **muscle memory**. Pick a car class, follow a target pressure or a telemetry-like pedal trace with your real pedals, and get an instant score.

> Status: **pre-alpha**. Milestone M1 is done (controller detection, axis wizard, calibration, saved profile, live bars/graph, latency test); M2 (drills and scoring) is next. See the [MVP backlog](docs/backlog/mvp-tickets.md).

## Goals

- **Native and low-latency.** Pedals are read at 500–1000 Hz, with under 50 ms from input to display (the target is under 20 ms on a 144 Hz monitor).
- **Works with any pedal iRacing sees.** Pedals are read through SDL3, and a DirectInput fallback is planned. Browser-based trainers often fail to detect some pedals.
- **Car-class presets** (GT3, NASCAR and Road/MX-5 for the MVP), with hold and trace drills for brake and throttle.
- **Visual and audio feedback**, a 0–100 score with a grade and sub-scores, and a local personal leaderboard.
- **Gamified UI** with a dark/light theme.
- **Offline, no account, no telemetry.** Licensed GPL-3.0.
- **Easy install:** download the `.exe` from GitHub Releases.

## Install (Windows)

Requires Windows 10 or 11, 64-bit. The app is pre-alpha: it shows your pedals live but has no drills yet.

1. Open the [Releases page](https://github.com/mikita-herasiutsin/sim-curve-train-app/releases) and download one file:
   - `SimCurveTrainApp_<version>_x64-setup.exe`: the installer. It installs for your user only, adds a Start menu entry, and can be removed from **Settings → Apps**.
   - `SimCurveTrainApp_windows_x64.exe`: the portable version. No install, run it from any folder. It needs the Microsoft Edge WebView2 runtime, which Windows 11 already has; on Windows 10, use the installer, which fetches WebView2 for you.
2. If Edge says the file "isn't commonly downloaded", choose **Keep**.
3. The builds are not code-signed yet ([D-16](docs/decisions/README.md)), so on first run SmartScreen shows "Windows protected your PC". Click **More info**, then **Run anyway**. The warning appears because the file is unsigned and new, not because anything harmful was found.
4. Optional: compare the file with the SHA-256 digest shown next to it on the release page, or [build from source](#development).

   ```powershell
   Get-FileHash .\SimCurveTrainApp_<version>_x64-setup.exe -Algorithm SHA256
   ```

## Tech stack

Tauri 2 (Rust core) with a Svelte + TypeScript UI. See [ADR-0001](docs/decisions/0001-tech-stack-tauri-rust.md).

## Development

### Prerequisites (Windows)

- [Rust](https://rustup.rs/) stable (MSVC toolchain), 1.88 or newer
- [Node.js](https://nodejs.org/) 24 (see `.nvmrc`; 22.22.2+ also works), with npm
- Visual Studio Build Tools with the "Desktop development with C++" workload
- WebView2 runtime (preinstalled on Windows 11)

### Commands

```sh
npm install          # install frontend deps
npm run tauri dev    # run the app with hot reload
npm run tauri build  # release build: target/release/SimCurveTrainApp.exe + NSIS installer

npm run verify       # run every check below (what CI will run)
npm run check        # svelte-check (TypeScript + Svelte)
npm run lint         # eslint + prettier --check
npm run format       # prettier --write
npm test             # vitest (frontend unit/component tests)
npm run rust:fmt     # cargo fmt --check
npm run rust:lint    # cargo clippy -D warnings (pedantic)
npm run rust:test    # cargo test --workspace
```

### Layout

```
crates/core/   sct-core: logic that doesn't depend on the UI (input, drills, scoring, storage)
src-tauri/     Tauri app shell: commands wiring the core to the UI
src/           SvelteKit (SPA) + Svelte 5 frontend
docs/          investigations, decisions, open questions, backlog
```

## Documentation

See [`docs/`](docs/README.md):

- [Investigations](docs/investigations/): market scan, architecture, preset pipeline
- [Decisions](docs/decisions/): decision log and ADRs
- [Open questions](docs/open-questions.md)
- [MVP backlog](docs/backlog/mvp-tickets.md)
- [Releasing](docs/releasing.md)

## Contributing

Contributions are welcome. See [CONTRIBUTING.md](CONTRIBUTING.md) and the [Code of Conduct](CODE_OF_CONDUCT.md). Report security issues privately ([SECURITY.md](SECURITY.md)).

## License

[GPL-3.0-only](LICENSE)
