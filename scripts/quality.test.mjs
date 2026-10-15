import { describe, expect, it } from "vitest";
import {
  addUntrackedToDiff,
  buildCommandLine,
  buildCargoMutantsCalls,
  buildStrykerMutateRanges,
  cargoMutantsCounts,
  collectCandidateFiles,
  collectTsFunctions,
  crap,
  evaluateFunctionMetrics,
  evaluateRunFailure,
  fileGates,
  findOutcomesPath,
  findTsCrapFiles,
  formatReport,
  isFunctionFailing,
  isTargetFunction,
  istanbulCoverage,
  lineCoverage,
  matchSpan,
  mutationScore,
  needsRustCoverage,
  overlaps,
  parseArgs,
  parseDiff,
  parseLcov,
  parseRustFunctionsJson,
  parseTsCoverageJson,
  parseUntrackedFiles,
  quoteArg,
  readOutcomes,
  readQualityConfig,
  runCommand,
  selectRustMutationTargets,
  selectTsMutationTargets,
  strykerCounts,
  suppression,
} from "./quality.mjs";

describe("parseDiff", () => {
  it("parses additions with counts", () => {
    const diff = `
--- a/src/foo.ts
+++ b/src/foo.ts
@@ -10,3 +10,5 @@
`;
    const map = parseDiff(diff);
    expect(map.get("src/foo.ts")).toEqual([[10, 14]]);
  });

  it("parses hunks where count is omitted", () => {
    const diff = `
--- a/src/foo.ts
+++ b/src/foo.ts
@@ -15 +15 @@
`;
    const map = parseDiff(diff);
    expect(map.get("src/foo.ts")).toEqual([[15, 15]]);
  });

  it("handles pure deletions where count is zero", () => {
    const diff = `
--- a/src/foo.ts
+++ b/src/foo.ts
@@ -20,2 +20,0 @@
@@ -1,5 +0,0 @@
`;
    const map = parseDiff(diff);
    expect(map.get("src/foo.ts")).toEqual([
      [20, 20],
      [1, 1],
    ]);
  });

  it("skips deleted files targeting /dev/null", () => {
    const diff = `
--- a/src/deleted.ts
+++ /dev/null
@@ -1,10 +0,0 @@
`;
    const map = parseDiff(diff);
    expect(map.has("src/deleted.ts")).toBe(false);
  });

  it("parses multiple files in one diff", () => {
    const diff = `
--- a/src/a.ts
+++ b/src/a.ts
@@ -1,2 +1,3 @@
--- a/src/b.ts
+++ b/src/b.ts
@@ -5 +5,2 @@
`;
    const map = parseDiff(diff);
    expect(map.get("src/a.ts")).toEqual([[1, 3]]);
    expect(map.get("src/b.ts")).toEqual([[5, 6]]);
  });
});

describe("overlaps", () => {
  const ranges = [
    [10, 20],
    [30, 40],
  ];

  it("detects interval intersections correctly", () => {
    expect(overlaps(ranges, 1, 9)).toBe(false);
    expect(overlaps(ranges, 1, 10)).toBe(true);
    expect(overlaps(ranges, 12, 18)).toBe(true);
    expect(overlaps(ranges, 20, 25)).toBe(true);
    expect(overlaps(ranges, 21, 29)).toBe(false);
    expect(overlaps(ranges, 25, 35)).toBe(true);
    expect(overlaps(ranges, 41, 50)).toBe(false);
  });

  it("returns false for empty or missing ranges", () => {
    expect(overlaps([], 10, 20)).toBe(false);
    expect(overlaps(undefined, 10, 20)).toBe(false);
  });
});

describe("crap", () => {
  it("matches standard test cases", () => {
    expect(crap(12, 1)).toBe(12);
    expect(crap(4, 0)).toBe(20);
    expect(crap(3, 0)).toBe(12);
  });
});

