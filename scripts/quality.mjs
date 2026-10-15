import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import process from "node:process";
import { pathToFileURL } from "node:url";
import { ESLint } from "eslint";

// Runs a git command in cwd and returns its stdout as string.
function git(args, cwd) {
  return execFileSync("git", args, { encoding: "utf8", cwd });
}

// Converts a path to repo-relative with forward slashes.
export function toRepoRelative(filePath, repoRoot) {
  const fileNorm = filePath.replace(/\\/g, "/");
  let rootNorm = repoRoot.replace(/\\/g, "/");
  if (!rootNorm.endsWith("/")) rootNorm += "/";

  if (fileNorm.length >= rootNorm.length) {
    const prefix = fileNorm.slice(0, rootNorm.length);
    if (prefix.toLowerCase() === rootNorm.toLowerCase()) {
      return fileNorm.slice(rootNorm.length);
    }
  }
  return fileNorm.replace(/^\/+/, "");
}

// Parses git diff -U0 output into line ranges per file.
export function parseDiff(text) {
  const result = new Map();
  let currentFile = null;

  for (const line of text.split(/\r?\n/)) {
    if (line.startsWith("+++ ")) {
      if (line === "+++ /dev/null") {
        currentFile = null;
      } else {
        const match = line.match(/^\+\+\+ b\/(.+)$/);
        if (match) {
          currentFile = match[1];
          if (!result.has(currentFile)) {
            result.set(currentFile, []);
          }
        } else {
          currentFile = null;
        }
      }
      continue;
    }

    if (currentFile && line.startsWith("@@ ")) {
      const hunkMatch = line.match(/^@@ -\d+(?:,\d+)? \+(\d+)(?:,(\d+))? @@/);
      if (hunkMatch) {
        const c = parseInt(hunkMatch[1], 10);
        const d = hunkMatch[2] !== undefined ? parseInt(hunkMatch[2], 10) : 1;
        if (d === 0) {
          result.get(currentFile).push([Math.max(c, 1), Math.max(c, 1)]);
        } else {
          result.get(currentFile).push([c, c + d - 1]);
        }
      }
    }
  }

  return result;
}

// Returns true when [start, end] intersects any range in ranges.
export function overlaps(ranges, start, end) {
  if (!ranges || ranges.length === 0) return false;
  for (const [rStart, rEnd] of ranges) {
    if (rStart <= end && rEnd >= start) {
      return true;
    }
  }
  return false;
}

// Computes the CRAP score from cyclomatic complexity and coverage in 0..1.
export function crap(complexity, coverage) {
  const cov = Math.max(0, Math.min(1, coverage));
  return complexity * complexity * Math.pow(1 - cov, 3) + complexity;
}

// Parses LCOV text into a map of line numbers to hit counts per relative file path.
export function parseLcov(text, repoRoot) {
  const result = new Map();
  let lineMap = null;

  for (const rawLine of text.split(/\r?\n/)) {
    const line = rawLine.trim();
    if (line.startsWith("SF:")) {
      const rawPath = line.slice(3).trim();
      const currentFile = toRepoRelative(rawPath, repoRoot);
      if (!result.has(currentFile)) {
        result.set(currentFile, new Map());
      }
      lineMap = result.get(currentFile);
    } else if (line.startsWith("DA:") && lineMap) {
      const parts = line.slice(3).split(",");
      const lineNum = parseInt(parts[0], 10);
      const hits = parseInt(parts[1], 10);
      if (!Number.isNaN(lineNum) && !Number.isNaN(hits)) {
        lineMap.set(lineNum, (lineMap.get(lineNum) ?? 0) + hits);
      }
    } else if (line === "end_of_record") {
      lineMap = null;
    }
  }

  return result;
}

// Line coverage share in [start, end] from LCOV line hits map.
export function lineCoverage(lines, start, end) {
  if (!lines) return 0;
  let total = 0;
  let covered = 0;

  for (const [line, hits] of lines) {
    if (line >= start && line <= end) {
      total++;
      if (hits > 0) {
        covered++;
      }
    }
  }

  if (total === 0) return 1;
  return covered / total;
}

