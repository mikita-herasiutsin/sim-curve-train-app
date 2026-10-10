// End-to-end smoke test: launches the real Tauri app with the simulated pedals and drives its
// WebView2 over the Chrome DevTools Protocol. Run with `npm run smoke`.
//
//   --keep-open        leave the app running afterwards (stays alive until Ctrl-C)
//   --out <dir>        screenshot directory (default: smoke-out/)
//   --no-launch --cdp <port>   attach to an app that is already running (it must have been
//                      started with SCT_SIM_PEDALS=1 and a fresh SCT_DATA_DIR). The port is
//                      required so the normal dev app on 9222 is never hit by accident.
import { spawn, spawnSync } from "node:child_process";
import { createServer } from "node:net";
import {
  closeSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  openSync,
  readdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const SIM_NAME = "SCT Simulated Pedals";

// ---------------------------------------------------------------- options

const argv = process.argv.slice(2);
const flag = (name) => argv.includes(name);
const option = (name, fallback) => {
  const i = argv.indexOf(name);
  if (i < 0) return fallback;
  const value = argv[i + 1];
  if (value === undefined || value.startsWith("--")) {
    throw new Error(`usage: ${name} needs a value`);
  }
  return value;
};
const KEEP_OPEN = flag("--keep-open");
const NO_LAUNCH = flag("--no-launch");
const OUT_DIR = resolve(ROOT, option("--out", "smoke-out"));
const BUILD_TIMEOUT_MS = 20 * 60_000;
// Whole run, build included. Then the summary is printed and the run fails.
const WATCHDOG_MS = 25 * 60_000;
const CDP_CALL_TIMEOUT_MS = 20_000;

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const log = (...args) => console.log(...args);

/** An error that ends the run: waitFor rethrows it at once instead of polling on. */
class Fatal extends Error {}

/** Set when the app process dies; every wait and CDP call then fails with it. */
let appDied = null;
const checkAlive = () => {
  if (appDied) throw new Fatal(appDied);
};

/** Polls `predicate` until it returns a truthy value; throws with `what` on timeout. */
async function waitFor(predicate, { timeout = 10_000, interval = 100, what = "condition" } = {}) {
  const deadline = Date.now() + timeout;
  let lastError;
  for (;;) {
    checkAlive();
    try {
      const value = await predicate();
      if (value) return value;
    } catch (e) {
      if (e instanceof Fatal) throw e;
      lastError = e;
    }
    checkAlive();
    if (Date.now() > deadline) {
      throw new Error(
        `timed out after ${timeout} ms waiting for ${what}` +
          (lastError ? ` (${lastError.message})` : ""),
      );
    }
    await sleep(interval);
  }
}

// ---------------------------------------------------------------- launching the app

let appProc = null;
let tempDir = null;
let dataDir = null;
// Substrings of the command lines of everything the launch started; see reapLeftovers.
const markers = [];

const appRunning = () =>
  appProc !== null && appProc.exitCode === null && appProc.signalCode === null;

/** Kills what this run started: the process tree, then leftovers found by a unique marker. */
function killApp() {
  if (appRunning()) {
    // Only while the child is alive: its PID could belong to someone else once it has exited.
    spawnSync("taskkill", ["/PID", String(appProc.pid), "/T", "/F"], { stdio: "ignore" });
  }
  appProc = null;
  if (markers.length > 0) reapLeftovers();
}

/**
 * Kills processes whose command line contains one of our markers: the temp config of the tauri
 * CLI, vite's `--port <n> --strictPort`, the temp WebView2 profile. Never matches by name alone.
 * The markers travel in an environment variable, so this PowerShell's own command line is no match.
 */
function reapLeftovers() {
  const script = `
    $m = $env:SMOKE_MARKERS -split '\\|'
    Get-CimInstance Win32_Process | Where-Object {
      $cl = $_.CommandLine
      $_.ProcessId -ne $PID -and $cl -and ($m | Where-Object { $cl.Contains($_) })
    } | ForEach-Object { & taskkill /PID $_.ProcessId /T /F | Out-Null }
  `;
  spawnSync("powershell", ["-NoProfile", "-NonInteractive", "-Command", script], {
    stdio: "ignore",
    env: { ...process.env, SMOKE_MARKERS: markers.join("|") },
    timeout: 30_000,
  });
}

function cleanup() {
  killApp();
  if (tempDir) {
    try {
      rmSync(tempDir, { recursive: true, force: true, maxRetries: 3 });
    } catch {
      // WebView2 may still hold files for a moment; the OS temp cleaner gets them.
    }
  }
}
process.on("exit", cleanup);
for (const sig of ["SIGINT", "SIGTERM", "SIGBREAK"]) {
  process.on(sig, () => process.exit(130));
}

function freePort(start) {
  return new Promise((res) => {
    const server = createServer();
    server.once("error", () => res(freePort(start + 1)));
    server.listen(start, "127.0.0.1", () => server.close(() => res(start)));
  });
}

async function launchApp() {
  // Random start ports, so parallel runs rarely collide; 1420 and 9222 belong to the normal dev app.
  const devPort = await freePort(1421 + Math.floor(Math.random() * 200));
  const cdpPort = await freePort(9223 + Math.floor(Math.random() * 200));
  tempDir = mkdtempSync(join(tmpdir(), "sct-smoke-"));
  dataDir = join(tempDir, "data");
  const webviewDir = join(tempDir, "webview2");
  mkdirSync(dataDir);
  mkdirSync(webviewDir);
  // vite.config.js binds 127.0.0.1 (unless TAURI_DEV_HOST is set, which is removed below).
  const url = `http://127.0.0.1:${devPort}`;
  const configPath = join(tempDir, "tauri.smoke.json");
  writeFileSync(
    configPath,
    JSON.stringify({
      build: { devUrl: url, beforeDevCommand: `npm run dev -- --port ${devPort} --strictPort` },
    }),
  );
  markers.push(configPath, `--port ${devPort} --strictPort`, tempDir);

  mkdirSync(OUT_DIR, { recursive: true });
  const appLog = join(OUT_DIR, "app.log");
  // A file descriptor, not pipes: with pipes into this process, --keep-open children would die
  // of EPIPE when it exits.
  const logFd = openSync(appLog, "w");
  log(
    `launching app (dev ${devPort}, cdp ${cdpPort}, data ${dataDir}); first build can take minutes`,
  );
  const env = {
    ...process.env,
    SCT_SIM_PEDALS: "1",
    SCT_DATA_DIR: dataDir,
    WEBVIEW2_USER_DATA_FOLDER: webviewDir,
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${cdpPort}`,
  };
  delete env.TAURI_DEV_HOST;
  appProc = spawn(`npm run tauri dev -- --config "${configPath}"`, {
    cwd: ROOT,
    shell: true,
    env,
    stdio: ["ignore", logFd, logFd],
  });
  closeSync(logFd);
  appProc.on("exit", (code, signal) => {
    appDied = `tauri dev exited early (${code ?? signal}); see ${appLog}`;
    rejectAllPending(new Fatal(appDied));
  });

  const target = await waitFor(
    async () => {
      const list = await (await fetch(`http://127.0.0.1:${cdpPort}/json`)).json();
      return list.find((t) => t.type === "page" && new URL(t.url).origin === url);
    },
    { timeout: BUILD_TIMEOUT_MS, interval: 1000, what: "the app window (CDP page)" },
  );
  return { target, url };
}

