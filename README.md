# SimCurveTrainApp

Free, open-source Windows app for sim racers to train brake and throttle **muscle memory**. Pick a car class, follow a target pressure or a telemetry-like pedal trace with your real pedals, and get an instant score.

> Status: **pre-alpha**. The app skeleton is in place (SCT-001); see the [MVP backlog](docs/backlog/mvp-tickets.md) for what comes next.

## Goals

- **Native and low-latency.** Pedals are read at 500–1000 Hz, with under 50 ms from input to display (the target is under 20 ms on a 144 Hz monitor).
- **Works with any pedal iRacing sees.** Pedals are read through SDL3, with DirectInput as a fallback. Browser-based trainers often fail to detect some pedals.
- **Car-class presets** (GT3, NASCAR and Road/MX-5 for the MVP), with hold and trace drills for brake and throttle.
- **Visual and audio feedback**, a 0–100 score with a grade and sub-scores, and a local personal leaderboard.
- **Gamified UI** with a dark/light theme.
- **Offline, no account, no telemetry.** Licensed GPL-3.0.
- **Easy install:** download the `.exe` from GitHub Releases.

## Tech stack

Tauri 2 (Rust core) with a Svelte + TypeScript UI. See [ADR-0001](docs/decisions/0001-tech-stack-tauri-rust.md).

## Development

### Prerequisites (Windows)

- [Rust](https://rustup.rs/) stable (MSVC toolchain), 1.85 or newer
- [Node.js](https://nodejs.org/) 22 or newer, with npm
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

## License

[GPL-3.0-only](LICENSE)
