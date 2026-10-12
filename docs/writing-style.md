# Writing style

These rules apply to everything we write in this repo: docs, READMEs, code comments, UI text, commit messages, and pull request text. They apply to people and to AI agents alike. `npm run docs:check` enforces the mechanical ones (see [What the checker enforces](#what-the-checker-enforces)); reviewers check the rest.

The general rules are adapted from the [unslop skill](https://github.com/cursor/plugins/blob/main/pstack/skills/unslop/SKILL.md). The rules for steps and UI text borrow from ASD-STE100 Simplified Technical English.

## Who reads what

| Reader                 | Reads                                                     | Needs                                                  |
| ---------------------- | --------------------------------------------------------- | ------------------------------------------------------ |
| Sim racers (users)     | README install section, UI text, release notes            | Short steps they can follow, often in a second language |
| Contributors           | CONTRIBUTING.md, `docs/`, code comments                   | Facts about this project: commands, paths, reasons     |
| AI agents              | the same files, plus [AGENTS.md](../AGENTS.md)            | The same facts, stated once, in one place              |

## General rules

1. **Say what it does.** Name the mechanism, the command or the number. "Pedals are read at 1000 Hz" beats "fast, responsive input". If a sentence could appear unchanged in another project's docs, cut it.
2. **One idea per sentence.** If the reader has to read a sentence twice, split it.
3. **Active voice.** Name the actor: "the input thread drops the sample", not "the sample is dropped".
4. **Plain words.** Write `use`, not `utilize` or `leverage`; `help`, not `facilitate`; `to`, not `in order to`; `is` and `has`, not `serves as` or `boasts`.
5. **No filler or hedging.** Cut `it is important to note that`, `could potentially`, `very`, and closing summaries such as "this makes the app better".
6. **State the point directly.** Drop "not just X, but Y" framing, forced groups of three, and metaphors where a literal word exists.
7. **Write whole sentences.** Keep the articles and verbs. Spell out arrows and abbreviations the reader may not know.
8. **Punctuation.** No em dashes or en dashes: use a period or a comma, and write ranges as "3 to 5 minutes" or "3-5 minutes". Straight quotes only. Colons go before a list or an example, not in the middle of a sentence.
9. **Formatting.** Sentence-case headings. Bold only a term the reader must find again, not every product name. No decorative emoji.

## Steps and UI text

Users follow these while sitting at their pedals, and many read English as a second language. Write them in the simplified style:

- **One action per step.** Use the imperative: "Press the brake pedal fully."
- **Steps stay under 20 words.** Other sentences stay under 25 words.
- **Condition first.** "If SmartScreen shows a warning, click **More info**."
- **Warnings before the step** they apply to, never after it.
- **Name controls exactly as on screen**, in bold: **Start drill**, **Settings → Apps**.
- **Use the terms below**, with the meaning given. A word means one thing everywhere.

## Terms

Use one word per concept, in docs, UI text and code names. Don't swap in synonyms.

| Term                  | Meaning                                                                                          | Don't write             |
| --------------------- | ------------------------------------------------------------------------------------------------ | ----------------------- |
| preset                | A car class's set of drills and its warm-up, in one `presets/*.json` file                         | profile, pack           |
| drill                 | One exercise in a preset: a hold drill or a trace drill                                          | exercise, task          |
| hold drill            | Hold one pedal at a target % for a set time                                                      | static drill            |
| trace drill           | Follow a target curve over time                                                                  | curve drill             |
| rep                   | One run of a drill, scored on its own                                                            | repetition, try, round  |
| set                   | All reps of one drill in a row                                                                   | session                 |
| warm-up               | A preset's fixed chain of drills, run with one click                                             | warmup, routine         |
| lead-in               | The countdown before each rep (3, 2, 1, GO)                                                      | countdown, prep         |
| throttle lead-in      | The throttle hold a brake drill can require before the brake point                               |                         |
| band                  | The tolerance band: target ± tolerance                                                           | window, zone            |
| profile               | Saved pedal setup for one device: axis mapping and calibration                                   | config                  |
| attempt               | A saved, scored set                                                                              | result, run             |
| score, grade          | 0 to 100, and its letter                                                                         | points, rank            |

Code and JSON names may differ (`warmup`, `leadInMs`); keep them as they are in code spans.

## Code comments

Comments follow the general rules. A comment says why the code does something, or what a reader can't see from the code: a unit, a limit, a decision id such as D-21. One line is the norm. Don't restate the code, and don't describe the change history; that belongs in the commit message.

## What the checker enforces

`npm run docs:check` (part of `npm run verify` and CI) reports:

- broken relative links and heading anchors,
- `npm run` scripts that don't exist in `package.json`,
- `SCT_*` environment variables that the code reads but no doc mentions, or the reverse,
- em dashes, en dashes, curly quotes, and the most common filler words and phrases, in docs and code comments.

It skips fenced code and inline code for links and prose, and it skips `docs/investigations/`, which holds dated research records. To keep a line that quotes a banned word on purpose, add `<!-- prose-ok -->` to the line (or `prose-ok` in a code comment).
