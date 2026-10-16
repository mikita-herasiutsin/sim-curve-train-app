import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import process from "node:process";
import { pathToFileURL } from "node:url";
import { ESLint } from "eslint";

function git(args, cwd) {
  return execFileSync("git", args, { encoding: "utf8", cwd });
}

export function quoteArg(arg) {
  const str = String(arg);
  if (str === "" || /[\s"&|<>^%()]/.test(str)) {
    return `"${str.replace(/"/g, '""')}"`;
  }
  return str;
}

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

export function addUntrackedToDiff(diffMap, untrackedFiles) {
  for (const file of untrackedFiles) {
    if (!diffMap.has(file)) {
      diffMap.set(file, [[1, Infinity]]);
    }
  }
  return diffMap;
}

export function overlaps(ranges, start, end) {
  if (!ranges || ranges.length === 0) return false;
  for (const [rStart, rEnd] of ranges) {
    if (rStart <= end && rEnd >= start) {
      return true;
    }
  }
  return false;
}

export function crap(complexity, coverage) {
  const cov = Math.max(0, Math.min(1, coverage));
  return complexity * complexity * Math.pow(1 - cov, 3) + complexity;
}

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

  if (total === 0) return 0;
  return covered / total;
}

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

  if (total === 0) return 0;
  return covered / total;
}

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

function countBracketDelta(text) {
  const stripped = text.replace(/"(?:\\.|[^"\\])*"/g, "");
  let delta = 0;
  for (let i = stripped.length - 1; i >= 0; i--) {
    if (stripped[i] === "]") delta++;
    else if (stripped[i] === "[") delta--;
  }
  return delta;
}

function parseIgnoreComment(trimmed) {
  if (!trimmed.startsWith("// quality-ignore")) return null;
  const rest = trimmed.slice(17);
  if (rest.startsWith(":")) {
    const reason = rest.slice(1).trim();
    return reason ? "ok" : "no-reason";
  }
  return rest.trim() === "" ? "no-reason" : "none";
}

export function suppression(sourceLines, startLine) {
  let bracketDepth = 0;
  for (let lineNum = startLine - 1; lineNum >= 1; lineNum--) {
    const rawLine = sourceLines[lineNum - 1];
    if (rawLine === undefined) continue;
    const trimmed = rawLine.trim();
    if (trimmed === "") continue;

    if (bracketDepth > 0) {
      bracketDepth += countBracketDelta(trimmed);
      continue;
    }

    const clean = trimmed.replace(/\/\/.*$/, "").trim();
    if (clean.endsWith("]") && !clean.startsWith("#[")) {
      bracketDepth += countBracketDelta(clean);
      if (bracketDepth > 0) continue;
    }

    if (SKIPPED_PREFIXES.test(trimmed)) {
      continue;
    }

    const ignoreResult = parseIgnoreComment(trimmed);
    if (ignoreResult !== null) {
      return ignoreResult;
    }
    return "none";
  }
  return "none";
}

export function mutationScore({ detected, undetected }) {
  const total = detected + undetected;
  if (total === 0) return null;
  return (detected / total) * 100;
}

const DETECTED_STATUSES = new Set(["Killed", "Timeout"]);
const UNDETECTED_STATUSES = new Set(["Survived", "NoCoverage"]);

function matchFileKey(normKey, fileSet) {
  if (!fileSet) return normKey;
  if (fileSet.has(normKey)) return normKey;
  for (const f of fileSet) {
    if (normKey.endsWith(f)) return f;
  }
  return null;
}

function toFileSet(files) {
  if (!files) return null;
  return new Set(files.map((f) => f.replace(/\\/g, "/")));
}

function formatMutantText(m) {
  return `${m.mutatorName}: ${m.replacement || ""}`;
}

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

  return { byFile, mutants };
}

const CARGO_DETECTED_STATUSES = new Set(["CaughtMutant", "Timeout"]);

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

