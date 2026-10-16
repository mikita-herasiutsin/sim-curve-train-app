import { existsSync, readFileSync, rmSync } from "node:fs";
import process from "node:process";
import { pathToFileURL } from "node:url";
import { runCommand } from "./quality.mjs";

// Stryker runs the command runner in its main process, so STRYKER_MUTATOR_WORKER is unset and
// concurrent runs share no id. The process id keeps their report files and caches apart.
export function getOutputFile(id = process.pid) {
  return `.stryker-tmp/vitest-${id}.json`;
}

export function getCacheDir(id = process.pid) {
  return `.stryker-tmp/vite-${id}`;
}

export function readVitestResults(filePath) {
  if (!existsSync(filePath)) return null;
  try {
    return JSON.parse(readFileSync(filePath, "utf8"));
  } catch {
    return null;
  }
}

// Vitest exits non-zero for failed tests, for suites that fail to load, and for a worker that
// crashes part-way (a mutant that loops until it runs out of memory). The JSON report marks that
// crash as a success, so the exit status decides. A run with no report, or one that collected no
// tests, crashed before the tests ran; that isn't the mutant's doing, so it isn't a kill.
export function evaluateVitestResults(results, status) {
  if (status === 0) {
    return "survived";
  }
  const ran = (results?.numTotalTests ?? 0) + (results?.numFailedTestSuites ?? 0);
  return ran > 0 ? "killed" : "no-results";
}

export function executeVitest(args, outputFile, runner = runCommand, cacheDir = getCacheDir()) {
  rmSync(outputFile, { force: true });
  const cleanArgs = args.filter(Boolean);
  try {
    runner(
      "npx",
      ["vitest", "related", "--run", "--reporter=json", `--outputFile=${outputFile}`, ...cleanArgs],
      {
        stdio: ["ignore", "ignore", "pipe"],
        encoding: "utf8",
        env: { ...process.env, QUALITY_VITE_CACHE_DIR: cacheDir },
      },
    );
    return { status: 0, stderr: "" };
  } catch (err) {
    return { status: err?.status ?? 1, stderr: err?.stderr ? String(err.stderr) : "" };
  }
}

export function handleNoResults(stderr, writer = (msg) => process.stderr.write(msg)) {
  writer("stryker-vitest: vitest produced no test results\n");
  if (stderr) {
    writer(stderr);
  }
}

export function runStrykerVitest(args, options = {}) {
  const runner = options.runner || runCommand;
  const outputFile = options.outputFile || getOutputFile();
  const cacheDir = options.cacheDir || getCacheDir();
  const { status, stderr } = executeVitest(args, outputFile, runner, cacheDir);
  const outcome = evaluateVitestResults(readVitestResults(outputFile), status);
  rmSync(outputFile, { force: true });
  rmSync(cacheDir, { recursive: true, force: true });

  if (outcome === "killed") {
    return 1;
  }
  if (outcome === "no-results") {
    handleNoResults(stderr, options.writer);
  }
  return 0;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const exitCode = runStrykerVitest(process.argv.slice(2));
  process.exit(exitCode);
}