describe("parseLcov", () => {
  it("normalises Windows paths to repo-relative paths", () => {
    const text = `
TN:
SF:C:\\workspace\\app\\crates\\core\\src\\audio_map.rs
DA:10,1
DA:11,0
DA:12,3
end_of_record
`;
    const map = parseLcov(text, "C:/workspace/app");
    expect(map.has("crates/core/src/audio_map.rs")).toBe(true);
    const lineMap = map.get("crates/core/src/audio_map.rs");
    expect(lineMap.get(10)).toBe(1);
    expect(lineMap.get(11)).toBe(0);
    expect(lineMap.get(12)).toBe(3);
  });

  it("matches case-insensitively on Windows drive letters", () => {
    const text = `
SF:c:\\workspace\\app\\src\\lib\\foo.ts
DA:5,2
end_of_record
`;
    const map = parseLcov(text, "C:/workspace/app");
    expect(map.has("src/lib/foo.ts")).toBe(true);
    expect(map.get("src/lib/foo.ts").get(5)).toBe(2);
  });
});

describe("lineCoverage", () => {
  it("returns zero when file is missing from report", () => {
    expect(lineCoverage(undefined, 1, 10)).toBe(0);
  });

  it("returns one when no lines are instrumented in range", () => {
    const lines = new Map([[20, 1]]);
    expect(lineCoverage(lines, 1, 10)).toBe(1);
  });

  it("computes partial coverage correctly", () => {
    const lines = new Map([
      [5, 1],
      [6, 0],
      [20, 1],
    ]);
    expect(lineCoverage(lines, 1, 10)).toBe(0.5);
  });
});

describe("istanbulCoverage", () => {
  it("returns zero when file coverage is missing", () => {
    expect(istanbulCoverage(undefined, 1, 10)).toBe(0);
  });

  it("returns one when no statements start in range", () => {
    const fileCov = {
      statementMap: { 0: { start: { line: 25 } } },
      s: { 0: 0 },
    };
    expect(istanbulCoverage(fileCov, 1, 10)).toBe(1);
  });

  it("computes partial statement coverage in range", () => {
    const fileCov = {
      statementMap: {
        0: { start: { line: 5 } },
        1: { start: { line: 7 } },
        2: { start: { line: 15 } },
      },
      s: { 0: 3, 1: 0, 2: 0 },
    };
    expect(istanbulCoverage(fileCov, 1, 10)).toBe(0.5);
  });
});

describe("matchSpan", () => {
  it("picks the nested arrow inside an enclosing method", () => {
    const methodSpan = {
      startLine: 1,
      startColumn: 1,
      endLine: 10,
      endColumn: 20,
    };
    const arrowSpan = {
      startLine: 3,
      startColumn: 5,
      endLine: 5,
      endColumn: 10,
    };
    const spans = [methodSpan, arrowSpan];

    expect(matchSpan(spans, 4, 6)).toBe(arrowSpan);
    expect(matchSpan(spans, 2, 2)).toBe(methodSpan);
    expect(matchSpan(spans, 15, 1)).toBeUndefined();
  });
});

describe("suppression", () => {
  it("detects valid suppression reason", () => {
    const lines = ["// quality-ignore: complex algorithm", "function foo() {}"];
    expect(suppression(lines, 2)).toBe("ok");
  });

  it("skips attributes and doc comment lines", () => {
    const lines = [
      "// quality-ignore: necessary complexity",
      "#[must_use]",
      "/// doc comment",
      "#[inline]",
      "fn complex_calc() {}",
    ];
    expect(suppression(lines, 5)).toBe("ok");
  });

  it("reports no-reason when reason is missing or empty", () => {
    expect(suppression(["// quality-ignore", "fn foo() {}"], 2)).toBe("no-reason");
    expect(suppression(["// quality-ignore:", "fn foo() {}"], 2)).toBe("no-reason");
    expect(suppression(["// quality-ignore:   ", "fn foo() {}"], 2)).toBe("no-reason");
  });

  it("returns none when no suppression comment is present", () => {
    expect(suppression(["const x = 1;", "function foo() {}"], 2)).toBe("none");
    expect(suppression(["// regular comment", "function foo() {}"], 2)).toBe("none");
    expect(suppression(["function foo() {}"], 1)).toBe("none");
  });
});

describe("mutationScore", () => {
  it("computes percentage correctly", () => {
    expect(mutationScore({ detected: 8, undetected: 2 })).toBe(80);
    expect(mutationScore({ detected: 0, undetected: 5 })).toBe(0);
  });

  it("returns null when total mutant count is zero", () => {
    expect(mutationScore({ detected: 0, undetected: 0 })).toBeNull();
  });
});