// Statement coverage share in [start, end] from Istanbul report.
export function istanbulCoverage(fileCov, start, end) {
  if (!fileCov || !fileCov.statementMap) return 0;
  const statementMap = fileCov.statementMap;
  const s = fileCov.s || {};
  let total = 0;
  let covered = 0;

  for (const key of Object.keys(statementMap)) {
    const stmt = statementMap[key];
    const line = stmt && stmt.start ? stmt.start.line : undefined;
    if (line >= start && line <= end) {
      total++;
      if (s[key] > 0) {
        covered++;
      }
    }
  }

  if (total === 0) return 1;
  return covered / total;
}

// Finds the innermost span containing (line, column).
export function matchSpan(spans, line, column) {
  const matching = spans.filter((span) => {
    const afterStart =
      line > span.startLine || (line === span.startLine && column >= (span.startColumn ?? 0));
    const beforeEnd =
      line < span.endLine || (line === span.endLine && column <= (span.endColumn ?? Infinity));
    return afterStart && beforeEnd;
  });

  if (matching.length === 0) return undefined;

  matching.sort((a, b) => {
    const linesA = a.endLine - a.startLine;
    const linesB = b.endLine - b.startLine;
    if (linesA !== linesB) {
      return linesA - linesB;
    }
    if (a.startLine !== b.startLine) {
      return b.startLine - a.startLine;
    }
    return (b.startColumn ?? 0) - (a.startColumn ?? 0);
  });

  return matching[0];
}