// ---------------------------------------------------------------- CDP

let ws;
let msgId = 0;
const pending = new Map();
let devUrl;

function rejectAllPending(error) {
  for (const { rej, timer } of pending.values()) {
    clearTimeout(timer);
    rej(error);
  }
  pending.clear();
}

async function connect(target) {
  ws = new WebSocket(target.webSocketDebuggerUrl);
  await new Promise((res, rej) => {
    ws.addEventListener("open", res, { once: true });
    ws.addEventListener("error", () => rej(new Error("CDP websocket failed")), { once: true });
  });
  ws.addEventListener("message", (e) => {
    const m = JSON.parse(e.data);
    const call = pending.get(m.id);
    if (!call) return;
    pending.delete(m.id);
    clearTimeout(call.timer);
    if (m.error) call.rej(new Error(`${call.method}: ${m.error.message}`));
    else call.res(m.result);
  });
  const lost = () => rejectAllPending(new Fatal("lost the CDP connection to the app"));
  ws.addEventListener("close", lost);
  ws.addEventListener("error", lost);
  await send("Page.enable");
}

function send(method, params = {}) {
  return new Promise((res, rej) => {
    try {
      checkAlive();
    } catch (e) {
      rej(e);
      return;
    }
    const id = ++msgId;
    const timer = setTimeout(() => {
      pending.delete(id);
      rej(new Error(`${method}: no answer after ${CDP_CALL_TIMEOUT_MS} ms`));
    }, CDP_CALL_TIMEOUT_MS);
    pending.set(id, { res, rej, timer, method });
    try {
      ws.send(JSON.stringify({ id, method, params }));
    } catch (e) {
      pending.delete(id);
      clearTimeout(timer);
      rej(new Fatal(`CDP send failed: ${e.message}`));
    }
  });
}

/** Evaluates an expression in the page (awaiting promises) and returns its JSON value. */
async function ev(expression) {
  const r = await send("Runtime.evaluate", { expression, awaitPromise: true, returnByValue: true });
  if (r.exceptionDetails) {
    throw new Error(r.exceptionDetails.exception?.description ?? r.exceptionDetails.text);
  }
  return r.result.value;
}

const invoke = (cmd, args = {}) =>
  ev(`window.__TAURI_INTERNALS__.invoke(${JSON.stringify(cmd)}, ${JSON.stringify(args)})`);

/** Sets the simulated pedals to fractions 0..1 (raw axis fractions, not calibrated). */
const setPedals = (throttle, brake, clutch) =>
  invoke("set_sim_pedals", { values: [throttle, brake, clutch], auto: false });

/** Full page load of an app route, resolved once the page has rendered something. */
async function go(path) {
  // A marker on the old document: it is gone once the new one has replaced it.
  await ev(`window.__smokeNav = true`);
  await send("Page.navigate", { url: devUrl + path });
  await waitFor(
    () => ev(`window.__smokeNav === undefined && location.pathname === ${JSON.stringify(path)}`),
    { timeout: 20_000, what: `the browser to arrive on ${path}` },
  );
  await waitFor(
    () => ev(`document.readyState === "complete" && document.body.innerText.length > 0`),
    { timeout: 20_000, what: `page ${path} to load` },
  );
  // After a load, the simulated-pedals panel writes its own sliders (all 0) once it sees the
  // device: via requestAnimationFrame, then IPC. Wait until that write has been sent. Steps that
  // need exact pedal values also read them back (setPedalsVerified), in case it lands later.
  await waitFor(() => ev(`Boolean(document.querySelector("aside.sim-panel"))`), {
    timeout: 15_000,
    what: "the simulated pedals panel",
  });
  await ev(`new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(() => r(true))))`);
  await invoke("app_info");
}

const hasText = (needle) => ev(`document.body.innerText.includes(${JSON.stringify(needle)})`);

/** Clicks the first element matching `selector` whose text is exactly `text` (or any, if null). */
async function click(selector, text = null, scope = "document") {
  await waitFor(
    async () => {
      const r = await ev(`(() => {
        const root = ${scope};
        if (!root) return "scope";
        const els = [...root.querySelectorAll(${JSON.stringify(selector)})];
        const el = els.find((e) => ${text === null} || e.textContent.trim() === ${JSON.stringify(text)});
        if (!el || el.disabled) return "none";
        el.click();
        return "ok";
      })()`);
      return r === "ok";
    },
    { timeout: 5000, what: `an enabled ${selector} with text ${JSON.stringify(text)} in ${scope}` },
  );
}

/** Svelte selects bound to objects need selectedIndex plus a bubbling change event. */
async function selectOption(selectIndex, optionText) {
  const ok = await ev(`(() => {
    const sel = document.querySelectorAll(".picker-controls select")[${selectIndex}];
    if (!sel) return false;
    const i = [...sel.options].findIndex((o) => o.textContent.trim().startsWith(${JSON.stringify(optionText)}));
    if (i < 0) return false;
    sel.selectedIndex = i;
    sel.dispatchEvent(new Event("change", { bubbles: true }));
    return true;
  })()`);
  if (!ok)
    throw new Error(
      `select #${selectIndex} has no option starting with ${JSON.stringify(optionText)}`,
    );
}

const optionTexts = (selectIndex) =>
  ev(
    `[...(document.querySelectorAll(".picker-controls select")[${selectIndex}]?.options ?? [])].map((o) => o.textContent.trim())`,
  );

/** Calibrated 0..1 value of the pedal now in the live stream (dev server module instance). */
const livePedal = (pedal) =>
  ev(
    `import("/src/lib/pedals/stream.ts").then((m) => m.pedalStream.history.latest()?.${pedal} ?? null)`,
  );

/**
 * Sets the pedals (raw fractions) and polls the live stream until the calibrated values read
 * back within `tol` percentage points of `expected` ({ brake: 60, throttle: 25 }), twice with
 * a pause between, writing again whenever they don't. Needs a page that feeds the pedal stream.
 */
async function setPedalsVerified(values, expected, tol) {
  const matches = async () => {
    for (const [pedal, pct] of Object.entries(expected)) {
      const v = await livePedal(pedal);
      if (v === null || Math.abs(v * 100 - pct) > tol) return false;
    }
    return true;
  };
  await waitFor(
    async () => {
      await setPedals(...values);
      try {
        await waitFor(matches, { timeout: 1500, what: "pedal readback" });
      } catch (e) {
        if (e instanceof Fatal) throw e;
        return false;
      }
      await sleep(300);
      return matches();
    },
    { timeout: 15_000, interval: 50, what: `pedals reading ${JSON.stringify(expected)} (±${tol})` },
  );
}

// ---------------------------------------------------------------- steps

let stepNo = 0;
const results = [];

async function shot(name) {
  const res = await send("Page.captureScreenshot", { format: "png" });
  const file = join(OUT_DIR, `${String(stepNo).padStart(2, "0")}-${name}.png`);
  writeFileSync(file, Buffer.from(res.data, "base64"));
  return file;
}

