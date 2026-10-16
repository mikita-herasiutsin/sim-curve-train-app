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

export function evaluateVitestResults(results) {
  if (!results || !results.numTotalTests) {
    return "no-results";
  }
  const failed = (results.numFailedTests ?? 0) + (results.numFailedTestSuites ?? 0);
  if (failed > 0) {
    return "killed";
  }
  return "survived";
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
    return "";
  } catch (err) {
    return err?.stderr ? String(err.stderr) : "";
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
  const stderr = executeVitest(args, outputFile, runner);
  const results = readVitestResults(outputFile);
  const outcome = evaluateVitestResults(results);

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
