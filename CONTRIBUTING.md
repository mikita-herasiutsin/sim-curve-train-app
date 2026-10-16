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

## Quality gates

CI enforces three gates on pull requests: cyclomatic complexity ≤ 12 per function, CRAP ≤ 12 per function, and mutation score ≥ 80% per file. The `quality.config.json` file configures the gates.

`CRAP = complexity² × (1 - coverage)³ + complexity`, where coverage is the share of lines (Rust, from cargo-llvm-cov) or statements (TypeScript, from Vitest's v8 coverage) tests run. A function at complexity 12 needs full coverage; an untested function passes only up to complexity 3. Mutation score is `(killed + timed out) / (killed + timed out + survived + not covered)`. The gate computes the score per file, and each file it mutates must reach 80%. cargo-mutants omits mutants that do not compile.

CI checks only functions a pull request adds or changes (overlapping a changed line against the merge base), plus the pilot files `crates/core/src/audio_map.rs` and `src/lib/pedals/history.ts`. CI does not check untouched code. To check another file, add it to `pilot` in `quality.config.json`.

| Files              | Complexity | CRAP                            | Mutation        |
| ------------------ | ---------- | ------------------------------- | --------------- |
| `*.rs`             | yes        | under `crates/`                 | under `crates/` |
| `src/**/*.ts`      | yes        | yes                             | yes             |
| `scripts/**/*.mjs` | yes        | only with a sibling `.test.mjs` | no              |
| `src/**/*.svelte`  | yes        | no                              | no              |

Scripts without unit tests, such as `scripts/smoke.mjs`, get complexity only.

The gate does not measure test files, `#[cfg(test)]` code, or `*.d.ts`. `src-tauri` gets complexity only, because coverage and mutation testing need the WebView build and real pedals.

Rust complexity comes from `tools/complexity` (a `syn`-based counter), TS complexity from ESLint's `complexity` rule, Rust mutants from `cargo-mutants`, and TS mutants from StrykerJS.

### What counts toward complexity

Rust (`tools/complexity`): 1, plus 1 for each `if` (including `else if`), `while`, `for`, `loop`, each `match` arm after the first, each match guard, each `let ... else`, each `&&` and `||`. The `?` operator does not count. Closures count toward the enclosing function; the counter measures nested `fn` items on their own. The counter skips branches inside macro invocations (`assert!`, `matches!`, `macro_rules!`). The gate skips code under `#[test]`, `#[tokio::test]`-style attributes, or a `cfg` predicate that requires `test`.

TypeScript uses ESLint's `complexity` rule, where every function and arrow counts on its own, so a nested arrow does not add to its parent. ESLint inline directives (`eslint-disable`) do not affect the gate.

### Commands

Run `cargo install --locked cargo-mutants@27.1.0 cargo-llvm-cov@0.9.1` and `rustup component add llvm-tools-preview` once.

- `npm run quality`: complexity and CRAP on changed functions (compared with `origin/main`; pass another base with `npm run quality -- origin/<branch>`).
- `npm run quality:mutation`: mutation tests on changed lines. Takes minutes. Stryker mutates TS files in place and restores them when it ends, so do not run this while a dev server watches the checkout. A run killed part-way (closed terminal, Ctrl-C twice) can leave a mutated TS file in the tree. Restore it with `git checkout -- <file>` or copy it back from `.stryker-tmp/backup-*`, and check `git status` before committing. When Vitest crashes before it collects any test, the gate counts that mutant as survived, not killed. A crash after the tests start, such as a mutant that loops until the worker runs out of memory, counts as killed.
- `npm run quality:pilot`: all gates on the pilot files.

The gates are not part of `npm run verify` yet, because they run tests a second time with coverage.

### Suppressions

Each suppression needs a reason:

- Complexity or CRAP: `// quality-ignore: <reason>` on the line above the function (attributes and doc comments in between are fine). A suppression without a reason fails.
- TS mutant no test can catch: `// Stryker disable next-line <mutator>: <reason>`.
- Rust mutant no test can catch: an `exclude_re` entry in `.cargo/mutants.toml` with a comment saying why.

### CI

The `quality` job in `.github/workflows/ci.yml` runs on Ubuntu. Pull requests run `npm run quality` and `npm run quality:mutation` against the base branch; pushes to `main` run `npm run quality:pilot`. The mutation step runs even when the metrics step fails, so a pull request sees both results. When a run fails, CI uploads mutation reports as an artifact.

## Docs and writing

- **Docs change with the code.** A pull request that changes behaviour, a command, a flag, an environment variable, a preset field or a file path also updates the doc that describes it. CI enforces this. The `RULES` list in `scripts/check-docs.mjs` maps code paths to the docs they need. A pull request that changes app code without changing any doc needs a line `Docs: none, <your reason>` in its body.
- **Write in the project style.** Docs, comments, UI text, commit messages and pull request text follow [docs/writing-style.md](docs/writing-style.md). `npm run docs:check` (part of `npm run verify`) checks links, commands, environment variables and the mechanical prose rules.
- AI agents also read [AGENTS.md](AGENTS.md).

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