async function step(name, fn) {
  stepNo += 1;
  const started = Date.now();
  let error = null;
  try {
    await fn();
  } catch (e) {
    error = e;
  }
  let file;
  try {
    file = await shot(name);
  } catch (e) {
    file = `(screenshot failed: ${e.message})`;
  }
  const secs = ((Date.now() - started) / 1000).toFixed(1);
  results.push({ name, ok: !error, secs, detail: error ? error.message.split("\n")[0] : "" });
  log(`${error ? "FAIL" : "PASS"}  ${String(stepNo).padStart(2, "0")} ${name} (${secs}s)`);
  if (error) log(`      ${error.message}\n      screenshot: ${file}`);
  // A fatal error ends the run here; the later steps would only fail the same way.
  if (error instanceof Fatal) throw error;
  // Leave the pedals released for the next step, whatever happened.
  await setPedals(0, 0, 0).catch(() => {});
}

function assert(cond, message) {
  if (!cond) throw new Error(message);
}

// ---- shared state between steps
let simDevice = null;
let profile = null;

/**
 * Raw sim fraction (0..1 of the axis) that the profile calibrates to `calibrated` (0..1): undoes
 * the deadzones, then the min..max range.
 */
function raw(pedal, calibrated) {
  const lo = profile?.[pedal]?.calibration.deadzoneLow ?? 0.02;
  const hi = profile?.[pedal]?.calibration.deadzoneHigh ?? 0.02;
  const min = profile?.[pedal]?.calibration.min ?? -32768;
  const max = profile?.[pedal]?.calibration.max ?? 32767;
  const rawValue = min + (lo + calibrated * (1 - lo - hi)) * (max - min);
  return (rawValue + 32768) / 65535;
}

const SIM_AXIS = { throttle: 0, brake: 1, clutch: 2 };

/** Reads what the drill screen shows right now. */
const drillSnap = () =>
  ev(`(() => {
    const t = (sel) => document.querySelector(sel)?.textContent.trim() ?? null;
    return {
      state: t(".status-badge"),
      rep: t(".rep-info h3"),
      grade: t(".score-card .grade"),
      total: t(".score-card .total"),
      finished: document.body.innerText.includes("Set Finished!"),
      summary: t(".summary-card"),
      pills: [...document.querySelectorAll(".rep-pill")].map((p) => p.textContent.trim()),
      error: t(".error-message"),
    };
  })()`);

/**
 * True when the trace canvas shows the accent now-line. Samples the canvas pixels at 25% of its
 * width (plus one pixel either side for subpixel offsets) at 30%, 50% and 70% of its height.
 * A pixel counts when each RGB channel is within 40 of --accent.
 */
const nowLineDrawn = () =>
  ev(`(() => {
    const canvas = document.querySelector('[data-testid="trace-view"] canvas');
    if (!canvas || canvas.width === 0 || canvas.height === 0) return false;
    const ctx = canvas.getContext("2d");
    const hex = getComputedStyle(document.documentElement).getPropertyValue("--accent").trim();
    const m = /^#([0-9a-f]{6})$/i.exec(hex);
    if (!m) return false;
    const n = parseInt(m[1], 16);
    const accent = [(n >> 16) & 255, (n >> 8) & 255, n & 255];
    const x = Math.round(canvas.width * 0.25);
    for (const dx of [0, -1, 1]) {
      for (const f of [0.3, 0.5, 0.7]) {
        const y = Math.round(canvas.height * f);
        const d = ctx.getImageData(x + dx, y, 1, 1).data;
        if (accent.every((c, i) => Math.abs(d[i] - c) <= 40)) return true;
      }
    }
    return false;
  })()`);

/** Opens /drill, waits for the live pedals and the preset list. */
async function openDrillPage() {
  await go("/drill");
  await waitFor(() => hasText(`Pedals Active: ${SIM_NAME}`), {
    timeout: 15_000,
    what: "the drill page to show the pedals as active",
  });
  await waitFor(async () => (await optionTexts(0)).length > 0, { what: "presets in the select" });
}

/** Starts the selected drill and waits for it to finish; returns every grade seen on the way. */
async function runSet({ timeout = 90_000 } = {}) {
  await click("button", "Start Drill");
  const grades = new Set();
  const states = new Set();
  let snap;
  await waitFor(
    async () => {
      snap = await drillSnap();
      if (snap.state) states.add(snap.state);
      if (snap.grade) grades.add(snap.grade);
      if (snap.error) throw new Fatal(`drill page error: ${snap.error}`);
      return snap.finished && snap.pills.length > 0;
    },
    { timeout, interval: 100, what: "the set to finish (FINISHED + Set Summary)" },
  );
  return { snap, grades: [...grades], states: [...states] };
}

const pillTotals = (pills) => pills.map((p) => Number(p.match(/:\s*(\d+)/)?.[1] ?? NaN));

// Grade thresholds (crates/core/src/scoring.rs): S >= 95, A >= 85, B >= 70, C >= 55, else D.

const loadProfileOf = (deviceId) => invoke("load_profile", { deviceId });

const bundledPresets = () =>
  readdirSync(join(ROOT, "presets"))
    .filter((f) => f.endsWith(".json"))
    .map((f) => JSON.parse(readFileSync(join(ROOT, "presets", f), "utf8")));

/** Drills on the brake or throttle: what the drill screen offers. */
const playableOf = (preset) => preset.drills.filter((d) => d.pedal !== "clutch");

function findDrill(presetId, drillId) {
  const preset = bundledPresets().find((p) => p.id === presetId);
  const drill = preset?.drills.find((d) => d.id === drillId);
  assert(drill, `preset ${presetId} / drill ${drillId} not found in presets/`);
  return drill;
}

/** Presses one pedal fully and releases it; true once the wizard has moved on from it. */
async function pressAndRelease(pedal) {
  const values = [0, 0, 0];
  values[SIM_AXIS[pedal]] = 1;
  // Let the detector take its resting baseline first.
  await sleep(600);
  await setPedals(...values);
  await sleep(700);
  await setPedals(0, 0, 0);
  try {
    await waitFor(
      () =>
        ev(`(() => {
          const strong = document.querySelector(".wizard .prompt strong");
          return !strong || strong.textContent.trim() !== ${JSON.stringify(pedal)};
        })()`),
      { timeout: 6000, what: `the wizard to accept the ${pedal}` },
    );
    return true;
  } catch (e) {
    if (e instanceof Fatal) throw e;
    return false;
  }
}

/** Sweeps the brake from the calibration panel, up to `peak` (raw fraction) and back. */
async function resweepBrake(peak) {
  const panel = `document.querySelector('[data-testid="calibration-brake"]')`;
  await click("button", "Recalibrate range", panel);
  await sleep(300);
  await setPedals(0, peak, 0);
  await sleep(500);
  await setPedals(0, 0, 0);
  await sleep(300);
  await click("button", "Done sweeping", panel);
}

