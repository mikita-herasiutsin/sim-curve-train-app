# Releasing

Releasing takes one workflow run. The Release workflow ([`.github/workflows/release.yml`](../.github/workflows/release.yml)) builds the app from `main`, attests the files and publishes the GitHub Release, which creates the `v<version>` tag.

The committed version on `main` is the placeholder `0.0.0-dev`, which dev builds show in the header. Release builds get their version from the tag. The workflow runs `node scripts/set-version.mjs <version>` on the runner before it builds, so version bumps never go through a pull request. To build a tagged version from source with the right version number, run the same script after checking out the tag.

## Before releasing

The release publishes without a manual check, so try the build first. CI's `Release build` job uploads `SimCurveTrainApp-<sha>` for every push to `main`. It is a release build, so it applies the Content Security Policy, which `tauri dev` skips. Download the one for the HEAD of `main` from its Actions run and start it once on Windows 11. Nothing tests the NSIS installer before it goes public; if it turns out broken, release a new patch.

## How to release

1. In the Actions tab, open the Release workflow, click **Run workflow** and keep branch `main`.
2. Or run the release script locally, with `gh` logged in. It starts the same workflow, follows it and prints the release URL.

```sh
npm run release                    # next patch, as a pre-release
npm run release -- 0.1.0           # this version, as a pre-release
npm run release -- 0.1.0 --full    # this version, as a full release
```

Workflow inputs:

- `version`. Empty picks the next patch after the highest `v*` tag. If the highest tag is a pre-release such as `v0.2.0-rc.1`, enter the version.
- `prerelease`. On by default; turn it off (`--full` in the script) for a full release, which GitHub marks as Latest. A version with a `-` suffix like `0.2.0-rc.1` is always a pre-release.

Pushing a `v*` tag by hand on a `main` commit still works:

```sh
git switch main && git pull
git tag v<x.y.z>
git push origin v<x.y.z>
```

The workflow takes the version from the tag. The tag must still be the highest `v*` tag. A hand-pushed tag makes a full release unless it has a `-` suffix.

## What the workflow does

1. It checks that the run is on `main` and that the version is valid semver and higher than every existing `v*` tag.
2. It writes the version into the build with `scripts/set-version.mjs`.
3. It builds the installer and the portable exe, which takes about 10 to 15 minutes.
4. It attests both files and checks that both are attached.
5. It publishes the release, which creates the tag on the `main` commit the run started from. The release exists as a draft until this step, so a failed check never leaves a broken public release.

The release has two assets:

- `SimCurveTrainApp_<x.y.z>_x64-setup.exe`: the NSIS installer
- `SimCurveTrainApp_windows_x64.exe`: the portable exe (`target/release/SimCurveTrainApp.exe`, renamed by `tauri-action`)

## If a run fails

A failed run leaves at most a draft release and no tag. Click **Re-run failed jobs** in Actions, and the run reuses that draft. The workflow refuses to touch a release that is already published.

To give up on that version instead, delete the draft release. If you pushed the tag by hand, delete it too:

```sh
git push --delete origin v<x.y.z>
git tag -d v<x.y.z>
```

Never move or delete a published tag. Release a new patch version instead.

## Code signing and SmartScreen

Builds are **not code-signed** for now ([D-16](decisions/README.md)). Windows SmartScreen therefore shows "Windows protected your PC" on first run, and Edge may warn that the download "isn't commonly downloaded". Users click **More info → Run anyway**; the README's [Install section](../README.md#install-windows) explains this. GitHub shows a SHA-256 digest for each release asset, which users can compare with `Get-FileHash`. Installers also carry build provenance attestations; verify with `gh attestation verify <file> --repo <owner>/<repo>`.

Signing (SignPath.io free OSS signing, or Azure Trusted Signing) is tracked as SCT-076 in the post-MVP backlog.