describe("strykerCounts", () => {
  it("extracts detected and undetected counts from report", () => {
    const report = {
      files: {
        "src/lib/foo.ts": {
          mutants: [
            {
              status: "Killed",
              mutatorName: "BinaryExpression",
              replacement: "-",
              location: { start: { line: 10 } },
            },
            {
              status: "Timeout",
              mutatorName: "EqualityOperator",
              replacement: "!==",
              location: { start: { line: 12 } },
            },
            {
              status: "Survived",
              mutatorName: "BooleanLiteral",
              replacement: "false",
              location: { start: { line: 15 } },
            },
            {
              status: "NoCoverage",
              mutatorName: "BlockStatement",
              replacement: "{}",
              location: { start: { line: 18 } },
            },
            {
              status: "Ignored",
              mutatorName: "ConditionalExpression",
              replacement: "true",
              location: { start: { line: 20 } },
            },
          ],
        },
        "src/lib/bar.ts": {
          mutants: [
            {
              status: "Survived",
              mutatorName: "StringLiteral",
              replacement: '""',
              location: { start: { line: 5 } },
            },
          ],
        },
      },
    };

    const counts = strykerCounts(report, ["src/lib/foo.ts"]);
    expect(counts.detected).toBe(2);
    expect(counts.undetected).toBe(2);
    expect(counts.mutants).toEqual([
      { file: "src/lib/foo.ts", line: 15, text: "BooleanLiteral: false" },
      { file: "src/lib/foo.ts", line: 18, text: "BlockStatement: {}" },
    ]);
  });
});

describe("cargoMutantsCounts", () => {
  it("extracts counts and undetected mutant names", () => {
    const outcomes = [
      {
        summary: "CaughtMutant",
        scenario: { Mutant: { name: "fn a -> bool" } },
      },
      {
        summary: "Timeout",
        scenario: { Mutant: { name: "fn b -> bool" } },
      },
      {
        summary: "MissedMutant",
        scenario: { Mutant: { name: "fn c -> bool" } },
      },
      {
        summary: "Unviable",
        scenario: { Mutant: { name: "fn d -> bool" } },
      },
      { summary: "Success", scenario: "Baseline" },
    ];

    const counts = cargoMutantsCounts(outcomes);
    expect(counts.detected).toBe(2);
    expect(counts.undetected).toBe(1);
    expect(counts.mutants).toEqual([{ text: "fn c -> bool" }]);
  });
});

describe("parseUntrackedFiles and addUntrackedToDiff", () => {
  it("parses output of git ls-files --others", () => {
    const output = "  src/components/NewButton.svelte \r\ncrates\\core\\src\\new_mod.rs\n\n";
    const files = parseUntrackedFiles(output);
    expect(files).toEqual(["src/components/NewButton.svelte", "crates/core/src/new_mod.rs"]);
  });

  it("adds untracked files with full file range and preserves existing diff ranges", () => {
    const diffMap = new Map([["src/existing.ts", [[5, 10]]]]);
    addUntrackedToDiff(diffMap, ["src/existing.ts", "src/new.ts"]);
    expect(diffMap.get("src/existing.ts")).toEqual([[5, 10]]);
    expect(diffMap.get("src/new.ts")).toEqual([[1, Infinity]]);
  });
});

describe("quoteArg and buildCommandLine", () => {
  it("leaves unquoted arguments without spaces or special characters", () => {
    expect(quoteArg("cargo")).toBe("cargo");
    expect(quoteArg("src/foo.ts")).toBe("src/foo.ts");
    expect(quoteArg("--metrics")).toBe("--metrics");
  });

  it("quotes arguments with spaces and special characters", () => {
    expect(quoteArg("")).toBe('""');
    expect(quoteArg("foo bar")).toBe('"foo bar"');
    expect(quoteArg("a&b")).toBe('"a&b"');
    expect(quoteArg("a|b")).toBe('"a|b"');
    expect(quoteArg("a<b")).toBe('"a<b"');
    expect(quoteArg("a>b")).toBe('"a>b"');
    expect(quoteArg("a^b")).toBe('"a^b"');
    expect(quoteArg("a%b")).toBe('"a%b"');
    expect(quoteArg("(nested)")).toBe('"(nested)"');
    expect(quoteArg('foo"bar')).toBe('"foo\\"bar"');
  });

  it("builds a single command line string from an executable and args", () => {
    const cmd = buildCommandLine("npx", ["vitest", "related", "--run", "src/foo bar.ts"]);
    expect(cmd).toBe('npx vitest related --run "src/foo bar.ts"');
  });

  it("runs command through runCommand helper", () => {
    const output = runCommand("node", ["-e", "process.stdout.write('test-ok')"], {
      encoding: "utf8",
    });
    expect(output).toBe("test-ok");
  });
});