async function main() {
  mkdirSync(OUT_DIR, { recursive: true });
  let target;
  if (NO_LAUNCH) {
    const cdp = option("--cdp", null);
    if (!cdp) throw new Fatal("--no-launch needs an explicit --cdp <port>");
    target = await waitFor(
      async () => {
        const list = await (await fetch(`http://127.0.0.1:${cdp}/json`)).json();
        return list.find((t) => t.type === "page");
      },
      { timeout: 15_000, interval: 500, what: `a page target on CDP port ${cdp}` },
    );
    devUrl = new URL(target.url).origin;
  } else {
    ({ target, url: devUrl } = await launchApp());
  }
  await connect(target);
  log(`attached to ${target.url}`);
  await waitFor(() => ev("Boolean(window.__TAURI_INTERNALS__)"), {
    timeout: 30_000,
    what: "the page to run inside Tauri",
  });
  if (!NO_LAUNCH) {
    // Proves the SCT_DATA_DIR override took effect: otherwise the user's real database is in use.
    await waitFor(() => existsSync(join(dataDir, "profiles.db")), {
      what: `${join(dataDir, "profiles.db")} (SCT_DATA_DIR override)`,
    }).catch((e) => {
      throw new Fatal(e.message);
    });
  }
  // Pedals released before anything starts.
  await setPedals(0, 0, 0);

  await step("devices-lists-simulated-pedals", async () => {
    await go("/devices");
    await waitFor(() => hasText(SIM_NAME), {
      timeout: 15_000,
      what: `${SIM_NAME} in the device list`,
    });
    const row = await ev(`(() => {
      const tr = [...document.querySelectorAll("tbody tr")].find((r) => r.textContent.includes(${JSON.stringify(SIM_NAME)}));
      return tr ? { badge: tr.querySelector(".simulated-badge")?.textContent.trim() ?? null, text: tr.textContent } : null;
    })()`);
    assert(row, "no table row for the simulated device");
    assert(
      row.badge === "Simulated",
      `badge is ${JSON.stringify(row.badge)}, expected "Simulated"`,
    );
    const snapshot = await invoke("list_devices");
    simDevice = snapshot.devices.find((d) => d.name === SIM_NAME);
    assert(simDevice?.simulated === true, "list_devices does not report the device as simulated");
    assert(simDevice.axisCount >= 3, `expected at least 3 axes, got ${simDevice.axisCount}`);

    // The run writes a profile and attempts, so it must start from empty data. Fatal: stop
    // before touching anything.
    if ((await loadProfileOf(simDevice.id)) !== null) {
      throw new Fatal("the simulated device already has a profile: not a fresh SCT_DATA_DIR");
    }
    for (const preset of bundledPresets()) {
      for (const d of preset.drills) {
        const found = await invoke("list_attempts", { drillId: d.id, limit: 1 }).catch((e) => {
          throw new Fatal(`cannot check attempts are empty: ${e.message}`);
        });
        if (found.length > 0) throw new Fatal(`attempts already exist for ${d.id}: not fresh data`);
      }
    }
  });

  await step("wizard-detect-and-calibrate", async () => {
    assert(simDevice, "previous step found no device");
    // The machine may have real devices too: press the button in the simulated row only.
    await click(
      "button",
      "Show axes",
      `[...document.querySelectorAll("tbody tr")].find((r) => r.textContent.includes(${JSON.stringify(SIM_NAME)}))`,
    );
    await waitFor(() => hasText("Detect pedals"), { what: "the Detect pedals button" });
    await click("button", "Detect pedals");

    for (const pedal of ["brake", "throttle", "clutch"]) {
      await waitFor(
        () =>
          ev(
            `document.querySelector(".wizard .prompt strong")?.textContent.trim() === ${JSON.stringify(pedal)}`,
          ),
        { what: `the wizard to ask for the ${pedal}` },
      );
      let accepted = false;
      for (let attempt = 0; attempt < 3 && !accepted; attempt++) {
        accepted = await pressAndRelease(pedal);
      }
      assert(accepted, `the wizard did not accept the ${pedal} after 3 presses`);
    }
    await waitFor(() => hasText("Done. Check the assignments below."), {
      what: "the wizard to finish",
    });
    await waitFor(() => hasText("Profile saved"), { what: "Profile saved" });

    profile = await loadProfileOf(simDevice.id);
    assert(profile, "no profile stored for the simulated device");
    for (const [pedal, axis] of Object.entries(SIM_AXIS)) {
      assert(
        profile[pedal]?.axis === axis,
        `${pedal} should be on axis ${axis}, got ${JSON.stringify(profile[pedal])}`,
      );
      const { min, max } = profile[pedal].calibration;
      assert(min <= -32000 && max >= 32000, `${pedal} range ${min}..${max} is not the full sweep`);
    }

    // Re-sweep the brake to 80% of its travel: the stored range must follow, and a sweep that
    // did nothing would leave the full range behind.
    await waitFor(
      () => ev(`Boolean(document.querySelector('[data-testid="calibration-brake"]'))`),
      { what: "the calibration panel" },
    );
    await resweepBrake(0.8);
    const wantMax = -32768 + 0.8 * 65535;
    const partial = await waitFor(
      async () => {
        const p = await loadProfileOf(simDevice.id);
        return Math.abs(p.brake.calibration.max - wantMax) <= 1500 ? p : null;
      },
      { what: `the brake range to follow the sweep (max about ${Math.round(wantMax)})` },
    );
    assert(partial.brake.calibration.min <= -32000, "brake min moved after the partial sweep");
    assert(
      !(await ev(`Boolean(document.querySelector(".calibration .error"))`)),
      "calibration panel shows an error",
    );
    // And back to the full range, which the later steps rely on.
    await waitFor(() => hasText("Recalibrate range"), { what: "the sweep to end" });
    await resweepBrake(1);
    profile = await waitFor(
      async () => {
        const p = await loadProfileOf(simDevice.id);
        return p.brake.calibration.max >= 32000 ? p : null;
      },
      { what: "the brake range to be full again" },
    );
    assert(
      !(await ev(`Boolean(document.querySelector(".calibration .error"))`)),
      "calibration panel shows an error",
    );
    assert(
      (await invoke("profiled_devices")).includes(simDevice.id),
      "device is not in profiled_devices",
    );
  });

  await step("live-view-shows-pedal-values", async () => {
    await go("/live");
    await waitFor(
      () =>
        ev(
          `document.querySelector('[data-testid="source-status"]')?.textContent.includes(${JSON.stringify(SIM_NAME)})`,
        ),
      { timeout: 15_000, what: "the live view to connect to the simulated pedals" },
    );
    assert(
      await ev(`Boolean(document.querySelector('canvas[aria-label^="Live brake"]'))`),
      "no pedal bars canvas",
    );
    assert(!(await hasText("No pedals set up")), "live view shows the no-profile overlay");
    // The bars are drawn on a canvas, so the numbers come from the stream the canvas draws.
    await setPedalsVerified(
      [raw("throttle", 0.25), raw("brake", 0.6), 0],
      { throttle: 25, brake: 60 },
      3,
    );
  });

  await step("hold-drill-in-band", async () => {
    await openDrillPage();
    await selectOption(0, "Sample");
    await waitFor(async () => (await optionTexts(1)).some((t) => t.startsWith("Brake hold 70%")), {
      what: "the brake hold drill",
    });
    await selectOption(1, "Brake hold 70%");
    await waitFor(() => hasText("Target:"), { what: "drill details" });
    const drill = findDrill("sample", "brake-hold-70");
    await setPedalsVerified([0, raw("brake", drill.target / 100), 0], { brake: drill.target }, 0.5);
    const { snap, grades, states } = await runSet();
    log(
      `      states ${states.join(",")} grades seen ${grades.join(",")} pills ${snap.pills.join(" ")}`,
    );
    assert(
      snap.pills.length === drill.reps,
      `expected ${drill.reps} reps, summary has ${snap.pills.length}`,
    );
    assert(snap.summary.includes("Set Summary"), "no Set Summary");
    assert(
      grades.length > 0 && grades.every((g) => g === "S"),
      `rep grades ${grades.join(",")} are not all S`,
    );
    const totals = pillTotals(snap.pills);
    assert(
      totals.every((t) => t >= 95),
      `rep totals ${totals.join(",")} are not all >= 95`,
    );
    const summaryGrade = snap.summary.match(/Average:\s*\d+\s*\((\w)\)/)?.[1];
    assert(summaryGrade === "S", `set grade ${summaryGrade}, expected S`);

    // The drill page saves the finished set. The save is async, so poll for it.
    const saved = await waitFor(
      async () => {
        const attempts = await invoke("list_attempts", { drillId: drill.id, limit: 5 });
        return attempts.length > 0 ? attempts : null;
      },
      { what: `the attempt for ${drill.id} to be saved` },
    ).then((attempts) => {
      assert(attempts.length === 1, `expected 1 saved attempt, found ${attempts.length}`);
      return attempts[0];
    });
    assert(saved.presetId === "sample", `saved attempt is for preset ${saved.presetId}`);
    assert(!saved.aborted, "saved attempt is marked aborted");
    assert(
      saved.reps.length === drill.reps,
      `saved attempt has ${saved.reps.length} reps, expected ${drill.reps}`,
    );
    assert(
      saved.best !== null && saved.best >= 95,
      `saved attempt best ${saved.best}, expected >= 95`,
    );
    log(
      `      attempt saved: id ${saved.id}, ${saved.reps.length} reps, best ${saved.best.toFixed(1)}`,
    );
  });

  await step("hold-drill-out-of-band", async () => {
    await click("button", "Pick Another Drill");
    await waitFor(() => hasText("Select a Drill"), { what: "the drill picker" });
    const drill = findDrill("sample", "brake-hold-70");
    const off = drill.target - 30;
    await setPedalsVerified([0, raw("brake", off / 100), 0], { brake: off }, 0.5);
    const { snap, grades } = await runSet();
    log(`      grades seen ${grades.join(",")} pills ${snap.pills.join(" ")}`);
    assert(
      snap.pills.length === drill.reps,
      `expected ${drill.reps} reps, summary has ${snap.pills.length}`,
    );
    assert(
      grades.length > 0 && grades.every((g) => g === "D"),
      `rep grades ${grades.join(",")} are not all D`,
    );
    const totals = pillTotals(snap.pills);
    assert(
      totals.every((t) => t < 55),
      `rep totals ${totals.join(",")} are not all < 55`,
    );
  });

  await step("trace-drill-runs", async () => {
    await click("button", "Pick Another Drill");
    await waitFor(() => hasText("Select a Drill"), { what: "the drill picker" });
    await selectOption(0, "Sample");
    await waitFor(async () => (await optionTexts(1)).some((t) => t.startsWith("Hairpin trace")), {
      what: "the hairpin trace drill",
    });
    await selectOption(1, "Hairpin trace");
    await waitFor(() => hasText("Duration:"), { what: "trace drill details" });
    const drill = findDrill("sample", "hairpin");
    const traceMode = () =>
      ev(`document.querySelector('[data-testid="trace-view"]')?.dataset.mode ?? null`);
    await click("button", "Ghost", `document.querySelector(".view-toggle")`);
    assert(
      (await ev(`localStorage.getItem("sct:trace_view")`)) === "ghost",
      "sct:trace_view is not ghost after clicking Ghost",
    );
    await setPedals(0, 0, 0);
    await click("button", "Start Drill");
    // The last second of the lead-in shows GO before the rep starts.
    await waitFor(
      () => ev(`document.querySelector(".countdown-number")?.textContent.trim() === "GO"`),
      { timeout: 15_000, interval: 50, what: "GO in the countdown overlay" },
    );
    assert((await traceMode()) === "ghost", "trace view is not in ghost mode at GO");
    await waitFor(nowLineDrawn, { timeout: 5_000, what: "the ghost now-line on the canvas" });
    await waitFor(
      () =>
        ev(`(() => {
          const view = document.querySelector('[data-testid="trace-view"]');
          const hud = document.querySelector('[data-testid="trace-hud"]');
          return Boolean(view && hud && /Target\\s*\\d+%/.test(hud.textContent));
        })()`),
      { timeout: 15_000, what: "trace-view and trace-hud with target percentage" },
    );
    const grades = new Set();
    const states = new Set();
    let snap;
    // The rep heading moves to "Rep 2 / N" once rep 1 is scored. Switch to Playhead then,
    // while the set keeps running.
    await waitFor(
      async () => {
        snap = await drillSnap();
        if (snap.state) states.add(snap.state);
        if (snap.grade) grades.add(snap.grade);
        if (snap.error) throw new Fatal(`drill page error: ${snap.error}`);
        return snap.rep?.startsWith("Rep 2 /");
      },
      { timeout: 90_000, interval: 100, what: "the second trace rep (Rep 2 heading)" },
    );
    await click("button", "Playhead", `document.querySelector(".rep-view-row")`);
    assert(
      (await traceMode()) === "playhead",
      "trace view is not in playhead mode after clicking Playhead",
    );
    assert(!(await drillSnap()).finished, "the set finished before the switch to Playhead");
    assert(
      (await ev(`localStorage.getItem("sct:trace_view")`)) === "playhead",
      "sct:trace_view is not playhead after clicking Playhead",
    );
    log(`      views: ghost rep 1, playhead from rep 2`);
    await waitFor(
      async () => {
        snap = await drillSnap();
        if (snap.state) states.add(snap.state);
        if (snap.grade) grades.add(snap.grade);
        if (snap.error) throw new Fatal(`drill page error: ${snap.error}`);
        return snap.finished && snap.pills.length > 0;
      },
      { timeout: 90_000, interval: 100, what: "the trace set to finish" },
    );
    log(`      grades seen ${[...grades].join(",")} pills ${snap.pills.join(" ")}`);
    assert(
      snap.pills.length === drill.reps,
      `expected ${drill.reps} reps, summary has ${snap.pills.length}`,
    );
  });

  await step("lead-in-drill-runs", async () => {
    await click("button", "Pick Another Drill");
    await waitFor(() => hasText("Select a Drill"), { what: "the drill picker" });
    await selectOption(0, "Sample");
    await waitFor(
      async () => (await optionTexts(1)).some((t) => t.startsWith("Hairpin from the throttle")),
      { what: "the hairpin from the throttle drill" },
    );
    await selectOption(1, "Hairpin from the throttle");
    await waitFor(() => hasText("Duration:"), { what: "trace drill details" });
    const drill = findDrill("sample", "hairpin-from-throttle");
    assert(
      drill.reps > 0 && drill.throttleLeadIn?.level === 80 && drill.throttleLeadIn?.holdMs === 1500,
      "unexpected drill config",
    );

    await waitFor(() => ev(`Boolean(document.querySelector('[data-testid="lead-in-info"]'))`), {
      timeout: 5000,
      what: "lead-in-info element",
    });
    const infoText = await ev(
      `document.querySelector('[data-testid="lead-in-info"]')?.textContent.replace(/\\s+/g, " ") ?? ""`,
    );
    assert(
      infoText.includes("Starts from throttle:"),
      `lead-in-info missing expected text, got ${JSON.stringify(infoText)}`,
    );
    assert(
      infoText.includes("before the brake point"),
      `lead-in-info missing expected text, got ${JSON.stringify(infoText)}`,
    );

    await setPedals(0, 0, 0);
    await click("button", "Start Drill");

    // Wait for countdown text THROTTLE
    await waitFor(
      () => ev(`document.querySelector(".countdown-number")?.textContent.trim() === "THROTTLE"`),
      { timeout: 15_000, interval: 50, what: "THROTTLE in the countdown overlay" },
    );

    // Then wait for [data-testid="throttle-cue"] with text containing Press the throttle
    await waitFor(
      () =>
        ev(
          `document.querySelector('[data-testid="throttle-cue"]')?.textContent.includes("Press the throttle")`,
        ),
      { timeout: 15_000, interval: 50, what: "throttle cue with Press the throttle" },
    );

    // setPedals(0.9, 0, 0) and wait for the cue to show a decimal seconds number
    await setPedals(0.9, 0, 0);
    const liveThr = await livePedal("throttle");
    if (liveThr !== null && liveThr < 0.7) {
      log("      calibrated throttle at 0.9 was under 70%, raising to 1.0");
      await setPedals(1.0, 0, 0);
    }

    await waitFor(
      () =>
        ev(`(() => {
          const cue = document.querySelector('[data-testid="throttle-cue"]');
          return Boolean(cue && /\\d+\\.\\d+/.test(cue.textContent));
        })()`),
      { timeout: 15_000, interval: 50, what: "decimal seconds number in throttle cue" },
    );
    await shot("lead-in-throttle-hold");

    // Wait for lift cue banner
    await waitFor(() => ev(`Boolean(document.querySelector('[data-testid="lift-cue"]'))`), {
      timeout: 15_000,
      interval: 50,
      what: "the lift cue banner",
    });
    await shot("lead-in-lift");

    // Play coast technique: release throttle, brief gap, then brake
    await setPedals(0, 0, 0);
    await sleep(150);
    await setPedals(0, 0.7, 0);

    // Wait for [data-testid="overlap"] and assert coast > 0 ms
    const overlapText = await waitFor(
      async () => {
        const text = await ev(
          `document.querySelector('[data-testid="overlap"]')?.textContent ?? null`,
        );
        return text;
      },
      { timeout: 15_000, interval: 50, what: "the overlap block to appear" },
    );
    const coastMatch = overlapText.match(/Coast\s*(\d+)\s*ms/);
    assert(coastMatch, `could not parse coast ms from ${JSON.stringify(overlapText)}`);
    const coastMs = Number(coastMatch[1]);
    assert(coastMs > 0, `expected coast > 0 ms, got ${coastMs}`);
    log(`      coast: ${coastMs} ms`);

    // Abort Set and wait for finished overlay
    await click("button", "Abort Set");
    await waitFor(async () => (await drillSnap()).finished, {
      timeout: 15_000,
      what: "the finished overlay after abort",
    });
  });

  await step("abort-and-play-again", async () => {
    await click("button", "Pick Another Drill");
    await waitFor(() => hasText("Select a Drill"), { what: "the drill picker" });
    await selectOption(1, "Brake hold 70%");
    const drill = findDrill("sample", "brake-hold-70");
    await setPedals(0, 0, 0);
    // Abort during the countdown: nothing was scored.
    await click("button", "Start Drill");
    await waitFor(async () => (await drillSnap()).state === "COUNTDOWN", { what: "COUNTDOWN" });
    await click("button", "Abort Set");
    await waitFor(async () => (await drillSnap()).finished, {
      timeout: 15_000,
      what: "the aborted set to finish",
    });
    const afterCountdown = await drillSnap();
    assert(afterCountdown.state === "FINISHED", "state is not FINISHED after abort");
    assert(
      afterCountdown.summary?.includes("No scored reps."),
      `expected "No scored reps." after a countdown abort, got ${JSON.stringify(afterCountdown.summary)}`,
    );
    // Play Again starts a fresh run, which is then aborted while ACTIVE.
    await click("button", "Play Again");
    await waitFor(async () => (await drillSnap()).state === "COUNTDOWN", {
      what: "COUNTDOWN after Play Again",
    });
    await waitFor(async () => (await drillSnap()).state === "ACTIVE", {
      timeout: 15_000,
      what: "ACTIVE",
    });
    await click("button", "Abort Set");
    await waitFor(async () => (await drillSnap()).finished, {
      timeout: 15_000,
      what: "the second aborted set to finish",
    });
    const afterActive = await drillSnap();
    assert(
      afterActive.pills.length < drill.reps,
      `an abort in the first rep left ${afterActive.pills.length} of ${drill.reps} reps scored`,
    );
    // The countdown abort saved nothing. The second abort saved a set only if a rep ended
    // first; with the in-band and out-of-band sets that makes 2 or 3 attempts.
    const expected = 2 + (afterActive.pills.length > 0 ? 1 : 0);
    await waitFor(
      async () => {
        const list = await invoke("list_attempts", { drillId: drill.id, limit: 10 });
        return list.length >= expected ? list : null;
      },
      { what: `${expected} saved attempts` },
    );
    await new Promise((r) => setTimeout(r, 300));
    const settled = await invoke("list_attempts", { drillId: drill.id, limit: 10 });
    assert(
      settled.length === expected,
      `expected ${expected} saved attempts after the aborts, found ${settled.length}`,
    );
    const abortedCount = settled.filter((a) => a.aborted).length;
    assert(
      abortedCount === expected - 2,
      `${abortedCount} saved attempts are aborted, expected ${expected - 2}`,
    );
    log(`      attempts saved: ${settled.length}, aborted: ${abortedCount}`);
    await click("button", "Pick Another Drill");
    await waitFor(() => hasText("Select a Drill"), { what: "the drill picker again" });
    assert(
      await ev(
        `[...document.querySelectorAll("button")].some((b) => b.textContent.trim() === "Start Drill")`,
      ),
      "no Start Drill button after returning to the picker",
    );
  });

  await step("presets-and-drills-selectable", async () => {
    if (!(await hasText("Select a Drill"))) await openDrillPage();
    const bundled = bundledPresets();
    assert(bundled.length > 0, "no presets/*.json found");
    const playable = bundled.filter((p) => playableOf(p).length > 0);
    const listed = await optionTexts(0);
    for (const p of playable) {
      assert(
        listed.includes(p.name),
        `preset ${JSON.stringify(p.name)} missing from the select (has ${listed.join(", ")})`,
      );
    }
    for (const p of playable) {
      await selectOption(0, p.name);
      const drills = playableOf(p);
      const expectedNames = drills.map((d) => `${d.name} (${d.reps} reps)`);
      await waitFor(
        async () => JSON.stringify(await optionTexts(1)) === JSON.stringify(expectedNames),
        { what: `drill list of ${p.name}: ${expectedNames.join(" | ")}` },
      );
      for (const [i, d] of drills.entries()) {
        await selectOption(1, d.name);
        await waitFor(
          () =>
            ev(`(() => {
              const info = document.querySelector(".drill-info")?.innerText ?? "";
              const kind = ${JSON.stringify(d.type === "trace" ? "Duration:" : `Target: ${d.target.toFixed(d.decimals ?? 0)}%`)};
              return info.includes("Target Pedal: ${d.pedal}") && info.includes(kind) && info.includes("Reps: ${d.reps}");
            })()`),
          { what: `details of ${p.name} / ${d.name} (index ${i})` },
        );
      }
    }
    log(
      `      presets: ${playable.map((p) => p.id).join(", ")}; without playable drills: ${
        bundled
          .filter((p) => !playable.includes(p))
          .map((p) => p.id)
          .join(", ") || "none"
      }`,
    );
  });

  await step("home-picker-opens-drill", async () => {
    await go("/");
    await waitFor(() => ev(`document.querySelectorAll(".preset-card").length > 0`), {
      what: "preset cards on the home page",
    });
    const playable = bundledPresets().filter((p) => playableOf(p).length > 0);
    const cardCount = await ev(`document.querySelectorAll(".preset-card").length`);
    assert(
      cardCount === playable.length,
      `home lists ${cardCount} presets, expected ${playable.length}`,
    );
    const names = await ev(
      `[...document.querySelectorAll(".preset-name")].map((e) => e.textContent.trim())`,
    );
    for (const p of playable) {
      assert(
        names.includes(p.name),
        `preset ${JSON.stringify(p.name)} missing on home (has ${names.join(", ")})`,
      );
    }

    // Card of a preset by its name, and the card's list of drills and scores.
    const cardOf = (name) =>
      `[...document.querySelectorAll("li.preset-card")].find((li) => li.querySelector(".preset-name")?.textContent.trim() === ${JSON.stringify(name)})`;
    const headerOf = (name) => `(${cardOf(name)})?.querySelector("button.preset-header")`;

    // The earlier steps saved attempts for "sample" only, so it is the preset whose scores show.
    const P = playable.find((p) => p.id === "sample");
    assert(
      P,
      `bundled preset "sample" is not playable (playable: ${playable.map((p) => p.id).join(", ")})`,
    );
    await waitFor(() => ev(`Boolean(${headerOf(P.name)})`), { what: `the ${P.name} card` });
    // Clicking an open card collapses it, so only click a closed one.
    const wasExpanded = await ev(`${headerOf(P.name)}.getAttribute("aria-expanded")`);
    if (wasExpanded !== "true") {
      await ev(`${headerOf(P.name)}.click()`);
    }
    await waitFor(() => ev(`(${headerOf(P.name)})?.getAttribute("aria-expanded") === "true"`), {
      what: `${P.name} to open`,
    });
    const drillNames = playableOf(P).map((d) => d.name);
    await waitFor(
      async () =>
        JSON.stringify(
          await ev(
            `[...(${cardOf(P.name)}).querySelectorAll(".drill-name")].map((e) => e.textContent.trim())`,
          ),
        ) === JSON.stringify(drillNames),
      { what: `drill list of ${P.name}: ${drillNames.join(" | ")}` },
    );
    // Earlier steps saved sets, so the expected text comes from the stored attempts.
    const expectedScores = [];
    for (const d of playableOf(P)) {
      const bests = (await invoke("list_attempts", { drillId: d.id, limit: 100 }))
        .filter((a) => a.presetId === P.id && a.best !== null)
        .map((a) => a.best);
      expectedScores.push(bests.length > 0 ? `Best ${Math.round(Math.max(...bests))}` : "Best —");
    }
    assert(
      expectedScores.some((s) => s !== "Best —"),
      "no saved attempt for this preset, so the best scores go untested",
    );
    // The scores load after the drill list, so poll for them.
    let scores = [];
    await waitFor(
      async () => {
        scores = await ev(
          `[...(${cardOf(P.name)}).querySelectorAll(".drill-score")].map((e) => e.textContent.trim())`,
        );
        return JSON.stringify(scores) === JSON.stringify(expectedScores);
      },
      { what: `drill scores ${expectedScores.join(" | ")}` },
    ).catch((e) => {
      throw new Error(`${e.message} (shown: ${scores.join(" | ")})`);
    });
    log(`      scores: ${scores.join(" | ")}`);
    await shot("home-picker");

    // The last playable drill, not the first, so the selection is really tested.
    const D = playableOf(P).at(-1);
    await ev(
      `[...(${cardOf(P.name)}).querySelectorAll("button.drill")].find((b) => b.querySelector(".drill-name")?.textContent.trim() === ${JSON.stringify(D.name)}).click()`,
    );
    const query = `preset=${encodeURIComponent(P.id)}`;
    const drillQuery = `drill=${encodeURIComponent(D.id)}`;
    await waitFor(
      () =>
        ev(
          `location.pathname.endsWith("/drill") && location.search.includes(${JSON.stringify(query)}) && location.search.includes(${JSON.stringify(drillQuery)})`,
        ),
      { what: `the drill screen for ${P.id} / ${D.id}` },
    );
    await waitFor(
      async () => {
        const sel = await ev(`[0, 1].map((i) => {
          const s = document.querySelectorAll(".picker-controls select")[i];
          return s?.options[s.selectedIndex]?.textContent.trim() ?? null;
        })`);
        return sel[0] === P.name && sel[1] === `${D.name} (${D.reps} reps)`;
      },
      { what: `preset ${P.name} and drill ${D.name} selected` },
    );
    await shot("drill-from-picker");

    // The last preset is remembered: the home page opens it again.
    await go("/");
    await waitFor(() => ev(`(${headerOf(P.name)})?.getAttribute("aria-expanded") === "true"`), {
      what: `${P.name} to be open again on the home page`,
    });
  });

  await step("warm-up-with-skips", async () => {
    const samplePreset = bundledPresets().find((p) => p.id === "sample");
    assert(samplePreset, "bundled preset 'sample' not found");
    const warmUpSteps = samplePreset.warmUp?.steps ?? [];
    assert(
      warmUpSteps.length === 4,
      `expected 4 warm-up steps in sample, got ${warmUpSteps.length}`,
    );
    const sampleName = samplePreset.name;

    await go("/");
    const cardOf = (name) =>
      `[...document.querySelectorAll("li.preset-card")].find((li) => li.querySelector(".preset-name")?.textContent.trim() === ${JSON.stringify(name)})`;
    const headerOf = (name) => `(${cardOf(name)})?.querySelector("button.preset-header")`;

    await waitFor(() => ev(`Boolean(${headerOf(sampleName)})`), {
      what: `the ${sampleName} card`,
    });
    const wasExpanded = await ev(`${headerOf(sampleName)}.getAttribute("aria-expanded")`);
    if (wasExpanded !== "true") {
      await ev(`${headerOf(sampleName)}.click()`);
    }
    await waitFor(() => ev(`(${headerOf(sampleName)})?.getAttribute("aria-expanded") === "true"`), {
      what: `${sampleName} to open`,
    });

    await click('[data-testid="warm-up-start"]', null, cardOf(sampleName));
    await waitFor(
      () => ev(`location.search.includes("preset=sample") && location.search.includes("warmup=1")`),
      { what: "location.search to contain preset=sample and warmup=1" },
    );
    await waitFor(() => hasText(`Warm-up: ${sampleName}`), {
      what: `the text "Warm-up: ${sampleName}"`,
    });
    await waitFor(() => hasText("Drill 1 of 4"), { what: 'the text "Drill 1 of 4"' });
    await waitFor(() => hasText(`Pedals Active: ${SIM_NAME}`), {
      timeout: 15_000,
      what: "the drill page to show the pedals as active",
    });

    const drill1 = findDrill("sample", warmUpSteps[0].drill);
    await setPedalsVerified(
      [0, raw("brake", drill1.target / 100), 0],
      { brake: drill1.target },
      0.5,
    );
    await click("button", "Start Drill");
    let snap;
    await waitFor(
      async () => {
        snap = await drillSnap();
        if (snap.error) throw new Fatal(`drill page error: ${snap.error}`);
        const nextEnabled = await ev(`(() => {
          const btn = [...document.querySelectorAll("button")].find((b) => b.textContent.trim() === "Next Drill");
          return Boolean(btn && !btn.disabled);
        })()`);
        return snap.finished && nextEnabled;
      },
      { timeout: 120_000, interval: 200, what: '"Set Finished!" and enabled "Next Drill" button' },
    );
    assert(
      snap.pills.length === warmUpSteps[0].reps,
      `expected ${warmUpSteps[0].reps} reps, summary has ${snap.pills.length}`,
    );
    await setPedalsVerified([0, raw("brake", 0), 0], { brake: 0 }, 0.5);
    await click("button", "Next Drill");

    await waitFor(() => hasText("Drill 2 of 4"), { timeout: 15_000, what: '"Drill 2 of 4"' });
    await click('[data-testid="warm-up-skip"]');

    await waitFor(() => hasText("Drill 3 of 4"), { timeout: 15_000, what: '"Drill 3 of 4"' });
    await click("button", "Start Drill");
    await waitFor(
      async () => {
        const s = await drillSnap();
        if (s.error) throw new Fatal(`drill page error: ${s.error}`);
        return s.rep?.startsWith("Rep 2 /");
      },
      {
        timeout: 30_000,
        interval: 100,
        what: 'second rep to start (.rep-info h3 reads "Rep 2 /")',
      },
    );
    await click('[data-testid="warm-up-skip"]');
    await waitFor(() => hasText("Drill 4 of 4"), { timeout: 15_000, what: '"Drill 4 of 4"' });

    await click('[data-testid="warm-up-skip"]');

    await waitFor(
      async () => {
        const summary = await ev(
          `Boolean(document.querySelector('[data-testid="warm-up-summary"]'))`,
        );
        const saved = await hasText("Saved.");
        const s = await drillSnap();
        if (s.error) throw new Fatal(`drill page error: ${s.error}`);
        return summary && saved;
      },
      { timeout: 15_000, what: 'warm-up summary with "Saved."' },
    );
    const rows = await ev(
      `[...document.querySelectorAll(".warm-up-table tbody tr")].map((tr) => [...tr.querySelectorAll("td")].map((td) => td.textContent.trim()))`,
    );
    assert(rows.length === 4, `expected 4 rows in warm-up summary, got ${rows.length}`);
    const row1Score = Number(rows[0][1]);
    assert(
      !Number.isNaN(row1Score) && row1Score >= 90,
      `expected row 1 score to be a number >= 90, got ${rows[0][1]}`,
    );
    for (let i = 1; i < 4; i++) {
      assert(
        rows[i][1] === "Skipped",
        `expected row ${i + 1} to read "Skipped", got ${JSON.stringify(rows[i][1])}`,
      );
    }
    const shownScoreText = await ev(
      `document.querySelector('[data-testid="warm-up-score"]')?.textContent.trim()`,
    );
    const shownScore = Number(shownScoreText);
    assert(
      !Number.isNaN(shownScore),
      `expected [data-testid="warm-up-score"] to be a number, got ${JSON.stringify(shownScoreText)}`,
    );
    assert(
      Math.abs(shownScore - Math.round(row1Score / 4)) <= 1,
      `overall score ${shownScore} does not match Math.round(${row1Score} / 4) within ±1`,
    );
    await shot("warm-up-summary");

    const runs = await waitFor(
      async () => {
        const r = await invoke("list_warm_up_runs", { presetId: "sample", limit: 5 });
        return Array.isArray(r) && r.length === 1 ? r : null;
      },
      { timeout: 5000, what: "saved warm-up run in database" },
    );
    const run = runs[0];
    assert(
      Array.isArray(run.steps) && run.steps.length === 4,
      `expected 4 steps in saved run, got ${run.steps?.length}`,
    );
    assert(
      run.steps[0].skipped === false,
      `expected step 0 skipped: false, got ${run.steps[0].skipped}`,
    );
    assert(
      typeof run.steps[0].attemptId === "number",
      `expected step 0 attemptId to be a number, got ${run.steps[0].attemptId}`,
    );
    assert(
      typeof run.steps[0].score === "number" && run.steps[0].score >= 90,
      `expected step 0 score >= 90, got ${run.steps[0].score}`,
    );

    assert(
      run.steps[1].skipped === true,
      `expected step 1 skipped: true, got ${run.steps[1].skipped}`,
    );
    assert(
      run.steps[1].attemptId === null,
      `expected step 1 attemptId: null, got ${run.steps[1].attemptId}`,
    );

    assert(
      run.steps[2].skipped === true,
      `expected step 2 skipped: true, got ${run.steps[2].skipped}`,
    );
    assert(
      typeof run.steps[2].attemptId === "number",
      `expected step 2 attemptId to be a number, got ${run.steps[2].attemptId}`,
    );

    assert(
      run.steps[3].skipped === true,
      `expected step 3 skipped: true, got ${run.steps[3].skipped}`,
    );
    assert(
      run.steps[3].attemptId === null,
      `expected step 3 attemptId: null, got ${run.steps[3].attemptId}`,
    );

    assert(
      Math.abs(run.score - run.steps[0].score / 4) <= 0.01,
      `run score ${run.score} does not match steps[0].score / 4 (${run.steps[0].score / 4}) within 0.01`,
    );

    const hairpinAttempts = await invoke("list_attempts", { drillId: "hairpin", limit: 20 });
    const hairpinAttempt = hairpinAttempts.find((a) => a.id === run.steps[2].attemptId);
    assert(
      hairpinAttempt,
      `hairpin attempt with id ${run.steps[2].attemptId} not found in list_attempts`,
    );
    assert(
      hairpinAttempt.aborted === true,
      `expected hairpin attempt to have aborted: true, got ${hairpinAttempt.aborted}`,
    );

    log(
      `      warm-up row scores: ${rows.map((r) => r[1]).join(", ")}; overall: ${shownScore} (run score: ${run.score.toFixed(1)})`,
    );
  });
}

