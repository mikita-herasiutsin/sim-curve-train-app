# Instructions for AI agents

Read [CONTRIBUTING.md](CONTRIBUTING.md) for the code conventions and the pull request rules. This file adds what agents tend to skip.

## Write in the project style

Before you write or edit prose, read [docs/writing-style.md](docs/writing-style.md) and follow it. It covers docs, code comments, UI text, commit messages, pull request bodies and review reports, and its glossary sets the terms to use.

## Update the docs in the same change

When a change alters something a doc describes, update that doc in the same pull request:

- behaviour a user sees, or a drill, preset or score rule,
- a command, an npm script, a CLI flag or an environment variable,
- a preset JSON field, default or limit (`docs/preset-format.md`),
- a file path or folder layout,
- a ticket's status (`docs/backlog/mvp-tickets.md`) or a decision (`docs/decisions/README.md`).

To find the doc, `git grep` the name you changed in `*.md`. The `RULES` list in `scripts/check-docs.mjs` maps code paths to the docs CI requires.

## Read before you build or run the app

[docs/dev-notes.md](docs/dev-notes.md) lists build problems on Windows and how to drive the app over CDP for screenshots. It also has two code rules that tests don't catch.

## Check before you finish

Run `npm run docs:check` after any doc or comment change, and `npm run verify` after any code change. Both must pass. CI also fails a pull request that changes app code without changing a doc, unless its body has a line `Docs: none, <your reason>`.
