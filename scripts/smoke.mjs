// End-to-end smoke test: launches the real Tauri app with the simulated pedals and drives its
// WebView2 over the Chrome DevTools Protocol. Run with `npm run smoke`.
//
//   --keep-open        leave the app running afterwards
//   --out <dir>        screenshot directory (default: smoke-out/)
//   --no-launch --cdp <port>   attach to an app that is already running (it must have been
//                      started with SCT_SIM_PEDALS=1 and a fresh SCT_DATA_DIR)
import { spawn, spawnSync } from "node:child_process";
import { createServer } from "node:net";
import { mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
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
  return i >= 0 ? argv[i + 1] : fallback;
};
const KEEP_OPEN = flag("--keep-open");
const NO_LAUNCH = flag("--no-launch");
const OUT_DIR = resolve(ROOT, option("--out", "smoke-out"));
const BUILD_TIMEOUT_MS = 20 * 60_000;

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const log = (...args) => console.log(...args);

/** Polls `predicate` until it returns a truthy value; throws with `what` on timeout. */
async function waitFor(predicate, { timeout = 10_000, interval = 100, what = "condition" } = {}) {
  const deadline = Date.now() + timeout;
  let lastError;
  for (;;) {
    try {
      const value = await predicate();
      if (value) return value;
    } catch (e) {
      lastError = e;
    }
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

function killAppTree() {
  if (appProc?.pid) {
    // Only the process tree this script started; never anything else.
    spawnSync("taskkill", ["/PID", String(appProc.pid), "/T", "/F"], { stdio: "ignore" });
    appProc = null;
  }
}

function cleanup() {
  if (!KEEP_OPEN) {
    killAppTree();
    if (tempDir) {
      try {
        rmSync(tempDir, { recursive: true, force: true, maxRetries: 3 });
      } catch {
        // WebView2 may still hold files for a moment; the OS temp cleaner gets them.
      }
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
  // Two ports: ports in use by another instance (1420/9222) are never touched.
  const devPort = await freePort(1421);
  const cdpPort = await freePort(9223);
  tempDir = mkdtempSync(join(tmpdir(), "sct-smoke-"));
  const dataDir = join(tempDir, "data");
  const webviewDir = join(tempDir, "webview2");
  mkdirSync(dataDir);
  mkdirSync(webviewDir);
  const devUrl = `http://localhost:${devPort}`;
  const configPath = join(tempDir, "tauri.smoke.json");
  writeFileSync(
    configPath,
    JSON.stringify({
      build: { devUrl, beforeDevCommand: `npm run dev -- --port ${devPort} --strictPort` },
    }),
  );

  mkdirSync(OUT_DIR, { recursive: true });
  const appLog = join(OUT_DIR, "app.log");
  writeFileSync(appLog, "");
  log(
    `launching app (dev ${devPort}, cdp ${cdpPort}, data ${dataDir}); first build can take minutes`,
  );
  appProc = spawn(`npm run tauri dev -- --config "${configPath}"`, {
    cwd: ROOT,
    shell: true,
    windowsHide: false,
    env: {
      ...process.env,
      SCT_SIM_PEDALS: "1",
      SCT_DATA_DIR: dataDir,
      WEBVIEW2_USER_DATA_FOLDER: webviewDir,
      WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${cdpPort}`,
    },
  });
  let exited = null;
  appProc.on("exit", (code) => (exited = code ?? "signal"));
  const append = (chunk) => writeFileSync(appLog, chunk, { flag: "a" });
  appProc.stdout.on("data", append);
  appProc.stderr.on("data", append);

  const target = await waitFor(
    async () => {
      if (exited !== null) throw new Error(`tauri dev exited early with ${exited}; see ${appLog}`);
      const list = await (await fetch(`http://127.0.0.1:${cdpPort}/json`)).json();
      return list.find((t) => t.type === "page" && t.url.startsWith(devUrl));
    },
    { timeout: BUILD_TIMEOUT_MS, interval: 1000, what: "the app window (CDP page)" },
  ).catch((e) => {
    if (exited !== null) throw new Error(`tauri dev exited early with ${exited}; see ${appLog}`);
    throw e;
  });
  return { target, devUrl };
}

// ---------------------------------------------------------------- CDP

let ws;
let msgId = 0;
const pending = new Map();
let devUrl;

async function connect(target) {
  ws = new WebSocket(target.webSocketDebuggerUrl);
  await new Promise((res, rej) => {
    ws.addEventListener("open", res, { once: true });
    ws.addEventListener("error", () => rej(new Error("CDP websocket failed")), { once: true });
  });
  ws.addEventListener("message", (e) => {
    const m = JSON.parse(e.data);
    if (m.id && pending.has(m.id)) {
      pending.get(m.id)(m);
      pending.delete(m.id);
    }
  });
  await send("Page.enable");
}

function send(method, params = {}) {
  return new Promise((res, rej) => {
    const id = ++msgId;
    pending.set(id, (m) =>
      m.error ? rej(new Error(`${method}: ${m.error.message}`)) : res(m.result),
    );
    ws.send(JSON.stringify({ id, method, params }));
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
  await send("Page.navigate", { url: devUrl + path });
  await waitFor(
    () => ev(`document.readyState === "complete" && document.body.innerText.length > 0`),
    {
      timeout: 20_000,
      what: `page ${path} to load`,
    },
  );
  // The simulated-pedals panel resends its own sliders (all 0) once it sees the device after a
  // load. Wait for it, or that write lands after, and overrides, the pedal values set by a step.
  await waitFor(() => ev(`Boolean(document.querySelector("aside.sim-panel"))`), {
    timeout: 15_000,
    what: "the simulated pedals panel",
  });
  await sleep(400);
}

const hasText = (needle) => ev(`document.body.innerText.includes(${JSON.stringify(needle)})`);

/** Clicks the first element matching `selector` whose text is exactly `text` (or any, if null). */
async function click(selector, text = null, scope = "document") {
  const ok = await ev(`(() => {
    const els = [...${scope}.querySelectorAll(${JSON.stringify(selector)})];
    const el = els.find((e) => ${text === null} || e.textContent.trim() === ${JSON.stringify(text)});
    if (!el || el.disabled) return false;
    el.click();
    return true;
  })()`);
  if (!ok) throw new Error(`no enabled ${selector} with text ${JSON.stringify(text)}`);
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
  // Leave the pedals released for the next step, whatever happened.
  await setPedals(0, 0, 0).catch(() => {});
}

function assert(cond, message) {
  if (!cond) throw new Error(message);
}

// ---- shared state between steps
let simDevice = null;
let profile = null;

/** Raw sim fraction that the profile calibrates to `calibrated` (0..1) for `pedal`. */
function rawFractionFor(pedal, calibrated) {
  const { deadzoneLow: lo, deadzoneHigh: hi } = profile[pedal].calibration;
  return lo + calibrated * (1 - lo - hi);
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
      if (snap.error) throw new Error(`drill page error: ${snap.error}`);
      return snap.finished && snap.pills.length > 0;
    },
    { timeout, interval: 100, what: "the set to finish (FINISHED + Set Summary)" },
  );
  return { snap, grades: [...grades], states: [...states] };
}

const pillTotals = (pills) => pills.map((p) => Number(p.match(/:\s*(\d+)/)?.[1] ?? NaN));

// Grade thresholds (crates/core/src/scoring.rs): S >= 95, A >= 85, B >= 70, C >= 55, else D.
const GOOD_GRADES = ["S", "A"];

async function main() {
  mkdirSync(OUT_DIR, { recursive: true });
  let target;
  if (NO_LAUNCH) {
    const cdp = option("--cdp", "9222");
    const list = await (await fetch(`http://127.0.0.1:${cdp}/json`)).json();
    target = list.find((t) => t.type === "page");
    assert(target, `no page target on CDP port ${cdp}`);
    devUrl = new URL(target.url).origin;
  } else {
    ({ target, devUrl } = await launchApp());
  }
  await connect(target);
  log(`attached to ${target.url}`);
  await waitFor(() => ev("Boolean(window.__TAURI_INTERNALS__)"), {
    timeout: 30_000,
    what: "the page to run inside Tauri",
  });
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
    assert(
      (await loadProfileOf(simDevice.id)) === null,
      "simulated device already has a profile; SCT_DATA_DIR should be fresh",
    );
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
      // Let the detector take its resting baseline, then press fully and release.
      await sleep(600);
      const values = [0, 0, 0];
      values[SIM_AXIS[pedal]] = 1;
      await setPedals(...values);
      await sleep(700);
      await setPedals(0, 0, 0);
      await waitFor(
        () =>
          ev(`(() => {
            const strong = document.querySelector(".wizard .prompt strong");
            return !strong || strong.textContent.trim() !== ${JSON.stringify(pedal)};
          })()`),
        { timeout: 10_000, what: `the wizard to accept the ${pedal}` },
      );
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

    // Re-sweep the brake range from the calibration panel.
    await waitFor(
      () => ev(`Boolean(document.querySelector('[data-testid="calibration-brake"]'))`),
      { what: "the calibration panel" },
    );
    await click(
      "button",
      "Recalibrate range",
      `document.querySelector('[data-testid="calibration-brake"]')`,
    );
    await sleep(300);
    await setPedals(0, 1, 0);
    await sleep(500);
    await setPedals(0, 0, 0);
    await sleep(300);
    await click(
      "button",
      "Done sweeping",
      `document.querySelector('[data-testid="calibration-brake"]')`,
    );
    await waitFor(() => hasText("Recalibrate range"), { what: "the sweep to end" });
    assert(
      !(await ev(`Boolean(document.querySelector(".calibration .error"))`)),
      "calibration panel shows an error",
    );
    profile = await loadProfileOf(simDevice.id);
    const { min, max } = profile.brake.calibration;
    assert(min <= -32000 && max >= 32000, `brake range after re-sweep ${min}..${max}`);
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
    const want = { throttle: raw("throttle", 0.25), brake: raw("brake", 0.6) };
    await setPedals(want.throttle, want.brake, 0);
    await waitFor(
      async () => {
        const [t, b] = [await livePedal("throttle"), await livePedal("brake")];
        return (
          t !== null && b !== null && Math.abs(t * 100 - 25) <= 3 && Math.abs(b * 100 - 60) <= 3
        );
      },
      { what: "throttle ~25% and brake ~60% in the live stream" },
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
    const drill = await findDrill("sample", "brake-hold-70");
    await setPedals(0, raw("brake", drill.target / 100), 0);
    await waitFor(async () => Math.abs((await livePedal("brake")) * 100 - drill.target) <= 1, {
      what: "the brake sitting on the target",
    });
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
      grades.length > 0 && grades.every((g) => GOOD_GRADES.includes(g)),
      `rep grades ${grades.join(",")} are not all S/A`,
    );
    const totals = pillTotals(snap.pills);
    assert(
      totals.every((t) => t >= 85),
      `rep totals ${totals.join(",")} are not all >= 85`,
    );
    const summaryGrade = snap.summary.match(/Average:\s*\d+\s*\((\w)\)/)?.[1];
    assert(GOOD_GRADES.includes(summaryGrade), `set grade ${summaryGrade}`);
  });

  await step("hold-drill-out-of-band", async () => {
    await click("button", "Pick Another Drill");
    await waitFor(() => hasText("Select a Drill"), { what: "the drill picker" });
    const drill = await findDrill("sample", "brake-hold-70");
    await setPedals(0, raw("brake", (drill.target - 30) / 100), 0);
    await waitFor(
      async () => Math.abs((await livePedal("brake")) * 100 - (drill.target - 30)) <= 1,
      { what: "the brake 30 points below target" },
    );
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

  await step("abort-and-play-again", async () => {
    await click("button", "Pick Another Drill");
    await waitFor(() => hasText("Select a Drill"), { what: "the drill picker" });
    await setPedals(0, 0, 0);
    // Abort during the countdown.
    await click("button", "Start Drill");
    await waitFor(async () => (await drillSnap()).state === "COUNTDOWN", { what: "COUNTDOWN" });
    await click("button", "Abort Set");
    await waitFor(async () => (await drillSnap()).finished, {
      timeout: 15_000,
      what: "the aborted set to finish",
    });
    assert(
      !(await ev(
        `[...document.querySelectorAll("button")].some((b) => b.textContent.trim() === "Abort Set")`,
      )),
      "Abort Set is still offered",
    );
    assert((await drillSnap()).state === "FINISHED", "state is not FINISHED after abort");
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
      await waitFor(async () => (await optionTexts(1)).length === drills.length, {
        what: `drill list of ${p.name}`,
      });
      for (const [i, d] of drills.entries()) {
        await selectOption(1, d.name);
        await waitFor(
          () =>
            ev(`(() => {
              const info = document.querySelector(".drill-info")?.innerText ?? "";
              return info.includes("Target Pedal: ${d.pedal}") && info.includes("Target: ${d.target}%") && info.includes("Reps: ${d.reps}");
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
}

// ---------------------------------------------------------------- helpers that need the profile

/** Raw sim fraction for a calibrated value; falls back to the default 2% deadzones. */
function raw(pedal, calibrated) {
  if (!profile) return 0.02 + calibrated * 0.96;
  return rawFractionFor(pedal, calibrated);
}

const loadProfileOf = (deviceId) => invoke("load_profile", { deviceId });

const bundledPresets = () =>
  readdirSync(join(ROOT, "presets"))
    .filter((f) => f.endsWith(".json"))
    .map((f) => JSON.parse(readFileSync(join(ROOT, "presets", f), "utf8")));

/** Hold drills on the brake or throttle: what the drill screen offers. */
const playableOf = (preset) =>
  preset.drills.filter((d) => d.type === "hold" && d.pedal !== "clutch");

async function findDrill(presetId, drillId) {
  const preset = bundledPresets().find((p) => p.id === presetId);
  const drill = preset?.drills.find((d) => d.id === drillId);
  assert(drill, `preset ${presetId} / drill ${drillId} not found in presets/`);
  return drill;
}

// ---------------------------------------------------------------- run

try {
  await main();
} catch (e) {
  results.push({ name: "setup", ok: false, secs: "0", detail: e.message });
  log(`FAIL  setup: ${e.stack ?? e.message}`);
}

log("\nSummary");
for (const [i, r] of results.entries()) {
  log(
    `${r.ok ? "PASS" : "FAIL"}  ${String(i + 1).padStart(2, "0")}  ${r.name.padEnd(36)} ${r.secs.padStart(6)}s  ${r.detail}`,
  );
}
const failed = results.filter((r) => !r.ok).length;
log(`\n${results.length - failed}/${results.length} passed. Screenshots: ${OUT_DIR}`);
try {
  ws?.close();
} catch {
  // already closed
}
process.exit(failed === 0 ? 0 : 1);
