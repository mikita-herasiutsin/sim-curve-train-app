# ADR-0003: GPL-3.0, fully offline, no telemetry

- **Status:** Accepted
- **Date:** 2026-10-06

## Context

- The competing tools are freemium, need an account, and run in the cloud.
- A free alternative that respects privacy is one of the reasons this project exists.

## Decision

- **License:** GPL-3.0-only.
- **No network calls at runtime.** The one exception would be an optional update check, which is post-MVP and must be opt-in.
- **No analytics and no crash reporting.**
- **All data stays local.** It lives in SQLite under `%APPDATA%\SimCurveTrainApp`.

## Consequences

- Forks have to stay open source.
- Any bug reports come from users filing GitHub issues by hand. To make that easy, the app should offer a "copy diagnostics" button that produces text the user can paste.
- An online leaderboard would need a separate opt-in decision. It stays out of scope.
