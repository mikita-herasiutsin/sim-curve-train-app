# SimCurveTrainApp

Free, open-source Windows app for sim racers to train brake and throttle **muscle memory**. Pick a car class, follow a target pressure or a telemetry-like pedal trace with your real pedals, and get an instant score.

> Status: **pre-development**. Investigation and MVP backlog are done; code has not started yet.

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

## Documentation

See [`docs/`](docs/README.md):

- [Investigations](docs/investigations/): market scan, architecture, preset pipeline
- [Decisions](docs/decisions/): decision log and ADRs
- [Open questions](docs/open-questions.md)
- [MVP backlog](docs/backlog/mvp-tickets.md)

## License

[GPL-3.0-only](LICENSE)
