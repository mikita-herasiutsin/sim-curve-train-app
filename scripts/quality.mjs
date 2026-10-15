import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import process from "node:process";
import { pathToFileURL } from "node:url";
import { ESLint } from "eslint";

// Runs a git command in cwd and returns its stdout as string.
function git(args, cwd) {
  return execFileSync("git", args, { encoding: "utf8", cwd });
}

// Quotes a command-line argument if it contains spaces or special characters.
export function quoteArg(arg) {
  const str = String(arg);
  if (str === "" || /[\s"&|<>^%()]/.test(str)) {
    return `"${str.replace(/"/g, '\\"')}"`;
  }
  return str;
}

// Builds a command line string from an executable and its argument list.
export function buildCommandLine(file, args = []) {
  return [file, ...args].map(quoteArg).join(" ");
}

// Executes a command, using a quoted string on Windows to avoid DEP0190.
export function runCommand(file, args, options) {
  if (process.platform === "win32") {
    const cmd = buildCommandLine(file, args);
    return execFileSync(cmd, { ...options, shell: true });
  }
  return execFileSync(file, args, { ...options, shell: false });
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

// Parses newline-separated output of untracked files from git ls-files.
export function parseUntrackedFiles(output) {
  const files = [];
  for (const rawLine of output.split(/\r?\n/)) {
    const line = rawLine.trim();
    if (line) {
      files.push(line.replace(/\\/g, "/"));
    }
  }
  return files;
}

// Appends untracked files to diff map with full file range.
export function addUntrackedToDiff(diffMap, untrackedFiles) {
  for (const file of untrackedFiles) {
    if (!diffMap.has(file)) {
      diffMap.set(file, [[1, Infinity]]);
    }
  }
  return diffMap;
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

// Converts a list of file paths to a set with normalised slashes.
function toFileSet(files) {
  if (!files) return null;
  return new Set(files.map((f) => f.replace(/\\/g, "/")));
}

// Formats the mutator name and replacement string.
function formatMutantText(m) {
  return `${m.mutatorName}: ${m.replacement || ""}`;
}

// Tallies a single mutant into detected or undetected counts for a file.
function tallyStrykerMutant(m, file, counts, mutants) {
  if (DETECTED_STATUSES.has(m.status)) {
    counts.detected++;
  } else if (UNDETECTED_STATUSES.has(m.status)) {
    counts.undetected++;
    mutants.push({
      file,
      line: m.location ? m.location.start?.line : undefined,
      text: formatMutantText(m),
    });
  }
}

// Tallies all mutants for a single file entry in the Stryker report.
function tallyStrykerFile(fileData, file, byFile, mutants) {
  let counts = byFile.get(file);
  if (!counts) {
    counts = { detected: 0, undetected: 0 };
    byFile.set(file, counts);
  }
  for (const m of fileData.mutants || []) {
    tallyStrykerMutant(m, file, counts, mutants);
  }
}

// Counts detected and undetected mutants per file from Stryker JSON report.
export function strykerCounts(report, files) {
  const byFile = new Map();
  const mutants = [];
  const fileSet = toFileSet(files);
  const reportFiles = report?.files || {};

  for (const [key, data] of Object.entries(reportFiles)) {
    const matched = matchFileKey(key.replace(/\\/g, "/"), fileSet);
    if (matched) {
      tallyStrykerFile(data, matched, byFile, mutants);
    }
  }

  const result = { byFile, mutants };
  Object.defineProperty(result, "undetectedMutants", {
    value: mutants,
    enumerable: false,
  });
  return result;
}

const CARGO_DETECTED_STATUSES = new Set(["CaughtMutant", "Timeout"]);

// Tallies a single cargo-mutants outcome into per-file detected or undetected counts.
function tallyCargoOutcome(item, byFile, mutants) {
  const mutant = item.scenario?.Mutant;
  const fileRaw = mutant?.file;
  if (!fileRaw) return;
  const file = fileRaw.replace(/\\/g, "/");

  let counts = byFile.get(file);
  if (!counts) {
    counts = { detected: 0, undetected: 0 };
    byFile.set(file, counts);
  }

  if (CARGO_DETECTED_STATUSES.has(item.summary)) {
    counts.detected++;
  } else if (item.summary === "MissedMutant") {
    counts.undetected++;
    mutants.push({
      file,
      text: mutant?.name ?? "",
    });
  }
}

// Counts detected and undetected mutants per file from cargo-mutants outcomes array.
export function cargoMutantsCounts(outcomes, ...more) {
  const byFile = new Map();
  const mutants = [];
  const seen = new Set();
  const list = [outcomes, ...more].flat(2);

  for (const item of list) {
    if (!item) continue;
    const name = item.scenario?.Mutant?.name;
    if (name) {
      if (seen.has(name)) continue;
      seen.add(name);
    }
    tallyCargoOutcome(item, byFile, mutants);
  }

  const result = { byFile, mutants };
  Object.defineProperty(result, "undetectedMutants", {
    value: mutants,
    enumerable: false,
  });
  return result;
}

const TS_EXCLUSIONS = /\.(test|spec)\.[^/]+$/;

// Returns gate permissions for Rust files.
function rustFileGates(norm) {
  const underCrates = norm.startsWith("crates/");
  return {
    type: "rust",
    complexity: true,
    crap: underCrates,
    mutation: underCrates,
  };
}

// Returns true if a TypeScript path is ignored by quality gates.
function isExcludedTs(norm) {
  if (norm === "src/vitest-setup.ts") return true;
  if (norm.endsWith(".d.ts")) return true;
  return TS_EXCLUSIONS.test(norm);
}

// Checks if path belongs to TypeScript application source.
function isTsSource(norm) {
  return norm.startsWith("src/") && norm.endsWith(".ts");
}

// Checks if path belongs to a project script.
function isScriptSource(norm) {
  return norm.startsWith("scripts/") && norm.endsWith(".mjs");
}

// Checks if path belongs to a Svelte UI component.
function isSvelteSource(norm) {
  return norm.startsWith("src/") && norm.endsWith(".svelte");
}

// Returns gate permissions for TypeScript and related web files.
function tsFileGates(norm) {
  if (isExcludedTs(norm)) return null;
  if (isTsSource(norm)) return { type: "ts", complexity: true, crap: true, mutation: true };
  if (isScriptSource(norm)) return { type: "ts", complexity: true, crap: true, mutation: false };
  if (isSvelteSource(norm)) return { type: "ts", complexity: true, crap: false, mutation: false };
  return null;
}

// Maps a repo-relative path to its gate permissions.
export function fileGates(relPath) {
  const norm = relPath.replace(/\\/g, "/");
  if (norm.endsWith(".rs")) {
    return rustFileGates(norm);
  }
  return tsFileGates(norm);
}

// Reads optional custom git base argument following --changed.
function readBaseArg(argv, i) {
  const next = argv[i + 1];
  if (next && !next.startsWith("--")) {
    return next;
  }
  return null;
}

// Resolves default base branch when not specified in changed mode.
function resolveBase(mode, base) {
  if (mode === "changed" && !base) {
    return "origin/main";
  }
  return base;
}

// Resolves metrics gate enablement when no explicit gate flags are given.
function resolveMetrics(metrics, mutation) {
  if (!metrics && !mutation) {
    return true;
  }
  return metrics;
}

// Applies default values to parsed arguments.
function applyArgDefaults({ mode, base, metrics, mutation }) {
  return {
    mode,
    base: resolveBase(mode, base),
    metrics: resolveMetrics(metrics, mutation),
    mutation,
  };
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
      const customBase = readBaseArg(argv, i);
      if (customBase) {
        base = customBase;
        i++;
      }
    } else if (arg === "--metrics") {
      metrics = true;
    } else if (arg === "--mutation") {
      mutation = true;
    }
  }

  return applyArgDefaults({ mode, base, metrics, mutation });
}

const SPAN_PARENT_TYPES = new Set(["MethodDefinition", "Property", "PropertyDefinition"]);

// Resolves start location of a function AST node or its enclosing definition.
function getSpanStart(node) {
  const parent = node.parent;
  if (parent && SPAN_PARENT_TYPES.has(parent.type)) {
    return parent.loc.start;
  }
  return node.loc.start;
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
                  function reportSpan(node) {
                    context.report({
                      loc: {
                        start: getSpanStart(node),
                        end: node.loc.end,
                      },
                      message: "span",
                    });
                  }
                  return {
                    FunctionDeclaration: reportSpan,
                    FunctionExpression: reportSpan,
                    ArrowFunctionExpression: reportSpan,
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

// Groups lint messages into spans and complexity warnings.
function groupLintMessages(messages) {
  const spans = [];
  const complexityMsgs = [];
  for (const msg of messages) {
    if (msg.ruleId === "quality/span") {
      spans.push({
        startLine: msg.line,
        startColumn: msg.column,
        endLine: msg.endLine,
        endColumn: msg.endColumn,
      });
    }
    if (msg.ruleId === "complexity") {
      complexityMsgs.push(msg);
    }
  }
  return { spans, complexityMsgs };
}

// Extracts function name and complexity score from message text.
function parseComplexityMessage(message) {
  const compMatch = message.match(/complexity of (\d+)/);
  if (!compMatch) return null;
  const nameMatch = message.match(/'([^']+)'/);
  return {
    complexity: parseInt(compMatch[1], 10),
    name: nameMatch ? nameMatch[1] : "<anonymous>",
  };
}

// Extracts functions and complexity ratings from a single lint result.
function extractFileFunctions(fileRes, repoRoot) {
  const relPath = toRepoRelative(fileRes.filePath, repoRoot);
  const { spans, complexityMsgs } = groupLintMessages(fileRes.messages);
  const functions = [];
  const assigned = new Set();

  for (const msg of complexityMsgs) {
    const parsed = parseComplexityMessage(msg.message);
    if (!parsed) continue;
    const span = matchSpan(spans, msg.line, msg.column);
    if (!span) continue;

    const key = `${relPath}:${span.startLine}:${span.startColumn}:${span.endLine}:${span.endColumn}`;
    if (!assigned.has(key)) {
      assigned.add(key);
      functions.push({
        file: relPath,
        name: parsed.name,
        startLine: span.startLine,
        endLine: span.endLine,
        complexity: parsed.complexity,
      });
    }
  }
  return functions;
}

// Collects TS function locations and complexities from ESLint results.
export function collectTsFunctions(lintResults, repoRoot) {
  const functions = [];
  for (const fileRes of lintResults) {
    functions.push(...extractFileFunctions(fileRes, repoRoot));
  }
  return functions;
}

// Locates outcomes.json inside mutants output directory.
export function findOutcomesPath(outDir, repoRoot, fileExists) {
  const p1 = resolve(repoRoot, outDir, "mutants.out/outcomes.json");
  if (fileExists(p1)) return p1;
  const p2 = resolve(repoRoot, outDir, "outcomes.json");
  if (fileExists(p2)) return p2;
  return null;
}

// Reads the `outcomes` array of cargo-mutants' outcomes.json from its output directory.
export function readOutcomes(outDir, repoRoot, fileReader, fileExists) {
  const exists = fileExists || existsSync;
  const read = fileReader || readFileSync;
  const p = findOutcomesPath(outDir, repoRoot, exists);
  // No file means cargo-mutants failed to run; scoring that as "no mutants" would pass the gate.
  if (!p) throw new Error(`cargo-mutants wrote no outcomes.json under ${outDir}`);
  return JSON.parse(read(p, "utf8")).outcomes ?? [];
}

// Parses Rust complexity entries into function descriptors.
export function parseRustFunctionsJson(output, repoRoot) {
  const entries = JSON.parse(output);
  return entries.map((e) => ({
    file: toRepoRelative(e.file, repoRoot),
    name: e.name,
    startLine: e.startLine,
    endLine: e.endLine,
    complexity: e.complexity,
  }));
}

// Gathers Rust functions using the complexity binary.
function collectRustFunctions(rsCandidates, repoRoot, runner) {
  if (rsCandidates.length === 0) return [];
  const run = runner || runCommand;
  const output = run("cargo", ["run", "-q", "-p", "complexity", "--", ...rsCandidates], {
    cwd: repoRoot,
    encoding: "utf8",
  });
  return parseRustFunctionsJson(output, repoRoot);
}

// Checks if a gate applies to Rust crate code requiring CRAP gate.
function isRustCrapGate(gate) {
  return Boolean(gate?.crap && gate.type === "rust");
}

// Checks whether any target function is in a Rust crate requiring CRAP gate.
export function needsRustCoverage(functions) {
  for (const fn of functions) {
    if (isRustCrapGate(fileGates(fn.file))) {
      return true;
    }
  }
  return false;
}

// Checks if a gate applies to TypeScript code requiring CRAP gate.
function isTsCrapGate(gate) {
  return Boolean(gate?.crap && gate.type === "ts");
}

// Returns unique TypeScript files requiring statement coverage for CRAP.
export function findTsCrapFiles(functions) {
  const files = new Set();
  for (const fn of functions) {
    if (isTsCrapGate(fileGates(fn.file))) {
      files.add(fn.file);
    }
  }
  return Array.from(files);
}

// Executes cargo llvm-cov and returns parsed LCOV results.
function runRustCoverage(repoRoot, runner, fileReader) {
  const run = runner || runCommand;
  const read = fileReader || readFileSync;
  run(
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
    { cwd: repoRoot, stdio: "ignore" },
  );
  const lcovText = read(resolve(repoRoot, "coverage/rust.lcov"), "utf8");
  return parseLcov(lcovText, repoRoot);
}

// Parses vitest coverage-final.json into a map of repo-relative paths.
export function parseTsCoverageJson(json, repoRoot) {
  const map = new Map();
  for (const [k, v] of Object.entries(json)) {
    map.set(toRepoRelative(k, repoRoot), v);
  }
  return map;
}

// Executes vitest related coverage and returns Istanbul coverage map.
function runTsCoverage(tsCrapFiles, repoRoot, runner, fileReader) {
  const run = runner || runCommand;
  const read = fileReader || readFileSync;
  run(
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
    { cwd: repoRoot, stdio: "ignore" },
  );
  const json = JSON.parse(read(resolve(repoRoot, "coverage/ts/coverage-final.json"), "utf8"));
  return parseTsCoverageJson(json, repoRoot);
}

// Collects coverage maps for Rust and TS when required.
function collectCoverage(targetFunctions, repoRoot) {
  const rustLcov = needsRustCoverage(targetFunctions) ? runRustCoverage(repoRoot) : null;
  const tsCrapFiles = findTsCrapFiles(targetFunctions);
  const tsCovMap = tsCrapFiles.length > 0 ? runTsCoverage(tsCrapFiles, repoRoot) : null;
  return { rustLcov, tsCovMap };
}

// Calculates maximum column widths for failing functions table.
function calculateColWidths(headers, rows) {
  return {
    loc: Math.max(headers.loc.length, ...rows.map((r) => r.loc.length)),
    name: Math.max(headers.name.length, ...rows.map((r) => r.name.length)),
    complexity: Math.max(headers.complexity.length, ...rows.map((r) => r.complexity.length)),
    coverage: Math.max(headers.coverage.length, ...rows.map((r) => r.coverage.length)),
    crap: Math.max(headers.crap.length, ...rows.map((r) => r.crap.length)),
  };
}

// Formats a single row for the failing functions report table.
function formatReportRow(r, colWidths) {
  return `${r.loc.padEnd(colWidths.loc)}  ${r.name.padEnd(colWidths.name)}  ${r.complexity.padEnd(colWidths.complexity)}  ${r.coverage.padEnd(colWidths.coverage)}  ${r.crap.padEnd(colWidths.crap)}`;
}

// Formats the table lines for functions exceeding complexity or CRAP limits.
export function formatReport(failingFunctions) {
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

  const colWidths = calculateColWidths(headers, rows);
  const lines = [formatReportRow(headers, colWidths)];
  for (const r of rows) {
    lines.push(formatReportRow(r, colWidths));
  }
  return lines;
}

// Prints the formatted table of failing functions.
function printFailingTable(failingFunctions) {
  for (const line of formatReport(failingFunctions)) {
    console.log(line);
  }
}

// Determines if a function falls within pilot files or changed ranges.
export function isTargetFunction(fn, pilotSet, diffMap, mode) {
  if (pilotSet.has(fn.file)) return true;
  if (mode === "changed") {
    const ranges = diffMap.get(fn.file);
    return overlaps(ranges, fn.startLine, fn.endLine);
  }
  return false;
}

// Resolves code coverage share for a function based on language type.
function computeFunctionCoverage(fn, gate, { rustLcov, tsCovMap }) {
  if (gate.type === "rust") {
    const lcov = rustLcov ? rustLcov.get(fn.file) : null;
    return lineCoverage(lcov, fn.startLine, fn.endLine);
  }
  const cov = tsCovMap ? tsCovMap.get(fn.file) : null;
  return istanbulCoverage(cov, fn.startLine, fn.endLine);
}

// Checks whether a function violates complexity or CRAP limits.
export function isFunctionFailing(fn, { complexityMax, crapMax, supp }) {
  if (supp === "no-reason") return true;
  if (supp === "ok") return false;
  if (fn.complexity > complexityMax) return true;
  if (fn.crap !== null && fn.crap > crapMax) return true;
  return false;
}

// Evaluates metrics and suppression status for a single function.
export function evaluateFunctionMetrics(fn, { rustLcov, tsCovMap, complexityMax, crapMax, lines }) {
  const gate = fileGates(fn.file);
  const appliesCrap = Boolean(gate && gate.crap);
  const cov = appliesCrap ? computeFunctionCoverage(fn, gate, { rustLcov, tsCovMap }) : null;
  const crapScore = appliesCrap ? crap(fn.complexity, cov) : null;

  fn.coverage = cov;
  fn.crap = crapScore;

  const supp = suppression(lines, fn.startLine);
  return isFunctionFailing(fn, { complexityMax, crapMax, supp });
}

// Creates a caching source file reader.
function createSourceCache(repoRoot, fileReader) {
  const read = fileReader || readFileSync;
  const cache = new Map();
  return function getSourceLines(relPath) {
    if (!cache.has(relPath)) {
      const text = read(resolve(repoRoot, relPath), "utf8");
      cache.set(relPath, text.split(/\r?\n/));
    }
    return cache.get(relPath);
  };
}

// Lints TypeScript candidate files and extracts function definitions.
async function lintTsCandidates(tsCandidates, repoRoot) {
  if (tsCandidates.length === 0) return [];
  const eslint = createQualityEslint();
  const lintResults = await eslint.lintFiles(tsCandidates.map((f) => resolve(repoRoot, f)));
  return collectTsFunctions(lintResults, repoRoot);
}

// Collects and filters target functions matching run mode.
async function collectTargetFunctions({ candidateFiles, pilotSet, diffMap, mode, repoRoot }) {
  const rsCandidates = [...candidateFiles].filter((f) => f.endsWith(".rs"));
  const tsCandidates = [...candidateFiles].filter((f) => !f.endsWith(".rs"));

  const rustFunctions = collectRustFunctions(rsCandidates, repoRoot);
  const tsFunctions = await lintTsCandidates(tsCandidates, repoRoot);
  const allFunctions = [...rustFunctions, ...tsFunctions];

  return allFunctions.filter((fn) => isTargetFunction(fn, pilotSet, diffMap, mode));
}

// Finds failing functions given target functions and evaluation context.
function findFailingFunctions(targetFunctions, context) {
  const failing = [];
  for (const fn of targetFunctions) {
    const lines = context.getSourceLines(fn.file);
    if (evaluateFunctionMetrics(fn, { ...context, lines })) {
      failing.push(fn);
    }
  }
  return failing;
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
  const targetFunctions = await collectTargetFunctions({
    candidateFiles,
    pilotSet,
    diffMap,
    mode,
    repoRoot,
  });

  if (targetFunctions.length === 0) {
    return { targetCount: 0, failed: false };
  }

  const coverageMaps = collectCoverage(targetFunctions, repoRoot);
  const getSourceLines = createSourceCache(repoRoot);
  const failing = findFailingFunctions(targetFunctions, {
    ...coverageMaps,
    getSourceLines,
    complexityMax,
    crapMax,
  });

  if (failing.length > 0) {
    printFailingTable(failing);
  }

  console.log(
    `metrics: ${targetFunctions.length} functions checked, ${failing.length} over the limits`,
  );
  return { targetCount: targetFunctions.length, failed: failing.length > 0 };
}

// Checks if a gate applies mutation testing to TypeScript code.
function isTsMutationGate(gate) {
  return Boolean(gate?.mutation && gate.type === "ts");
}

// Checks if a file path is a TypeScript file subject to mutation testing.
function isTsMutationCandidate(file, fileExists) {
  if (!isTsMutationGate(fileGates(file))) return false;
  return fileExists(file);
}

// Builds Stryker range strings from changed diff ranges.
export function buildStrykerMutateRanges(files, diffMap) {
  const parts = [];
  for (const f of files) {
    const ranges = diffMap.get(f) || [];
    for (const [start, end] of ranges) {
      if (end === Infinity) {
        parts.push(f);
      } else {
        parts.push(`${f}:${start}-${end}`);
      }
    }
  }
  return parts;
}

// Selects target files and mutation patterns for TypeScript Stryker run.
export function selectTsMutationTargets({ pilotFiles, diffMap, mode, fileExists }) {
  const tsPilot = pilotFiles.filter((f) => f.startsWith("src/") && f.endsWith(".ts"));
  if (mode === "pilot") {
    return { targetFiles: tsPilot, mutateList: tsPilot };
  }

  const tsChanged = [...diffMap.keys()].filter((f) => isTsMutationCandidate(f, fileExists));
  const targetFiles = Array.from(new Set([...tsPilot, ...tsChanged]));
  const mutateList = [...buildStrykerMutateRanges(tsChanged, diffMap), ...tsPilot];
  return { targetFiles, mutateList };
}

// Runs Stryker tool via child process.
function execStryker(mutateList, targetFiles, repoRoot, runner) {
  const run = runner || runCommand;
  try {
    run("npx", ["stryker", "run", "--mutate", mutateList.join(",")], {
      cwd: repoRoot,
      env: {
        ...process.env,
        QUALITY_RELATED: targetFiles.join(" "),
      },
      stdio: "inherit",
    });
  } catch {
    // Stryker exit code ignored
  }
}

// Formats one score line for a file in a mutation run.
export function formatFileScoreLine(lang, file, counts, mutationMin) {
  const total = counts.detected + counts.undetected;
  if (total === 0) return null;
  const score = mutationScore(counts);
  const scoreStr = `${score.toFixed(1)}%`;
  const failed = score < mutationMin;
  const suffix = failed ? " FAIL" : "";
  return {
    line: `mutation ${lang} ${file}: ${counts.detected}/${total} detected = ${scoreStr} (min ${mutationMin})${suffix}`,
    failed,
  };
}

// Logs undetected mutants for a language.
export function logUndetectedMutants(lang, mutants, logger = console.log) {
  if (lang === "rust") {
    for (const m of mutants) {
      logger(m.text);
    }
  } else {
    for (const m of mutants) {
      logger(`${m.file}:${m.line} ${m.text}`);
    }
  }
}

// Formats and logs per-file mutation scores and undetected mutants for a language.
export function evaluateMutationCounts(lang, counts, mutationMin, logger = console.log) {
  let hasFailed = false;
  for (const [file, fileCounts] of counts.byFile || []) {
    const formatted = formatFileScoreLine(lang, file, fileCounts, mutationMin);
    if (!formatted) continue;
    logger(formatted.line);
    if (formatted.failed) {
      hasFailed = true;
    }
  }
  logUndetectedMutants(lang, counts.mutants || [], logger);
  return hasFailed;
}

// Reads and scores Stryker results against minimum threshold.
export function evaluateStrykerScore(
  targetFiles,
  repoRoot,
  mutationMin,
  fileReader,
  fileExists,
  logger = console.log,
) {
  const exists = fileExists || existsSync;
  const read = fileReader || readFileSync;
  const reportPath = resolve(repoRoot, "reports/mutation/mutation.json");
  if (!exists(reportPath)) {
    throw new Error("missing Stryker report: reports/mutation/mutation.json");
  }
  const report = JSON.parse(read(reportPath, "utf8"));
  const counts = strykerCounts(report, targetFiles);
  return evaluateMutationCounts("ts", counts, mutationMin, logger);
}

// Runs TypeScript mutation gate using Stryker.
function runTsMutation({ pilotFiles, diffMap, mode, repoRoot, mutationMin }) {
  const fileExists = (f) => existsSync(resolve(repoRoot, f));
  const { targetFiles, mutateList } = selectTsMutationTargets({
    pilotFiles,
    diffMap,
    mode,
    fileExists,
  });

  if (targetFiles.length === 0) return false;
  // A report left by an earlier run would be scored if Stryker fails to start.
  rmSync(resolve(repoRoot, "reports/mutation/mutation.json"), { force: true });
  execStryker(mutateList, targetFiles, repoRoot);
  return evaluateStrykerScore(targetFiles, repoRoot, mutationMin);
}

// Checks if a gate applies mutation testing to Rust code.
function isRustMutationGate(gate) {
  return Boolean(gate?.mutation && gate.type === "rust");
}

// Checks if a file path is a Rust file subject to mutation testing.
function isRustMutationCandidate(file, fileExists) {
  if (!isRustMutationGate(fileGates(file))) return false;
  return fileExists(file);
}

// Selects Rust target files for pilot or changed modes.
export function selectRustMutationTargets({ pilotFiles, diffMap, mode, fileExists }) {
  const rustPilot = pilotFiles.filter((f) => f.startsWith("crates/") && f.endsWith(".rs"));
  if (mode === "pilot") {
    return { targetFiles: rustPilot, rustPilot };
  }
  const rustChanged = [...diffMap.keys()].filter((f) => isRustMutationCandidate(f, fileExists));
  const targetFiles = Array.from(new Set([...rustPilot, ...rustChanged]));
  return { targetFiles, rustPilot, rustChanged };
}

// Builds the cargo mutants command call descriptors for given mode and pilot files.
export function buildCargoMutantsCalls({ mode, rustPilot, rustChanged = [] }) {
  if (mode === "pilot") {
    const args = ["mutants", "-o", "mutants.out"];
    for (const f of rustPilot) {
      args.push("-f", f);
    }
    return [{ outDir: "mutants.out", args, needsDiff: false }];
  }

  // The pilot run tests changed pilot files in full, so the diff run is only for other files.
  const calls = rustChanged.some((f) => !rustPilot.includes(f))
    ? [
        {
          outDir: "mutants.out",
          args: ["mutants", "-o", "mutants.out", "--in-diff", "mutants.out/in.diff"],
          needsDiff: true,
        },
      ]
    : [];

  if (rustPilot.length > 0) {
    const pilotArgs = ["mutants", "-o", "mutants.out/pilot"];
    for (const f of rustPilot) {
      pilotArgs.push("-f", f);
    }
    calls.push({ outDir: "mutants.out/pilot", args: pilotArgs, needsDiff: false });
  }

  return calls;
}

// Generates in.diff inside mutants.out directory from git diff.
function prepareMutantsDiff(repoRoot, mergeBase, gitRunner) {
  const runner = gitRunner || git;
  mkdirSync(resolve(repoRoot, "mutants.out"), { recursive: true });
  const cratesDiff = runner(["diff", "--no-color", mergeBase, "--", "crates"], repoRoot);
  writeFileSync(resolve(repoRoot, "mutants.out/in.diff"), cratesDiff, "utf8");
}

// Executes cargo mutants command invocation.
function execCargoMutants(args, repoRoot, runner) {
  const run = runner || runCommand;
  try {
    run("cargo", args, { cwd: repoRoot, stdio: "inherit" });
  } catch {
    // Exit code ignored
  }
}

// Aggregates mutant results across multiple outcome directories.
export function aggregateMutantOutcomes(calls, repoRoot, fileReader, fileExists) {
  const allOutcomes = [];
  for (const { outDir } of calls) {
    allOutcomes.push(...readOutcomes(outDir, repoRoot, fileReader, fileExists));
  }
  return cargoMutantsCounts(allOutcomes);
}

// Evaluates aggregate Rust mutation score against minimum required threshold.
export function evaluateRustMutationScore(totals, mutationMin, logger = console.log) {
  return evaluateMutationCounts("rust", totals, mutationMin, logger);
}

// Executes each planned mutants command invocation.
function executeMutantCalls(calls, repoRoot, mergeBase) {
  for (const call of calls) {
    if (call.needsDiff) {
      prepareMutantsDiff(repoRoot, mergeBase);
    }
    execCargoMutants(call.args, repoRoot);
  }
}

// Runs Rust mutation gate using cargo-mutants.
function runRustMutation({ pilotFiles, diffMap, mode, repoRoot, mergeBase, mutationMin }) {
  const fileExists = (f) => existsSync(resolve(repoRoot, f));
  const { targetFiles, rustPilot, rustChanged } = selectRustMutationTargets({
    pilotFiles,
    diffMap,
    mode,
    fileExists,
  });

  if (targetFiles.length === 0) return false;

  const calls = buildCargoMutantsCalls({ mode, rustPilot, rustChanged });
  // Outcomes left by an earlier run would be scored for files this run doesn't test.
  rmSync(resolve(repoRoot, "mutants.out"), { recursive: true, force: true });
  mkdirSync(resolve(repoRoot, "mutants.out"), { recursive: true });
  executeMutantCalls(calls, repoRoot, mergeBase);

  const totals = aggregateMutantOutcomes(calls, repoRoot);
  return evaluateRustMutationScore(totals, mutationMin);
}

// Reads quality gate configuration from JSON file.
export function readQualityConfig(repoRoot, fileReader) {
  const read = fileReader || readFileSync;
  const configPath = resolve(repoRoot, "quality.config.json");
  const config = JSON.parse(read(configPath, "utf8"));
  const pilot = config.pilot || [];
  return {
    complexityMax: config.complexityMax || 12,
    crapMax: config.crapMax || 12,
    mutationMin: config.mutationMin || 80,
    pilotFiles: pilot.map((f) => f.replace(/\\/g, "/")),
  };
}

// Checks if a changed file is tracked by quality gates and exists.
function isCandidateFile(f, fileExists) {
  if (fileGates(f) === null) return false;
  return fileExists(f);
}

// Filters candidate files by fileGates and filesystem existence.
function addGatedChangedFiles(candidateSet, diffMap, fileExists) {
  for (const f of diffMap.keys()) {
    if (isCandidateFile(f, fileExists)) {
      candidateSet.add(f);
    }
  }
}

// Collects candidate files to inspect for active run mode.
export function collectCandidateFiles({ pilotFiles, diffMap, mode, fileExists }) {
  const candidateSet = new Set(pilotFiles.filter(fileExists));
  if (mode === "changed") {
    addGatedChangedFiles(candidateSet, diffMap, fileExists);
  }
  return candidateSet;
}

// Loads merge-base and diff map for changed files including untracked additions.
function loadChangedDiff(base, repoRoot, gitRunner) {
  const runner = gitRunner || git;
  const mergeBase = runner(["merge-base", base, "HEAD"], repoRoot).trim();
  const diffText = runner(["diff", "-U0", "--no-color", mergeBase], repoRoot);
  const diffMap = parseDiff(diffText);
  const untrackedText = runner(["ls-files", "--others", "--exclude-standard"], repoRoot);
  const untracked = parseUntrackedFiles(untrackedText);
  addUntrackedToDiff(diffMap, untracked);
  return { mergeBase, diffMap };
}

// Executes mutation suites for TS and Rust and checks for failures.
function runMutationSuites(params) {
  const tsFailed = runTsMutation(params);
  const rustFailed = runRustMutation(params);
  return tsFailed || rustFailed;
}

// Executes metrics gate workflow.
async function executeMetricsWorkflow(params) {
  return runMetricsGate({
    candidateFiles: params.candidateFiles,
    pilotSet: new Set(params.pilotFiles),
    diffMap: params.diffMap,
    mode: params.args.mode,
    repoRoot: params.repoRoot,
    complexityMax: params.config.complexityMax,
    crapMax: params.config.crapMax,
  });
}

// Executes mutation gate workflow.
function executeMutationWorkflow(params) {
  return runMutationSuites({
    pilotFiles: params.pilotFiles,
    diffMap: params.diffMap,
    mode: params.args.mode,
    repoRoot: params.repoRoot,
    mergeBase: params.mergeBase,
    mutationMin: params.config.mutationMin,
  });
}

// Runs metrics check stage and reports status.
async function checkMetricsStage(params) {
  if (!params.args.metrics) {
    return { ran: false, failed: false, empty: false };
  }
  const res = await executeMetricsWorkflow(params);
  const empty = res.targetCount === 0 && !params.args.mutation;
  return { ran: true, failed: res.failed, empty };
}

// Runs mutation check stage and reports status.
function checkMutationStage(params) {
  if (!params.args.mutation) return false;
  return executeMutationWorkflow(params);
}

// Evaluates overall exit status from metrics and mutation results.
export function evaluateRunFailure(metricsFailed, mutationFailed) {
  if (metricsFailed) return 1;
  if (mutationFailed) return 1;
  return 0;
}

// Runs metrics and mutation checks based on CLI arguments.
async function runQualityChecks(params) {
  const metricsResult = await checkMetricsStage(params);
  if (metricsResult.empty) {
    console.log("quality: nothing to check");
    return 0;
  }

  const mutationFailed = checkMutationStage(params);
  return evaluateRunFailure(metricsResult.failed, mutationFailed);
}

// Main entry point for quality gate enforcement.
async function main() {
  const repoRoot = resolve(git(["rev-parse", "--show-toplevel"]).trim());
  const config = readQualityConfig(repoRoot);
  const args = parseArgs(process.argv.slice(2));

  let diffMap = new Map();
  let mergeBase = null;
  if (args.mode === "changed") {
    ({ mergeBase, diffMap } = loadChangedDiff(args.base, repoRoot));
  }

  const fileExists = (f) => existsSync(resolve(repoRoot, f));
  const candidateFiles = collectCandidateFiles({
    pilotFiles: config.pilotFiles,
    diffMap,
    mode: args.mode,
    fileExists,
  });

  if (candidateFiles.size === 0) {
    console.log("quality: nothing to check");
    process.exit(0);
  }

  const exitCode = await runQualityChecks({
    candidateFiles,
    pilotFiles: config.pilotFiles,
    diffMap,
    mergeBase,
    args,
    repoRoot,
    config,
  });
  process.exit(exitCode);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main();
}
