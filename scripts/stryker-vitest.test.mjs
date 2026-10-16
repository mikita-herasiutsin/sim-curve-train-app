import { describe, expect, it } from "vitest";
import {
  evaluateVitestResults,
  getOutputFile,
  handleNoResults,
  readVitestResults,
  runStrykerVitest,
} from "./stryker-vitest.mjs";

describe("stryker-vitest", () => {
  it("determines output file path using worker ID", () => {
    expect(getOutputFile("3")).toBe(".stryker-tmp/vitest-3.json");
    expect(getOutputFile("0")).toBe(".stryker-tmp/vitest-0.json");
  });

  it("reads vitest results or returns null when missing", () => {
    expect(readVitestResults("non-existent.json")).toBeNull();
  });

  it("evaluates survived when vitest exits 0", () => {
    expect(evaluateVitestResults({ numTotalTests: 5, numFailedTests: 0 }, 0)).toBe("survived");
  });

  it("evaluates killed when vitest fails after collecting tests", () => {
    expect(evaluateVitestResults({ numTotalTests: 5, numFailedTests: 1 }, 1)).toBe("killed");
    // A worker that runs out of memory leaves numFailedTests at 0 and success true.
    expect(
      evaluateVitestResults({ numTotalTests: 86, numPassedTests: 67, numFailedTests: 0 }, 1),
    ).toBe("killed");
  });

  it("evaluates killed when every suite fails to load", () => {
    expect(evaluateVitestResults({ numTotalTests: 0, numFailedTestSuites: 2 }, 1)).toBe("killed");
  });

  it("evaluates no-results when vitest fails without a report or tests", () => {
    expect(evaluateVitestResults(null, 1)).toBe("no-results");
    expect(evaluateVitestResults({ numTotalTests: 0, numFailedTestSuites: 0 }, 1)).toBe(
      "no-results",
    );
  });

  it("prints notice and stderr on no-results", () => {
    const output = [];
    handleNoResults("fatal error\n", (msg) => output.push(msg));
    expect(output).toEqual(["stryker-vitest: vitest produced no test results\n", "fatal error\n"]);
  });

  it("returns exit code 1 when killed and 0 on survival or crash", () => {
    const failRunner = () => {
      const err = new Error("vitest failed");
      err.stderr = "crash details";
      throw err;
    };
    const logged = [];
    const crashCode = runStrykerVitest(["foo.ts"], {
      runner: failRunner,
      outputFile: "non-existent-results.json",
      writer: (msg) => logged.push(msg),
    });
    expect(crashCode).toBe(0);
    expect(logged).toContain("stryker-vitest: vitest produced no test results\n");
    expect(logged).toContain("crash details");
  });
});