// ---------------------------------------------------------------- run

function printSummaryAndExit(code) {
  log("\nSummary");
  for (const [i, r] of results.entries()) {
    log(
      `${r.ok ? "PASS" : "FAIL"}  ${String(i + 1).padStart(2, "0")}  ${r.name.padEnd(36)} ${r.secs.padStart(6)}s  ${r.detail}`,
    );
  }
  const failed = results.filter((r) => !r.ok).length;
  log(`\n${results.length - failed}/${results.length} passed. Screenshots: ${OUT_DIR}`);
  process.exit(code ?? (failed === 0 ? 0 : 1));
}

const watchdog = setTimeout(() => {
  results.push({ name: "watchdog", ok: false, secs: "0", detail: "run exceeded 25 minutes" });
  log("FAIL  watchdog: the run exceeded 25 minutes");
  printSummaryAndExit(1);
}, WATCHDOG_MS);

try {
  await main();
} catch (e) {
  results.push({ name: "run-aborted", ok: false, secs: "0", detail: e.message });
  log(`FAIL  run aborted: ${e instanceof Fatal ? e.message : (e.stack ?? e.message)}`);
}
clearTimeout(watchdog);

if (KEEP_OPEN && !NO_LAUNCH && appRunning()) {
  const failed = results.filter((r) => !r.ok).length;
  log(`\n${results.length - failed}/${results.length} passed. Screenshots: ${OUT_DIR}`);
  log(`app left running (temp dir ${tempDir}); press Ctrl-C to stop it and clean up`);
  setInterval(() => {}, 1 << 30);
  await new Promise(() => {});
}
printSummaryAndExit();