export function cargoMutantsCounts(outcomes = []) {
  const byFile = new Map();
  const mutants = [];
  const seen = new Set();

  for (const item of outcomes) {
    if (!item) continue;
    const name = item.scenario?.Mutant?.name;
    if (name) {
      if (seen.has(name)) continue;
      seen.add(name);
    }
    tallyCargoOutcome(item, byFile, mutants);
  }

  return { byFile, mutants };
}

const TS_EXCLUSIONS = /\.(test|spec)\.[^/]+$/;

function rustFileGates(norm) {
  const underCrates = norm.startsWith("crates/");
  return {
    type: "rust",
    complexity: true,
    crap: underCrates,
    mutation: underCrates,
  };
}

function isExcludedTs(norm) {
  if (norm === "src/vitest-setup.ts") return true;
  if (norm.endsWith(".d.ts")) return true;
  return TS_EXCLUSIONS.test(norm);
}

function isTsSource(norm) {
  return norm.startsWith("src/") && norm.endsWith(".ts");
}

function isScriptSource(norm) {
  return norm.startsWith("scripts/") && norm.endsWith(".mjs");
}

function isSvelteSource(norm) {
  return norm.startsWith("src/") && norm.endsWith(".svelte");
}

function scriptCrap(norm, fileExists) {
  const sibling = norm.replace(/\.mjs$/, ".test.mjs");
  return Boolean(fileExists && fileExists(sibling));
}

function tsFileGates(norm, fileExists) {
  if (isExcludedTs(norm)) return null;
  if (isTsSource(norm)) return { type: "ts", complexity: true, crap: true, mutation: true };
  if (isScriptSource(norm)) {
    return { type: "ts", complexity: true, crap: scriptCrap(norm, fileExists), mutation: false };
  }
  if (isSvelteSource(norm)) return { type: "ts", complexity: true, crap: false, mutation: false };
  return null;
}

export function fileGates(relPath, fileExists = existsSync) {
  const norm = relPath.replace(/\\/g, "/");
  if (norm.endsWith(".rs")) {
    return rustFileGates(norm);
  }
  return tsFileGates(norm, fileExists);
}

export function hasGate(file, gate, fileExists = existsSync) {
  const gates = fileGates(file, fileExists);
  return Boolean(gates && gates[gate]);
}

function readBaseArg(argv, i) {
  const next = argv[i + 1];
  if (next && !next.startsWith("--")) {
    return next;
  }
  return null;
}

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

  const resolvedBase = mode === "changed" && !base ? "origin/main" : base;
  const resolvedMetrics = !metrics && !mutation ? true : metrics;
  return { mode, base: resolvedBase, metrics: resolvedMetrics, mutation };
}

const SPAN_PARENT_TYPES = new Set(["MethodDefinition", "Property", "PropertyDefinition"]);

function getSpanStart(node) {
  let curr = node;
  while (curr.parent) {
    const p = curr.parent;
    if (SPAN_PARENT_TYPES.has(p.type)) {
      return p.loc.start;
    }
    if (p.type === "VariableDeclarator" && p.init === curr) {
      curr = p;
      continue;
    }
    if (p.type === "VariableDeclaration" && curr.type === "VariableDeclarator") {
      curr = p;
      continue;
    }
    if (p.type === "ExportNamedDeclaration" && curr.type === "VariableDeclaration") {
      return p.loc.start;
    }
    break;
  }
  return curr.loc.start;
}

function getFunctionName(node) {
  if (node.id?.name) return node.id.name;
  if (
    node.parent?.type === "VariableDeclarator" &&
    node.parent.init === node &&
    node.parent.id?.type === "Identifier"
  ) {
    return node.parent.id.name;
  }
  return undefined;
}

export function createQualityEslint() {
  return new ESLint({
    allowInlineConfig: false,
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
                      message: getFunctionName(node) ?? "span",
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
        name: msg.message !== "span" ? msg.message : undefined,
      });
    }
    if (msg.ruleId === "complexity") {
      complexityMsgs.push(msg);
    }
  }
  return { spans, complexityMsgs };
}

