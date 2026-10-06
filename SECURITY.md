# Security Policy

## Supported versions

Only the latest release on [GitHub Releases](https://github.com/mikita-herasiutsin/sim-curve-train-app/releases)
receives fixes.

## Reporting a vulnerability

Please **do not open a public issue** for security problems.
Report them privately through
[GitHub private vulnerability reporting](https://github.com/mikita-herasiutsin/sim-curve-train-app/security/advisories/new).

You can expect an acknowledgement within a week. Once a fix is released, the issue will be
disclosed in the release notes.

## Scope

SimCurveTrainApp is a fully offline desktop app (no accounts, no network calls, no telemetry).
The most relevant areas are:

- The Tauri configuration (CSP, capabilities, IPC commands)
- Parsing of preset and drill files
- The installer and release artifacts
