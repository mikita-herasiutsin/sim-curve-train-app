# Releasing

The committed version on `main` is the placeholder `0.0.0-dev`. Dev builds show this placeholder in the header. Release builds get their version from the release tag. The release workflow runs `node scripts/set-version.mjs <version>` on the runner before building. Nobody bumps versions in a PR any more.

## Before releasing

The CI `Release build` job uploads `SimCurveTrainApp-<sha>` for every push to `main`. This release executable is the only build that applies the Content Security Policy (CSP). Download the artifact for HEAD of `main` from the Actions run. Start it once on Windows 11 to confirm that it launches. That replaces the old clean-VM check on the draft.

## How to release

1. In the Actions tab, open the Release workflow, click **Run workflow** and keep branch `main`.
2. Or run the release script locally, with `gh` logged in. It starts the same workflow, follows it and prints the release URL.

```sh
npm run release
# or specify an explicit version
npm run release -- 0.1.0
```

Workflow inputs:
- `version`. Leaving this empty picks the next patch version after the highest existing `v*` tag.
- `prerelease`. On by default; turn it off for a full release. A version with a `-` suffix like `0.2.0-rc.1` always makes a pre-release.

Pushing a `v*` tag by hand on a `main` commit still works:

```sh
git switch main && git pull
git tag v<x.y.z>
git push origin v<x.y.z>
```

The workflow takes the version from the tag. A hand-pushed tag makes a full release unless it has a `-` suffix.

## What the workflow does

The workflow verifies that the version is higher than every existing `v*` tag. It tags the `main` commit `v<version>`. It writes the version into the build using `scripts/set-version.mjs`. It builds the installer and portable executable in about 10 to 15 minutes. It attests both files and confirms that both assets are attached. Finally, it publishes the release. The workflow creates the release as a draft first so a failed check never leaves a public broken release.

The release includes two assets:
- `SimCurveTrainApp_<x.y.z>_x64-setup.exe`: the NSIS installer
- `SimCurveTrainApp_windows_x64.exe`: the portable exe (`target/release/SimCurveTrainApp.exe`, renamed by `tauri-action`)

## If a run fails

Re-run failed jobs in Actions is safe. The version check accepts a tag that already exists at the same commit.

To give up on that version instead, delete the draft release and the tag:

```sh
git push --delete origin v<x.y.z>
git tag -d v<x.y.z>
```

Never move or delete a published tag. Release a new patch version instead.

## Code signing and SmartScreen

Builds are **not code-signed** for now ([D-16](decisions/README.md)). Windows SmartScreen therefore shows "Windows protected your PC" on first run, and Edge may warn that the download "isn't commonly downloaded". Users click **More info → Run anyway**; the README's [Install section](../README.md#install-windows) explains this. GitHub shows a SHA-256 digest for each release asset, which users can compare with `Get-FileHash`. Installers also carry build provenance attestations; verify with `gh attestation verify <file> --repo <owner>/<repo>`.

Signing (SignPath.io free OSS signing, or Azure Trusted Signing) is tracked as SCT-076 in the post-MVP backlog.