describe("fileGates", () => {
  it("assigns gate rules based on path conventions", () => {
    expect(fileGates("crates/core/src/lib.rs")).toEqual({
      type: "rust",
      complexity: true,
      crap: true,
      mutation: true,
    });
    expect(fileGates("src-tauri/src/main.rs")).toEqual({
      type: "rust",
      complexity: true,
      crap: false,
      mutation: false,
    });
    expect(fileGates("src/vitest-setup.ts")).toBeNull();
    expect(fileGates("src/types.d.ts")).toBeNull();
    expect(fileGates("src/button.test.ts")).toBeNull();
    expect(fileGates("src/button.spec.ts")).toBeNull();
    expect(fileGates("src/app.ts")).toEqual({
      type: "ts",
      complexity: true,
      crap: true,
      mutation: true,
    });
    expect(fileGates("scripts/quality.mjs")).toEqual({
      type: "ts",
      complexity: true,
      crap: true,
      mutation: false,
    });
    expect(fileGates("src/App.svelte")).toEqual({
      type: "ts",
      complexity: true,
      crap: false,
      mutation: false,
    });
    expect(fileGates("package.json")).toBeNull();
  });
});

describe("parseArgs", () => {
  it("defaults to changed mode targeting origin/main with metrics enabled", () => {
    const args = parseArgs([]);
    expect(args).toEqual({
      mode: "changed",
      base: "origin/main",
      metrics: true,
      mutation: false,
    });
  });

  it("parses pilot mode", () => {
    const args = parseArgs(["--pilot"]);
    expect(args.mode).toBe("pilot");
    expect(args.metrics).toBe(true);
  });

  it("parses custom merge base after --changed", () => {
    const args = parseArgs(["--changed", "upstream/master"]);
    expect(args.mode).toBe("changed");
    expect(args.base).toBe("upstream/master");
  });

  it("handles explicit metrics and mutation flags", () => {
    const args = parseArgs(["--mutation"]);
    expect(args.metrics).toBe(false);
    expect(args.mutation).toBe(true);

    const both = parseArgs(["--metrics", "--mutation"]);
    expect(both.metrics).toBe(true);
    expect(both.mutation).toBe(true);
  });
});

describe("collectTsFunctions", () => {
  it("extracts named and anonymous function descriptors from ESLint messages", () => {
    const lintResults = [
      {
        filePath: "C:/repo/src/lib/foo.ts",
        messages: [
          {
            ruleId: "quality/span",
            line: 10,
            column: 1,
            endLine: 20,
            endColumn: 2,
          },
          {
            ruleId: "complexity",
            line: 10,
            column: 1,
            message: "Function 'compute' has a complexity of 5.",
          },
          {
            ruleId: "quality/span",
            line: 25,
            column: 5,
            endLine: 30,
            endColumn: 10,
          },
          {
            ruleId: "complexity",
            line: 25,
            column: 5,
            message: "Arrow function has a complexity of 2.",
          },
        ],
      },
    ];

    const fns = collectTsFunctions(lintResults, "C:/repo");
    expect(fns).toEqual([
      {
        file: "src/lib/foo.ts",
        name: "compute",
        startLine: 10,
        endLine: 20,
        complexity: 5,
      },
      {
        file: "src/lib/foo.ts",
        name: "<anonymous>",
        startLine: 25,
        endLine: 30,
        complexity: 2,
      },
    ]);
  });
});

describe("findOutcomesPath and readOutcomes", () => {
  it("prefers mutants.out/outcomes.json over outcomes.json", () => {
    const fakeExists = (path) => path.replace(/\\/g, "/").includes("mutants.out/outcomes.json");
    const p = findOutcomesPath("target", "C:/repo", fakeExists);
    expect(p).not.toBeNull();
    expect(p.replace(/\\/g, "/")).toContain("mutants.out");
  });

  it("returns outcomes.json when nested file is absent", () => {
    const fakeExists = (path) => {
      const norm = path.replace(/\\/g, "/");
      return norm.endsWith("outcomes.json") && !norm.includes("mutants.out");
    };
    const p = findOutcomesPath("target", "C:/repo", fakeExists);
    expect(p).not.toBeNull();
  });

  it("reads and parses outcomes data or returns empty list", () => {
    const fakeExists = () => true;
    const fakeReader = () => JSON.stringify({ outcomes: [{ summary: "CaughtMutant" }], caught: 1 });
    const outcomes = readOutcomes("out", "C:/repo", fakeReader, fakeExists);
    expect(outcomes).toEqual([{ summary: "CaughtMutant" }]);

    const missing = readOutcomes("out", "C:/repo", fakeReader, () => false);
    expect(missing).toEqual([]);
  });
});