function parseComplexityMessage(message) {
  const compMatch = message.match(/complexity of (\d+)/);
  if (!compMatch) return null;
  const nameMatch = message.match(/'([^']+)'/);
  return {
    complexity: parseInt(compMatch[1], 10),
    name: nameMatch ? nameMatch[1] : "<anonymous>",
  };
}

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
      const fnName =
        parsed.name && parsed.name !== "<anonymous>" ? parsed.name : (span.name ?? "<anonymous>");
      functions.push({
        file: relPath,
        name: fnName,
        startLine: span.startLine,
        endLine: span.endLine,
        complexity: parsed.complexity,
      });
    }
  }
  return functions;
}

export function collectTsFunctions(lintResults, repoRoot) {
  const functions = [];
  for (const fileRes of lintResults) {
    functions.push(...extractFileFunctions(fileRes, repoRoot));
  }
  return functions;
}

export function findOutcomesPath(outDir, repoRoot, fileExists) {
  const p = resolve(repoRoot, outDir, "mutants.out/outcomes.json");
  return fileExists(p) ? p : null;
}

// cargo-mutants skips outcomes.json only when it finds no mutants and exits 0. Any other missing
// file means it failed, and scoring that as "no mutants" would pass the gate.
export function readOutcomes(outDir, repoRoot, fileReader, fileExists, status) {
  const exists = fileExists || existsSync;
  const read = fileReader || readFileSync;
  const p = findOutcomesPath(outDir, repoRoot, exists);
  if (!p) {
    if (status === 0) {
      return [];
    }
    throw new Error(`cargo-mutants wrote no outcomes.json under ${outDir}`);
  }
  const data = JSON.parse(read(p, "utf8"));
  const outcomes = data.outcomes ?? [];
  for (const item of outcomes) {
    if (item.scenario === "Baseline" && item.summary !== "Success") {
      throw new Error("cargo-mutants baseline failed");
    }
  }
  const totalMutants = data.total_mutants ?? 0;
  if (totalMutants > 0) {
    const hasViable = outcomes.some(
      (o) =>
        o.summary === "CaughtMutant" || o.summary === "MissedMutant" || o.summary === "Timeout",
    );
    if (!hasViable) {
      throw new Error("cargo-mutants tested no viable mutants");
    }
  }
  return outcomes;
}

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

function collectRustFunctions(rsCandidates, repoRoot, runner) {
  if (rsCandidates.length === 0) return [];
  const run = runner || runCommand;
  const output = run("cargo", ["run", "-q", "-p", "complexity", "--", ...rsCandidates], {
    cwd: repoRoot,
    encoding: "utf8",
  });
  return parseRustFunctionsJson(output, repoRoot);
}

export function needsRustCoverage(functions) {
  for (const fn of functions) {
    if (fn.file.endsWith(".rs") && hasGate(fn.file, "crap")) {
      return true;
    }
  }
  return false;
}

export function findTsCrapFiles(functions) {
  const files = new Set();
  for (const fn of functions) {
    if (!fn.file.endsWith(".rs") && hasGate(fn.file, "crap")) {
      files.add(fn.file);
    }
  }
  return Array.from(files);
}

function runRustCoverage(repoRoot, runner, fileReader) {
  const run = runner || runCommand;
  const read = fileReader || readFileSync;
  mkdirSync(resolve(repoRoot, "coverage"), { recursive: true });
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
    { cwd: repoRoot, stdio: ["ignore", "ignore", "inherit"] },
  );
  const lcovText = read(resolve(repoRoot, "coverage/rust.lcov"), "utf8");
  return parseLcov(lcovText, repoRoot);
}

