import { existsSync, readFileSync, rmSync } from "node:fs";
import process from "node:process";
import { pathToFileURL } from "node:url";
import { runCommand } from "./quality.mjs";

export function getOutputFile(worker = process.env.STRYKER_MUTATOR_WORKER || "0") {
  return `.stryker-tmp/vitest-${worker}.json`;
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

export function executeVitest(args, outputFile, runner = runCommand) {
  rmSync(outputFile, { force: true });
  const cleanArgs = args.filter(Boolean);
  try {
    runner(
      "npx",
      ["vitest", "related", "--run", "--reporter=json", `--outputFile=${outputFile}`, ...cleanArgs],
      { stdio: ["ignore", "ignore", "pipe"], encoding: "utf8" },
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
  const worker = options.worker ?? (process.env.STRYKER_MUTATOR_WORKER || "0");
  const outputFile = options.outputFile || getOutputFile(worker);
  const { status, stderr } = executeVitest(args, outputFile, runner);
  const outcome = evaluateVitestResults(readVitestResults(outputFile), status);

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