const SKIPPED_PREFIXES = /^(?:#\[|\/\/\/|\/\/!|\/\*\*|\*|@)/;

// Checks suppression comments above startLine, skipping attributes and doc comments.
export function suppression(sourceLines, startLine) {
  for (let lineNum = startLine - 1; lineNum >= 1; lineNum--) {
    const rawLine = sourceLines[lineNum - 1];
    if (rawLine === undefined) continue;
    const trimmed = rawLine.trim();
    if (trimmed === "" || SKIPPED_PREFIXES.test(trimmed)) {
      continue;
    }

    if (trimmed.startsWith("// quality-ignore")) {
      const rest = trimmed.slice(17);
      if (rest.startsWith(":")) {
        const reason = rest.slice(1).trim();
        return reason ? "ok" : "no-reason";
      }
      return rest.trim() === "" ? "no-reason" : "none";
    }
    return "none";
  }
  return "none";
}

// Mutation score percent, or null when the total is 0.
export function mutationScore({ detected, undetected }) {
  const total = detected + undetected;
  if (total === 0) return null;
  return (detected / total) * 100;
}

const DETECTED_STATUSES = new Set(["Killed", "Timeout"]);
const UNDETECTED_STATUSES = new Set(["Survived", "NoCoverage"]);

// Matches a report file path against the target files set.
function matchFileKey(normKey, fileSet) {
  if (!fileSet) return normKey;
  if (fileSet.has(normKey)) return normKey;
  for (const f of fileSet) {
    if (normKey.endsWith(f)) return f;
  }
  return null;
}

// Counts detected and undetected mutants from Stryker JSON report.
export function strykerCounts(report, files) {
  let detected = 0;
  let undetected = 0;
  const mutants = [];

  const fileSet = files ? new Set(files.map((f) => f.replace(/\\/g, "/"))) : null;
  const reportFiles = report?.files || {};

  for (const [key, fileData] of Object.entries(reportFiles)) {
    const matchedFile = matchFileKey(key.replace(/\\/g, "/"), fileSet);
    if (!matchedFile) continue;

    for (const m of fileData.mutants || []) {
      if (DETECTED_STATUSES.has(m.status)) {
        detected++;
      } else if (UNDETECTED_STATUSES.has(m.status)) {
        undetected++;
        mutants.push({
          file: matchedFile,
          line: m.location?.start?.line,
          text: `${m.mutatorName}: ${m.replacement || ""}`,
        });
      }
    }
  }

  const result = { detected, undetected, mutants };
  Object.defineProperty(result, "undetectedMutants", {
    value: mutants,
    enumerable: false,
  });
  return result;
}

// Counts detected and undetected mutants from cargo-mutants outcomes array.
export function cargoMutantsCounts(outcomes) {
  let detected = 0;
  let undetected = 0;
  const mutants = [];

  for (const item of outcomes ?? []) {
    const summary = item.summary;
    if (summary === "CaughtMutant" || summary === "Timeout") {
      detected++;
    } else if (summary === "MissedMutant") {
      undetected++;
      mutants.push({
        text: item.scenario?.Mutant?.name ?? "",
      });
    }
  }

  const result = { detected, undetected, mutants };
  Object.defineProperty(result, "undetectedMutants", {
    value: mutants,
    enumerable: false,
  });
  return result;
}

const TS_EXCLUSIONS = /\.(test|spec)\.[^/]+$/;

// Maps a repo-relative path to its gate permissions.
export function fileGates(relPath) {
  const norm = relPath.replace(/\\/g, "/");

  if (norm.endsWith(".rs")) {
    const underCrates = norm.startsWith("crates/");
    return {
      type: "rust",
      complexity: true,
      crap: underCrates,
      mutation: underCrates,
    };
  }

  if (norm === "src/vitest-setup.ts" || norm.endsWith(".d.ts") || TS_EXCLUSIONS.test(norm)) {
    return null;
  }

  if (norm.startsWith("src/") && norm.endsWith(".ts")) {
    return { type: "ts", complexity: true, crap: true, mutation: true };
  }
  if (norm.startsWith("scripts/") && norm.endsWith(".mjs")) {
    return { type: "ts", complexity: true, crap: true, mutation: false };
  }
  if (norm.startsWith("src/") && norm.endsWith(".svelte")) {
    return { type: "ts", complexity: true, crap: false, mutation: false };
  }

  return null;
}

// Parses command-line arguments into mode and active gates.
export function parseArgs(argv) {
  let mode = "changed";
  let base = null;
  let metrics = false;
  let mutation = false;

  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    if (arg === "--pilot") {
      mode = "pilot";
    } else if (arg === "--changed") {
      mode = "changed";
      const next = argv[i + 1];
      if (next && !next.startsWith("--")) {
        base = next;
        i++;
      }
    } else if (arg === "--metrics") {
      metrics = true;
    } else if (arg === "--mutation") {
      mutation = true;
    }
  }

  if (mode === "changed" && !base) {
    base = "origin/main";
  }
  if (!metrics && !mutation) {
    metrics = true;
  }

  return { mode, base, metrics, mutation };
}

// Configures ESLint with complexity and span rules.
export function createQualityEslint() {
  return new ESLint({
    overrideConfig: [
      {
        plugins: {
          quality: {
            rules: {
              span: {
                create(context) {
                  function report(node) {
                    const parent = node.parent;
                    const isParent =
                      parent &&
                      (parent.type === "MethodDefinition" ||
                        parent.type === "Property" ||
                        parent.type === "PropertyDefinition");
                    context.report({
                      loc: {
                        start: isParent ? parent.loc.start : node.loc.start,
                        end: node.loc.end,
                      },
                      message: "span",
                    });
                  }
                  return {
                    FunctionDeclaration: report,
                    FunctionExpression: report,
                    ArrowFunctionExpression: report,
                  };
                },
              },
            },
          },
        },
        rules: {
          complexity: ["error", 0],
          "quality/span": "error",
        },
      },
    ],
  });
}

