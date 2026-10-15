import { describe, expect, it } from "vitest";
import {
  cargoMutantsCounts,
  crap,
  istanbulCoverage,
  lineCoverage,
  matchSpan,
  mutationScore,
  overlaps,
  parseDiff,
  parseLcov,
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
