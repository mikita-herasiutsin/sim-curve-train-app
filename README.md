# PR screenshots

This orphan branch holds the images embedded in pull request bodies, so PNGs stay out of `main`.
It shares no history with `main`; never merge it.

## Adding screenshots

1. Put the images in a folder named after the PR branch, e.g. `feat-SCT-050-drill-sets/set-finished.png`.
2. Commit and push to `pr-screenshots`.
3. Link each image by commit SHA so the link keeps working after later commits:
   `https://github.com/mikita-herasiutsin/sim-curve-train-app/blob/<sha>/<folder>/<file>.png?raw=true`

Screenshots follow the telemetry rule: aggregate stats and drill names only, no raw laps or exports.

Screenshots for PRs up to #35 were committed to `docs/screenshots/` on `main` and stay in its history.