// Collects TS function locations and complexities from ESLint results.
export function collectTsFunctions(lintResults, repoRoot) {
  const functions = [];

  for (const fileRes of lintResults) {
    const relPath = toRepoRelative(fileRes.filePath, repoRoot);
    const spans = [];
    const complexityMsgs = [];

    for (const msg of fileRes.messages) {
      if (msg.ruleId === "quality/span") {
        spans.push({
          startLine: msg.line,
          startColumn: msg.column,
          endLine: msg.endLine,
          endColumn: msg.endColumn,
        });
      } else if (msg.ruleId === "complexity") {
        complexityMsgs.push(msg);
      }
    }

    const assigned = new Set();
    for (const msg of complexityMsgs) {
      const compMatch = msg.message.match(/complexity of (\d+)/);
      if (!compMatch) continue;
      const complexity = parseInt(compMatch[1], 10);
      const nameMatch = msg.message.match(/'([^']+)'/);
      const name = nameMatch ? nameMatch[1] : "<anonymous>";
      const span = matchSpan(spans, msg.line, msg.column);
      if (span) {
        const key = `${relPath}:${span.startLine}:${span.startColumn}:${span.endLine}:${span.endColumn}`;
        if (!assigned.has(key)) {
          assigned.add(key);
          functions.push({
            file: relPath,
            name,
            startLine: span.startLine,
            endLine: span.endLine,
            complexity,
          });
        }
      }
    }
  }

  return functions;
}

// Reads outcomes.json from cargo-mutants output directory.
function readOutcomes(outDir, repoRoot) {
  const p1 = resolve(repoRoot, outDir, "mutants.out/outcomes.json");
  const p2 = resolve(repoRoot, outDir, "outcomes.json");
  const p = existsSync(p1) ? p1 : existsSync(p2) ? p2 : null;
  if (!p) return [];
  return JSON.parse(readFileSync(p, "utf8"));
}

// Gathers Rust functions using the complexity binary.
function collectRustFunctions(rsCandidates, repoRoot) {
  if (rsCandidates.length === 0) return [];
  const output = execFileSync("cargo", ["run", "-q", "-p", "complexity", "--", ...rsCandidates], {
    cwd: repoRoot,
    shell: process.platform === "win32",
    encoding: "utf8",
  });
  const entries = JSON.parse(output);
  return entries.map((e) => ({
    file: toRepoRelative(e.file, repoRoot),
    name: e.name,
    startLine: e.startLine,
    endLine: e.endLine,
    complexity: e.complexity,
  }));
}

// Collects coverage maps for Rust and TS when required.
function collectCoverage(targetFunctions, repoRoot) {
  let rustLcov = null;
  let tsCovMap = null;

  const rustNeedsCrap = targetFunctions.some(
    (fn) => fileGates(fn.file)?.type === "rust" && fileGates(fn.file)?.crap,
  );
  if (rustNeedsCrap) {
    execFileSync(
      "cargo",
      [
        "llvm-cov",
        "-p",
        "sct-core",
        "-p",
        "telemetry-extract",
        "--lcov",
        "--output-path",
        "coverage/rust.lcov",
      ],
      {
        cwd: repoRoot,
        shell: process.platform === "win32",
        stdio: "ignore",
      },
    );
    const lcovText = readFileSync(resolve(repoRoot, "coverage/rust.lcov"), "utf8");
    rustLcov = parseLcov(lcovText, repoRoot);
  }

  const tsCrapFiles = Array.from(
    new Set(
      targetFunctions
        .filter((fn) => {
          const g = fileGates(fn.file);
          return g?.type === "ts" && g?.crap;
        })
        .map((fn) => fn.file),
    ),
  );

  if (tsCrapFiles.length > 0) {
    execFileSync(
      "npx",
      [
        "vitest",
        "related",
        "--run",
        ...tsCrapFiles,
        "--coverage.enabled",
        "--coverage.provider=v8",
        "--coverage.reporter=json",
        "--coverage.reportsDirectory=coverage/ts",
      ],
      {
        cwd: repoRoot,
        shell: process.platform === "win32",
        stdio: "ignore",
      },
    );
    const tsCovJson = JSON.parse(
      readFileSync(resolve(repoRoot, "coverage/ts/coverage-final.json"), "utf8"),
    );
    tsCovMap = new Map();
    for (const [k, v] of Object.entries(tsCovJson)) {
      tsCovMap.set(toRepoRelative(k, repoRoot), v);
    }
  }

  return { rustLcov, tsCovMap };
}