describe("needsRustCoverage and findTsCrapFiles", () => {
  it("detects whether rust functions need crap coverage", () => {
    expect(needsRustCoverage([{ file: "src-tauri/src/main.rs" }])).toBe(false);
    expect(needsRustCoverage([{ file: "crates/core/src/lib.rs" }])).toBe(true);
  });

  it("finds unique TypeScript files needing CRAP coverage", () => {
    const functions = [
      { file: "src/foo.ts" },
      { file: "src/foo.ts" },
      { file: "src/App.svelte" },
      { file: "scripts/quality.mjs" },
    ];
    const files = findTsCrapFiles(functions);
    expect(files).toEqual(["src/foo.ts", "scripts/quality.mjs"]);
  });
});

describe("parseRustFunctionsJson and parseTsCoverageJson", () => {
  it("normalises paths in rust complexity output", () => {
    const json = JSON.stringify([
      {
        file: "crates\\core\\src\\lib.rs",
        name: "foo",
        startLine: 1,
        endLine: 10,
        complexity: 2,
      },
    ]);
    const fns = parseRustFunctionsJson(json, "C:/repo");
    expect(fns[0].file).toBe("crates/core/src/lib.rs");
  });

  it("normalises paths in typescript coverage final json", () => {
    const json = {
      "C:\\repo\\src\\foo.ts": { s: { 0: 1 } },
    };
    const map = parseTsCoverageJson(json, "C:/repo");
    expect(map.has("src/foo.ts")).toBe(true);
  });
});

describe("formatReport", () => {
  it("formats failing functions into report table lines", () => {
    const failing = [
      {
        file: "src/foo.ts",
        startLine: 10,
        name: "complexFn",
        complexity: 15,
        coverage: 0.8,
        crap: 16.5,
      },
    ];
    const lines = formatReport(failing);
    expect(lines.length).toBe(2);
    expect(lines[0]).toContain("file:startLine");
    expect(lines[1]).toContain("src/foo.ts:10");
    expect(lines[1]).toContain("complexFn");
    expect(lines[1]).toContain("15");
    expect(lines[1]).toContain("80%");
  });
});

describe("isTargetFunction", () => {
  it("matches pilot files unconditionally", () => {
    const fn = { file: "src/foo.ts", startLine: 10, endLine: 20 };
    expect(isTargetFunction(fn, new Set(["src/foo.ts"]), new Map(), "changed")).toBe(true);
  });

  it("matches changed files intersecting diff ranges", () => {
    const fn = { file: "src/foo.ts", startLine: 10, endLine: 20 };
    const diffMap = new Map([["src/foo.ts", [[15, 25]]]]);
    expect(isTargetFunction(fn, new Set(), diffMap, "changed")).toBe(true);

    const nonOverlapping = new Map([["src/foo.ts", [[30, 40]]]]);
    expect(isTargetFunction(fn, new Set(), nonOverlapping, "changed")).toBe(false);
  });
});

describe("isFunctionFailing and evaluateFunctionMetrics", () => {
  it("handles suppressions and metric limit thresholds", () => {
    const fn = { complexity: 15, crap: 20 };
    expect(isFunctionFailing(fn, { complexityMax: 12, crapMax: 12, supp: "no-reason" })).toBe(true);
    expect(isFunctionFailing(fn, { complexityMax: 12, crapMax: 12, supp: "ok" })).toBe(false);
    expect(isFunctionFailing(fn, { complexityMax: 12, crapMax: 12, supp: "none" })).toBe(true);

    const validFn = { complexity: 5, crap: 8 };
    expect(isFunctionFailing(validFn, { complexityMax: 12, crapMax: 12, supp: "none" })).toBe(
      false,
    );
  });

  it("evaluates metric failures with source suppression check", () => {
    const fn = { file: "src/foo.ts", startLine: 2, complexity: 15 };
    const lines = ["// quality-ignore: known heavy parser", "function parse() {}"];
    const failed = evaluateFunctionMetrics(fn, {
      rustLcov: null,
      tsCovMap: null,
      complexityMax: 12,
      crapMax: 12,
      lines,
    });
    expect(failed).toBe(false);
  });
});

