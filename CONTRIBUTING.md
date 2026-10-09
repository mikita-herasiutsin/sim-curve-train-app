# Contributing to SimCurveTrainApp

Thanks for your interest! This is a small, free, open-source project (GPL-3.0-only).
By contributing you agree that your work is licensed under the same terms, and to follow the
[Code of Conduct](CODE_OF_CONDUCT.md).

## Before you start

- Check the [MVP backlog](docs/backlog/mvp-tickets.md) and the
  [open questions](docs/open-questions.md). Work is tracked as `SCT-NNN` tickets.
- For anything bigger than a small fix, open an issue first so we can agree on the approach.
- Architectural or product decisions go into the [decision log](docs/decisions/README.md),
  with an ADR when there are real trade-offs.

## Development setup

See the [Development section of the README](README.md#development) for prerequisites and commands.
Before pushing, always run:

```sh
npm run verify
```

CI runs the same checks on every pull request, and they must pass before merging.

## Code style

Formatting and linting are automated. Don't hand-tune style; run the tools.

| Area                | Tooling                                                                | Notes                                                                                     |
| ------------------- | ---------------------------------------------------------------------- | ----------------------------------------------------------------------------------------- |
| Rust                | `rustfmt` (`rustfmt.toml`), `clippy` with `pedantic` and `-D warnings` | Use `#[expect(lint, reason = "...")]` rather than `#[allow]`. `unsafe_code` is forbidden. |
| TypeScript / Svelte | Prettier (`.prettierrc`) and ESLint (`eslint.config.js`)               | Svelte 5 runes (`$state`, `$props`, `$derived`). Strict TypeScript.                       |
| Everything          | `.editorconfig`                                                        | UTF-8, LF line endings (enforced by `.gitattributes`), final newline                      |

**Conventions**

- **UI vs. core.** Logic that doesn't need a window goes in `crates/core` (`sct-core`) with unit
  tests. `src-tauri` only wires that logic to the UI through Tauri commands.
- **Pedal data stays in Rust.** Scoring and timing use the full-rate samples in Rust. The UI
  only draws the data; it never scores.
- **Offline.** No network calls, analytics or telemetry (see
  [ADR-0003](docs/decisions/0003-gpl3-offline-no-telemetry.md)).
- **Tests.** New behaviour comes with tests: `cargo test` for Rust, Vitest + Testing Library
  for the UI (`*.test.ts` next to the code, with Tauri IPC mocked through `mockIPC`).

## Branches, commits and pull requests

- `main` is protected. All changes go through a pull request with passing CI.
- Branch names: `feat/SCT-NNN-short-name`, `fix/...`, `docs/...`, `chore/...`.
- Commit subjects: imperative mood, ≤ 72 characters, starting with the ticket ID when there is
  one (e.g. `SCT-012: add 1 kHz input thread`).
- Fill in the pull request template. Every pull request needs screenshots: the changed app
  screens for UI changes, terminal output (test, smoke or command run) for other code changes, and
  the rendered diff for docs-only changes. Bot pull requests, such as Dependabot updates, are
  exempt; their CI run is the evidence. Mark the ticket ✅ in the backlog when its acceptance
  criteria are met.
- Drag screenshots into the pull request body. Don't commit them to `main`. Maintainers may
  instead commit them to the orphan
  [`pr-screenshots`](https://github.com/mikita-herasiutsin/sim-curve-train-app/tree/pr-screenshots)
  branch and link them by commit SHA, as its README describes.
- Pull requests are squash-merged.

## Reporting bugs and requesting features

Use the [issue templates](https://github.com/mikita-herasiutsin/sim-curve-train-app/issues/new/choose).
For bugs, include your pedal or wheel hardware, the app version and the diagnostics output when
it's available. For security issues, see [SECURITY.md](SECURITY.md).