// Prints the formatted table of failing functions.
function printFailingTable(failingFunctions) {
  const rows = failingFunctions.map((f) => ({
    loc: `${f.file}:${f.startLine}`,
    name: f.name,
    complexity: String(f.complexity),
    coverage: f.coverage !== null ? `${Math.round(f.coverage * 100)}%` : "-",
    crap: f.crap !== null ? f.crap.toFixed(1) : "-",
  }));

  const headers = {
    loc: "file:startLine",
    name: "name",
    complexity: "complexity",
    coverage: "coverage%",
    crap: "CRAP",
  };

  const colWidths = {
    loc: Math.max(headers.loc.length, ...rows.map((r) => r.loc.length)),
    name: Math.max(headers.name.length, ...rows.map((r) => r.name.length)),
    complexity: Math.max(headers.complexity.length, ...rows.map((r) => r.complexity.length)),
    coverage: Math.max(headers.coverage.length, ...rows.map((r) => r.coverage.length)),
    crap: Math.max(headers.crap.length, ...rows.map((r) => r.crap.length)),
  };

  function formatRow(r) {
    return `${r.loc.padEnd(colWidths.loc)}  ${r.name.padEnd(colWidths.name)}  ${r.complexity.padEnd(colWidths.complexity)}  ${r.coverage.padEnd(colWidths.coverage)}  ${r.crap.padEnd(colWidths.crap)}`;
  }

  console.log(formatRow(headers));
  for (const r of rows) {
    console.log(formatRow(r));
  }
}

// Runs the metrics gate (complexity and CRAP).
async function runMetricsGate({
  candidateFiles,
  pilotSet,
  diffMap,
  mode,
  repoRoot,
  complexityMax,
  crapMax,
}) {
  const rsCandidates = [...candidateFiles].filter((f) => f.endsWith(".rs"));
  const tsCandidates = [...candidateFiles].filter((f) => !f.endsWith(".rs"));

  const rustFunctions = collectRustFunctions(rsCandidates, repoRoot);

  let tsFunctions = [];
  if (tsCandidates.length > 0) {
    const eslint = createQualityEslint();
    const lintResults = await eslint.lintFiles(tsCandidates.map((f) => resolve(repoRoot, f)));
    tsFunctions = collectTsFunctions(lintResults, repoRoot);
  }

  const allFunctions = [...rustFunctions, ...tsFunctions];
  const targetFunctions = allFunctions.filter((fn) => {
    if (pilotSet.has(fn.file)) return true;
    if (mode === "changed") {
      const ranges = diffMap.get(fn.file);
      return overlaps(ranges, fn.startLine, fn.endLine);
    }
    return false;
  });

  if (targetFunctions.length === 0) {
    return { targetCount: 0, failed: false };
  }

  const { rustLcov, tsCovMap } = collectCoverage(targetFunctions, repoRoot);

  const sourceCache = new Map();
  function getSourceLines(relPath) {
    if (!sourceCache.has(relPath)) {
      const text = readFileSync(resolve(repoRoot, relPath), "utf8");
      sourceCache.set(relPath, text.split(/\r?\n/));
    }
    return sourceCache.get(relPath);
  }

  const failingFunctions = [];
  for (const fn of targetFunctions) {
    const gate = fileGates(fn.file);
    const appliesCrap = gate ? gate.crap : false;
    let cov = null;
    let crapScore = null;

    if (appliesCrap) {
      if (gate.type === "rust") {
        cov = lineCoverage(rustLcov?.get(fn.file), fn.startLine, fn.endLine);
      } else {
        cov = istanbulCoverage(tsCovMap?.get(fn.file), fn.startLine, fn.endLine);
      }
      crapScore = crap(fn.complexity, cov);
    }

    fn.coverage = cov;
    fn.crap = crapScore;

    const lines = getSourceLines(fn.file);
    const supp = suppression(lines, fn.startLine);

    const overComplexity = fn.complexity > complexityMax;
    const overCrap = appliesCrap && crapScore > crapMax;
    const isOverLimits = overComplexity || overCrap;
    const failed = supp === "no-reason" || (supp !== "ok" && isOverLimits);

    if (failed) {
      failingFunctions.push(fn);
    }
  }

  if (failingFunctions.length > 0) {
    printFailingTable(failingFunctions);
  }

  console.log(
    `metrics: ${targetFunctions.length} functions checked, ${failingFunctions.length} over the limits`,
  );
  return { targetCount: targetFunctions.length, failed: failingFunctions.length > 0 };
}

