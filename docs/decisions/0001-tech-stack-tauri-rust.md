# ADR-0001: Tauri 2 + Rust core, Svelte UI

- **Status:** Accepted
- **Date:** 2026-10-06

## Context

- The app needs to look modern and polished. Gamification is part of the goal.
- Input-to-display latency must be **under 50 ms**. The user's pedal software has a noticeable delay, which is part of the motivation for this project.
- It ships as a single Windows `.exe` built by CI, so users never build it themselves.

## Options considered

| Option | Pros | Cons |
|---|---|---|
| **Tauri 2 + Rust + web UI** | Modern UI tooling, small binary (~10 MB), Rust input thread at 1 kHz, mature CI action | Rendering goes through WebView2. Latency is still bounded by the frame rate, so the budget is ~10–20 ms at 144 Hz. |
| Rust + egui | Lowest latency, single binary | Looks like a tool rather than a polished app; harder to gamify |
| C# .NET + WinUI/Avalonia | Mature Windows tooling, easy DirectInput | Self-contained exe is 60–100 MB |
| Browser (Gamepad API) | No install | Input updates only once per frame (~60 Hz); some pedals are never detected. The competitors that went this way have exactly these problems. |

## Decision

We use Tauri 2.

- **Rust side:** pedal input, the drill engine, scoring, audio and SQLite storage.
- **UI:** Svelte + TypeScript, drawing on `<canvas>` in `requestAnimationFrame`.
- **Bridge:** samples go from Rust to the UI in batches over a Tauri v2 `Channel`.

## Consequences

- Scoring always runs in Rust on the full-rate samples. How often the UI redraws never changes a score.
- WebView2 is required. It is preinstalled on Windows 11, and the installer can bootstrap it elsewhere.
- Contributors need both Rust and Node toolchains.