export function parseTsCoverageJson(json, repoRoot) {
  const map = new Map();
  for (const [k, v] of Object.entries(json)) {
    map.set(toRepoRelative(k, repoRoot), v);
  }
  return map;
}

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

function collectCoverage(targetFunctions, repoRoot) {
  const rustLcov = needsRustCoverage(targetFunctions) ? runRustCoverage(repoRoot) : null;
  const tsCrapFiles = findTsCrapFiles(targetFunctions);
  const tsCovMap = tsCrapFiles.length > 0 ? runTsCoverage(tsCrapFiles, repoRoot) : null;
  return { rustLcov, tsCovMap };
}

function calculateColWidths(headers, rows) {
  return {
    loc: Math.max(headers.loc.length, ...rows.map((r) => r.loc.length)),
    name: Math.max(headers.name.length, ...rows.map((r) => r.name.length)),
    complexity: Math.max(headers.complexity.length, ...rows.map((r) => r.complexity.length)),
    coverage: Math.max(headers.coverage.length, ...rows.map((r) => r.coverage.length)),
    crap: Math.max(headers.crap.length, ...rows.map((r) => r.crap.length)),
  };
}

function formatReportRow(r, colWidths) {
  return `${r.loc.padEnd(colWidths.loc)}  ${r.name.padEnd(colWidths.name)}  ${r.complexity.padEnd(colWidths.complexity)}  ${r.coverage.padEnd(colWidths.coverage)}  ${r.crap.padEnd(colWidths.crap)}`;
}

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

function printFailingTable(failingFunctions) {
  for (const line of formatReport(failingFunctions)) {
    console.log(line);
  }
}

export function isTargetFunction(fn, pilotSet, diffMap, mode) {
  if (pilotSet.has(fn.file)) return true;
  if (mode === "changed") {
    const ranges = diffMap.get(fn.file);
    return overlaps(ranges, fn.startLine, fn.endLine);
  }
  return false;
}

function computeFunctionCoverage(fn, gate, { rustLcov, tsCovMap }) {
  if (gate.type === "rust") {
    const lcov = rustLcov ? rustLcov.get(fn.file) : null;
    return lineCoverage(lcov, fn.startLine, fn.endLine);
  }
  const cov = tsCovMap ? tsCovMap.get(fn.file) : null;
  return istanbulCoverage(cov, fn.startLine, fn.endLine);
}

export function isFunctionFailing(fn, { complexityMax, crapMax, supp }) {
  if (supp === "no-reason") return true;
  if (supp === "ok") return false;
  if (fn.complexity > complexityMax) return true;
  if (fn.crap !== null && fn.crap > crapMax) return true;
  return false;
}

export function evaluateFunctionMetrics(
  fn,
  { rustLcov, tsCovMap, complexityMax, crapMax, lines, fileExists },
) {
  const gate = fileGates(fn.file, fileExists);
  const appliesCrap = Boolean(gate && gate.crap);
  const cov = appliesCrap ? computeFunctionCoverage(fn, gate, { rustLcov, tsCovMap }) : null;
  const crapScore = appliesCrap ? crap(fn.complexity, cov) : null;

  fn.coverage = cov;
  fn.crap = crapScore;

  const supp = suppression(lines, fn.startLine);
  return isFunctionFailing(fn, { complexityMax, crapMax, supp });
}

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

async function lintTsCandidates(tsCandidates, repoRoot) {
  if (tsCandidates.length === 0) return [];
  const eslint = createQualityEslint();
  const lintResults = await eslint.lintFiles(tsCandidates.map((f) => resolve(repoRoot, f)));
  return collectTsFunctions(lintResults, repoRoot);
}

async function collectTargetFunctions({ candidateFiles, pilotSet, diffMap, mode, repoRoot }) {
  const rsCandidates = [...candidateFiles].filter((f) => f.endsWith(".rs"));
  const tsCandidates = [...candidateFiles].filter((f) => !f.endsWith(".rs"));

  const rustFunctions = collectRustFunctions(rsCandidates, repoRoot);
  const tsFunctions = await lintTsCandidates(tsCandidates, repoRoot);
  const allFunctions = [...rustFunctions, ...tsFunctions];

  return allFunctions.filter((fn) => isTargetFunction(fn, pilotSet, diffMap, mode));
}

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