// Runs TypeScript mutation gate using Stryker.
function runTsMutation({ pilotFiles, diffMap, mode, repoRoot, mutationMin }) {
  const tsPilot = pilotFiles.filter((f) => f.startsWith("src/") && f.endsWith(".ts"));
  let tsTargetFiles;
  let tsMutateList;

  if (mode === "pilot") {
    tsTargetFiles = tsPilot;
    tsMutateList = tsPilot;
  } else {
    const tsChanged = [...diffMap.keys()].filter((f) => {
      const g = fileGates(f);
      return g && g.mutation && g.type === "ts" && existsSync(resolve(repoRoot, f));
    });
    tsTargetFiles = Array.from(new Set([...tsPilot, ...tsChanged]));
    const mutateParts = [];
    for (const f of tsChanged) {
      const ranges = diffMap.get(f) ?? [];
      for (const [start, end] of ranges) {
        mutateParts.push(`${f}:${start}-${end}`);
      }
    }
    for (const f of tsPilot) {
      mutateParts.push(f);
    }
    tsMutateList = mutateParts;
  }

  if (tsTargetFiles.length === 0) return false;

  try {
    execFileSync("npx", ["stryker", "run", "--mutate", tsMutateList.join(",")], {
      cwd: repoRoot,
      shell: process.platform === "win32",
      env: {
        ...process.env,
        QUALITY_RELATED: tsTargetFiles.join(" "),
      },
      stdio: "inherit",
    });
  } catch {
    // Stryker exit code ignored
  }

  const reportPath = resolve(repoRoot, "reports/mutation/mutation.json");
  if (!existsSync(reportPath)) {
    throw new Error("missing Stryker report: reports/mutation/mutation.json");
  }
  const report = JSON.parse(readFileSync(reportPath, "utf8"));
  const counts = strykerCounts(report, tsTargetFiles);
  for (const m of counts.mutants) {
    console.log(`${m.file}:${m.line} ${m.text}`);
  }
  const score = mutationScore(counts);
  const scoreStr = score !== null ? `${score.toFixed(1)}%` : "-";
  console.log(
    `mutation ts: ${counts.detected}/${counts.detected + counts.undetected} detected = ${scoreStr} (min ${mutationMin})`,
  );
  return score !== null && score < mutationMin;
}

