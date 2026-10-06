# Docs

| Folder / file | Contents |
|---|---|
| [`investigations/`](investigations/) | Research reports, one file per topic, named `YYYY-MM-DD-<topic>.md`. Write new ones whenever we need to research something. |
| [`decisions/`](decisions/) | Decision log ([index](decisions/README.md)) plus ADRs for decisions that change the architecture or the product. |
| [`open-questions.md`](open-questions.md) | Unanswered questions. Once one is answered, move it into the decision log. |
| [`backlog/mvp-tickets.md`](backlog/mvp-tickets.md) | MVP tickets: small, working slices with acceptance criteria. |
| [`releasing.md`](releasing.md) | How to cut a release: version bump, tag, smoke test, publish. |

## Conventions

- **Decisions:** each one gets a row in `decisions/README.md`. If it has trade-offs worth explaining, also add an `NNNN-title.md` ADR (Context / Decision / Consequences).
- **Tickets:** IDs are `SCT-NNN`. Ticket status is tracked in the backlog file until we move the tickets to GitHub Issues.