describe("buildStrykerMutateRanges and selectTsMutationTargets", () => {
  it("generates range patterns and whole-file targets for Infinity ranges", () => {
    const diffMap = new Map([
      ["src/changed.ts", [[10, 20]]],
      ["src/untracked.ts", [[1, Infinity]]],
    ]);
    const ranges = buildStrykerMutateRanges(["src/changed.ts", "src/untracked.ts"], diffMap);
    expect(ranges).toEqual(["src/changed.ts:10-20", "src/untracked.ts"]);
  });

  it("selects mutation targets for pilot mode and changed mode", () => {
    const pilot = ["src/pilot.ts"];
    const diffMap = new Map([["src/changed.ts", [[1, 5]]]]);
    const fileExists = () => true;

    const pilotTargets = selectTsMutationTargets({
      pilotFiles: pilot,
      diffMap,
      mode: "pilot",
      fileExists,
    });
    expect(pilotTargets.targetFiles).toEqual(["src/pilot.ts"]);

    const changedTargets = selectTsMutationTargets({
      pilotFiles: pilot,
      diffMap,
      mode: "changed",
      fileExists,
    });
    expect(changedTargets.targetFiles).toEqual(["src/pilot.ts", "src/changed.ts"]);
    expect(changedTargets.mutateList).toEqual(["src/changed.ts:1-5", "src/pilot.ts"]);
  });
});

describe("buildCargoMutantsCalls and selectRustMutationTargets", () => {
  it("builds invocation arguments for pilot and changed modes", () => {
    const pilotCalls = buildCargoMutantsCalls({
      mode: "pilot",
      rustPilot: ["crates/core/src/lib.rs"],
    });
    expect(pilotCalls).toEqual([
      {
        outDir: "mutants.out",
        args: ["mutants", "-o", "mutants.out", "-f", "crates/core/src/lib.rs"],
        needsDiff: false,
      },
    ]);

    const changedCalls = buildCargoMutantsCalls({
      mode: "changed",
      rustPilot: ["crates/core/src/lib.rs"],
    });
    expect(changedCalls.length).toBe(2);
    expect(changedCalls[0].needsDiff).toBe(true);
  });

  it("selects rust mutation targets", () => {
    const diffMap = new Map([["crates/core/src/foo.rs", [[1, 5]]]]);
    const targets = selectRustMutationTargets({
      pilotFiles: ["crates/core/src/lib.rs"],
      diffMap,
      mode: "changed",
      fileExists: () => true,
    });
    expect(targets.targetFiles).toEqual(["crates/core/src/lib.rs", "crates/core/src/foo.rs"]);
  });
});

describe("readQualityConfig and collectCandidateFiles", () => {
  it("reads config and supplies defaults when properties are omitted", () => {
    const config = readQualityConfig("C:/repo", () => JSON.stringify({}));
    expect(config.complexityMax).toBe(12);
    expect(config.crapMax).toBe(12);
    expect(config.mutationMin).toBe(80);
    expect(config.pilotFiles).toEqual([]);
  });

  it("collects candidate files for changed mode", () => {
    const diffMap = new Map([
      ["src/app.ts", [[1, 2]]],
      ["README.md", [[1, 2]]],
    ]);
    const candidates = collectCandidateFiles({
      pilotFiles: ["src/pilot.ts"],
      diffMap,
      mode: "changed",
      fileExists: () => true,
    });
    expect(candidates.has("src/pilot.ts")).toBe(true);
    expect(candidates.has("src/app.ts")).toBe(true);
    expect(candidates.has("README.md")).toBe(false);
  });
});

describe("evaluateRunFailure", () => {
  it("returns 1 if either gate fails and 0 if both pass", () => {
    expect(evaluateRunFailure(true, false)).toBe(1);
    expect(evaluateRunFailure(false, true)).toBe(1);
    expect(evaluateRunFailure(true, true)).toBe(1);
    expect(evaluateRunFailure(false, false)).toBe(0);
  });
});