export function selectTsMutationTargets({ pilotFiles, diffMap, mode, fileExists }) {
  const tsPilot = pilotFiles
    .filter((f) => f.startsWith("src/") && f.endsWith(".ts"))
    .filter(fileExists);
  if (mode === "pilot") {
    return { targetFiles: tsPilot, mutateList: tsPilot };
  }

  const tsChanged = [...diffMap.keys()].filter(
    (f) => !f.endsWith(".rs") && hasGate(f, "mutation", fileExists) && fileExists(f),
  );
  const targetFiles = Array.from(new Set([...tsPilot, ...tsChanged]));
  const mutateList = [...buildStrykerMutateRanges(tsChanged, diffMap), ...tsPilot];
  return { targetFiles, mutateList };
}

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
    // Stryker exits with a non-zero code when any mutant survives; evaluateStrykerScore inspects the report.
  }
}

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

export function selectRustMutationTargets({ pilotFiles, diffMap, mode, fileExists }) {
  const rustPilot = pilotFiles
    .filter((f) => f.startsWith("crates/") && f.endsWith(".rs"))
    .filter(fileExists);
  if (mode === "pilot") {
    return { targetFiles: rustPilot, rustPilot };
  }
  const rustChanged = [...diffMap.keys()].filter(
    (f) => f.endsWith(".rs") && hasGate(f, "mutation", fileExists) && fileExists(f),
  );
  const targetFiles = Array.from(new Set([...rustPilot, ...rustChanged]));
  return { targetFiles, rustPilot, rustChanged };
}

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

function diffUntracked(file, repoRoot, runner) {
  const norm = file.replace(/\\/g, "/");
  try {
    return runner(
      [
        "diff",
        "--no-index",
        "--no-color",
        "--src-prefix=a/",
        "--dst-prefix=b/",
        "--no-ext-diff",
        "/dev/null",
        norm,
      ],
      repoRoot,
    );
  } catch (err) {
    if (err && (err.status === 1 || err.status === 0)) {
      return err.stdout ?? "";
    }
    throw err;
  }
}

export function prepareMutantsDiff(repoRoot, mergeBase, gitRunner, rustPilot = []) {
  const runner = gitRunner || git;
  mkdirSync(resolve(repoRoot, "mutants.out"), { recursive: true });
  const excludeArgs = rustPilot.map((p) => `:(exclude)${p}`);
  const cratesDiff = runner(
    [
      "diff",
      "--no-color",
      "--src-prefix=a/",
      "--dst-prefix=b/",
      "--no-ext-diff",
      mergeBase,
      "--",
      "crates",
      ...excludeArgs,
    ],
    repoRoot,
  );
  let inDiff = cratesDiff;
  const untrackedText = runner(["ls-files", "--others", "--exclude-standard"], repoRoot);
  const untrackedFiles = parseUntrackedFiles(untrackedText);
  for (const file of untrackedFiles) {
    const norm = file.replace(/\\/g, "/");
    if (norm.startsWith("crates/") && norm.endsWith(".rs")) {
      inDiff += diffUntracked(norm, repoRoot, runner);
    }
  }
  writeFileSync(resolve(repoRoot, "mutants.out/in.diff"), inDiff, "utf8");
}

// cargo-mutants exits with 2 when mutants survive and 3 when tests time out; these are normal outcomes.
export function execCargoMutants(args, repoRoot, runner) {
  const run = runner || runCommand;
  let status = 0;
  try {
    run("cargo", args, { cwd: repoRoot, stdio: "inherit" });
  } catch (err) {
    status = err?.status ?? 1;
  }
  if (status === 0 || status === 2 || status === 3) {
    return status;
  }
  throw new Error(`cargo-mutants exited with ${status}`);
}

