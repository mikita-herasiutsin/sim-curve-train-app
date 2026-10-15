# Development notes

Problems that come up when you build, run or test the app on Windows, and how to avoid them. The commands themselves are in the [README](../README.md#development).

## Worktrees and builds

- A new git worktree can have an empty `node_modules`. Run `npm ci` before `npm run verify`. Don't run it while a `tauri build` runs: the build can then embed a broken frontend.
- A long worktree path breaks the SDL3 cmake build. Set a short `CARGO_TARGET_DIR`, such as `D:/ct`, for verify, smoke and clippy.
- After you move a worktree, delete `target/*/build/{tauri*,sdl3-sys*,sim-curve*}` and the matching `target/*/.fingerprint` folders. Their build scripts keep the old path.
- The exe compiles in the presets (`BUNDLED_PRESETS` in `crates/core/src/preset.rs`). A `presets/` folder next to the exe replaces a bundled preset with the same id. A `target/debug/presets/` folder left from an older build therefore breaks the preset steps of `npm run smoke`. Delete it.

## Running a second dev app

`tauri dev` uses port 1420, and the debug port is often 9222. To run a second app next to one that holds those ports:

- Pass `--config` with `{"build":{"devUrl":"http://localhost:1421","beforeDevCommand":"npm run dev -- --port 1421 --strictPort"}}`.
- Set `WEBVIEW2_USER_DATA_FOLDER` to an empty folder. Otherwise WebView2 joins the other app's instance and the page never appears.
- Set `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=<port>` with a free port.

`npm run smoke` does all of this itself. When you stop `tauri dev`, vite keeps running on its port; stop it before you start the app again.

## Driving the app over CDP

Use this for screenshots and for checks that tests don't cover.

- Start the app with `npm run smoke -- --keep-open`. It prints the debug port, and the app has calibrated simulated pedals and its own data folder. Stop it with Ctrl-C. If you kill the smoke process instead, the app, vite and the tauri launcher keep running, so stop them by PID.
- Move the pedals with `__TAURI_INTERNALS__.invoke("set_sim_pedals", { values: [throttle, brake, clutch], auto: false })`, and take screenshots with `Page.captureScreenshot`. `scripts/smoke.mjs` has examples of both.
- A Svelte `<select>` bound to objects ignores a new `.value`. Set `selectedIndex` and dispatch a `change` event.
- `Page.navigate` doesn't work while a drill runs. Click the in-app links instead.
- For a screenshot of terminal output, render it to an HTML file and run `msedge --headless=new --window-size=880,560 --screenshot=<png> file:///<html>`.

## Testing the portable exe

Run `npx tauri build --no-bundle` with its own `CARGO_TARGET_DIR`, such as `D:/ct-rel`. Copy the exe alone into an empty folder and start it from there, so it can't find a stray `presets/` folder. A release build ignores `SCT_DATA_DIR` and uses your real data folder.

## Code rules that tests don't catch

- **Draw loops don't allocate per sample.** In code that runs every frame, read samples with `FrameHistory.copySince` into reused typed arrays, never with `forEachSince`. Per-sample garbage allocated about 100 MB/s on the live view and grew the WebView's memory (PR #59).
- **New Tauri commands need two entries.** Add the command to the list in `src-tauri/build.rs` and allow it in `src-tauri/capabilities/default.json`. Without both, the webview call fails without an error, and vitest's `mockIPC` doesn't catch it.