// Runs Rust mutation gate using cargo-mutants.
function runRustMutation({ pilotFiles, diffMap, mode, repoRoot, mergeBase, mutationMin }) {
  const rustPilot = pilotFiles.filter((f) => f.startsWith("crates/") && f.endsWith(".rs"));
  let rustTargetFiles;

  if (mode === "pilot") {
    rustTargetFiles = rustPilot;
  } else {
    const rustChanged = [...diffMap.keys()].filter((f) => {
      const g = fileGates(f);
      return g && g.mutation && g.type === "rust" && existsSync(resolve(repoRoot, f));
    });
    rustTargetFiles = Array.from(new Set([...rustPilot, ...rustChanged]));
  }

  if (rustTargetFiles.length === 0) return false;

  let totalDetected = 0;
  let totalUndetected = 0;
  const allUndetectedMutants = [];

  function processOutcomes(outDir) {
    const outcomes = readOutcomes(outDir, repoRoot);
    const counts = cargoMutantsCounts(outcomes);
    totalDetected += counts.detected;
    totalUndetected += counts.undetected;
    allUndetectedMutants.push(...counts.mutants);
  }

  if (mode === "pilot") {
    const args = ["mutants", "-o", "mutants.out"];
    for (const f of rustPilot) {
      args.push("-f", f);
    }
    try {
      execFileSync("cargo", args, {
        cwd: repoRoot,
        shell: process.platform === "win32",
        stdio: "inherit",
      });
    } catch {
      // Exit code ignored
    }
    processOutcomes("mutants.out");
  } else {
    mkdirSync(resolve(repoRoot, "mutants.out"), { recursive: true });
    const cratesDiff = git(["diff", "--no-color", mergeBase, "--", "crates"], repoRoot);
    writeFileSync(resolve(repoRoot, "mutants.out/in.diff"), cratesDiff, "utf8");

    try {
      execFileSync("cargo", ["mutants", "-o", "mutants.out", "--in-diff", "mutants.out/in.diff"], {
        cwd: repoRoot,
        shell: process.platform === "win32",
        stdio: "inherit",
      });
    } catch {
      // Exit code ignored
    }
    processOutcomes("mutants.out");

    if (rustPilot.length > 0) {
      const pilotArgs = ["mutants", "-o", "mutants.out/pilot"];
      for (const f of rustPilot) {
        pilotArgs.push("-f", f);
      }
      try {
        execFileSync("cargo", pilotArgs, {
          cwd: repoRoot,
          shell: process.platform === "win32",
          stdio: "inherit",
        });
      } catch {
        // Exit code ignored
      }
      processOutcomes("mutants.out/pilot");
    }
  }

  for (const m of allUndetectedMutants) {
    console.log(m.text);
  }
  const score = mutationScore({
    detected: totalDetected,
    undetected: totalUndetected,
  });
  const scoreStr = score !== null ? `${score.toFixed(1)}%` : "-";
  console.log(
    `mutation rust: ${totalDetected}/${totalDetected + totalUndetected} detected = ${scoreStr} (min ${mutationMin})`,
  );
  return score !== null && score < mutationMin;
}

// Main entry point for quality gate enforcement.
async function main() {
  const repoRoot = resolve(git(["rev-parse", "--show-toplevel"]).trim());
  const configPath = resolve(repoRoot, "quality.config.json");
  const config = JSON.parse(readFileSync(configPath, "utf8"));
  const complexityMax = config.complexityMax ?? 12;
  const crapMax = config.crapMax ?? 12;
  const mutationMin = config.mutationMin ?? 80;
  const pilotFiles = (config.pilot ?? []).map((f) => f.replace(/\\/g, "/"));

  const {
    mode,
    base,
    metrics: runMetrics,
    mutation: runMutation,
  } = parseArgs(process.argv.slice(2));

  let diffMap = new Map();
  let mergeBase = null;

  if (mode === "changed") {
    mergeBase = git(["merge-base", base, "HEAD"], repoRoot).trim();
    const diffText = git(["diff", "-U0", "--no-color", mergeBase], repoRoot);
    diffMap = parseDiff(diffText);
  }

  const pilotSet = new Set(pilotFiles);
  const candidateFiles = new Set(pilotFiles.filter((f) => existsSync(resolve(repoRoot, f))));

  if (mode === "changed") {
    for (const f of diffMap.keys()) {
      if (fileGates(f) !== null && existsSync(resolve(repoRoot, f))) {
        candidateFiles.add(f);
      }
    }
  }

  if (candidateFiles.size === 0) {
    console.log("quality: nothing to check");
    process.exit(0);
  }

  let metricsFailed = false;
  let mutationFailed = false;

  if (runMetrics) {
    const res = await runMetricsGate({
      candidateFiles,
      pilotSet,
      diffMap,
      mode,
      repoRoot,
      complexityMax,
      crapMax,
    });
    if (res.targetCount === 0 && !runMutation) {
      console.log("quality: nothing to check");
      process.exit(0);
    }
    metricsFailed = res.failed;
  }

  if (runMutation) {
    const tsFailed = runTsMutation({
      pilotFiles,
      diffMap,
      mode,
      repoRoot,
      mutationMin,
    });
    const rustFailed = runRustMutation({
      pilotFiles,
      diffMap,
      mode,
      repoRoot,
      mergeBase,
      mutationMin,
    });
    mutationFailed = tsFailed || rustFailed;
  }

  process.exit(metricsFailed || mutationFailed ? 1 : 0);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main();
}