export function aggregateMutantOutcomes(calls, repoRoot, fileReader, fileExists) {
  const allOutcomes = [];
  for (const call of calls) {
    allOutcomes.push(...readOutcomes(call.outDir, repoRoot, fileReader, fileExists, call.status));
  }
  return cargoMutantsCounts(allOutcomes);
}

function executeMutantCalls(calls, repoRoot, mergeBase, rustPilot) {
  for (const call of calls) {
    if (call.needsDiff) {
      prepareMutantsDiff(repoRoot, mergeBase, undefined, rustPilot);
    }
    call.status = execCargoMutants(call.args, repoRoot);
  }
}

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
  executeMutantCalls(calls, repoRoot, mergeBase, rustPilot);

  const totals = aggregateMutantOutcomes(calls, repoRoot);
  return evaluateMutationCounts("rust", totals, mutationMin);
}

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

function isCandidateFile(f, fileExists) {
  if (fileGates(f, fileExists) === null) return false;
  return fileExists(f);
}

function addGatedChangedFiles(candidateSet, diffMap, fileExists) {
  for (const f of diffMap.keys()) {
    if (isCandidateFile(f, fileExists)) {
      candidateSet.add(f);
    }
  }
}

export function collectCandidateFiles({ pilotFiles, diffMap, mode, fileExists }) {
  const candidateSet = new Set(pilotFiles.filter(fileExists));
  if (mode === "changed") {
    addGatedChangedFiles(candidateSet, diffMap, fileExists);
  }
  return candidateSet;
}

function loadChangedDiff(base, repoRoot, gitRunner) {
  const runner = gitRunner || git;
  const mergeBase = runner(["merge-base", base, "HEAD"], repoRoot).trim();
  const diffText = runner(
    ["diff", "-U0", "--no-color", "--src-prefix=a/", "--dst-prefix=b/", "--no-ext-diff", mergeBase],
    repoRoot,
  );
  const diffMap = parseDiff(diffText);
  const untrackedText = runner(["ls-files", "--others", "--exclude-standard"], repoRoot);
  const untracked = parseUntrackedFiles(untrackedText);
  addUntrackedToDiff(diffMap, untracked);
  return { mergeBase, diffMap };
}

export function evaluateRunFailure(metricsFailed, mutationFailed) {
  if (metricsFailed) return 1;
  if (mutationFailed) return 1;
  return 0;
}

export async function runQualityChecks(params) {
  let metricsFailed = false;
  if (params.args.metrics) {
    const res = await runMetricsGate({
      candidateFiles: params.candidateFiles,
      pilotSet: new Set(params.pilotFiles),
      diffMap: params.diffMap,
      mode: params.args.mode,
      repoRoot: params.repoRoot,
      complexityMax: params.config.complexityMax,
      crapMax: params.config.crapMax,
    });
    metricsFailed = res.failed;
  }

  let mutationFailed = false;
  if (params.args.mutation) {
    const tsFailed = runTsMutation({
      pilotFiles: params.pilotFiles,
      diffMap: params.diffMap,
      mode: params.args.mode,
      repoRoot: params.repoRoot,
      mutationMin: params.config.mutationMin,
    });
    const rustFailed = runRustMutation({
      pilotFiles: params.pilotFiles,
      diffMap: params.diffMap,
      mode: params.args.mode,
      repoRoot: params.repoRoot,
      mergeBase: params.mergeBase,
      mutationMin: params.config.mutationMin,
    });
    mutationFailed = tsFailed || rustFailed;
  }

  return evaluateRunFailure(metricsFailed, mutationFailed);
}

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
  const pilotFiles = config.pilotFiles.filter(fileExists);
  const candidateFiles = collectCandidateFiles({
    pilotFiles,
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
    pilotFiles,
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
