# Releasing

Releases are built by [`.github/workflows/release.yml`](../.github/workflows/release.yml) when a `v*` tag is pushed. The workflow creates a **draft** GitHub Release; nothing is public until you publish it.

## Steps

1. **Bump the version** on a branch, in both places (`src/version-sync.test.ts` checks that they match):
   - `version` under `[workspace.package]` in `Cargo.toml`
   - `package.json`: run `npm version <x.y.z> --no-git-tag-version`

   Then run `cargo metadata --format-version 1 > /dev/null` to refresh `Cargo.lock`, and merge the change to `main` through a pull request as usual.
2. **Tag the merge commit on `main`** and push the tag:

   ```sh
   git switch main && git pull
   git tag v<x.y.z>
   git push origin v<x.y.z>
   ```

   The workflow refuses to build if the tag doesn't equal `v` + the app version, or if the tagged commit isn't on `main`. A tag with a pre-release suffix (e.g. `v0.2.0-rc.1`) creates a pre-release.
3. **Wait for the Release workflow** (Actions tab, about 10–15 minutes). The draft release appears under Releases with:
   - `SimCurveTrainApp_<x.y.z>_x64-setup.exe`: the NSIS installer
   - `SimCurveTrainApp_windows_x64.exe`: the portable exe (`target/release/SimCurveTrainApp.exe`, renamed by `tauri-action`)
4. **Smoke-test both files on a clean Windows 11 machine** (a fresh VM or Windows Sandbox works). `tauri dev` never applies the CSP on desktop, so this is the first time the CSP is exercised:
   - The installer installs, the app starts from the Start menu, and the header shows the new version.
   - The portable exe starts from a plain folder (e.g. `Downloads`).
   - SmartScreen shows the expected "unknown publisher" warning (see below), and nothing else.
5. **Edit the draft notes** if needed and click **Publish release**.

## If the build fails or the files are wrong

The release is still a draft, so it's safe to redo. Delete the draft release and the tag, fix the problem on `main`, then tag again:

```sh
git push --delete origin v<x.y.z>
git tag -d v<x.y.z>
```

Never move a tag after its release has been published; release a new patch version instead.

## Code signing and SmartScreen

Builds are **not code-signed** for now ([D-16](decisions/README.md)). Windows SmartScreen therefore shows "Windows protected your PC" on first run, and Edge may warn that the download "isn't commonly downloaded". Users click **More info → Run anyway**; the README's [Install section](../README.md#install-windows) explains this. GitHub shows a SHA-256 digest for each release asset, which users can compare with `Get-FileHash`. Installers also carry build provenance attestations; verify with `gh attestation verify <file> --repo <owner>/<repo>`.

Signing (SignPath.io free OSS signing, or Azure Trusted Signing) is tracked as SCT-076 in the post-MVP backlog.
