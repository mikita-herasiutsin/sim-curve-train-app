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

  it("evaluates killed when tests or test suites fail", () => {
    expect(
      evaluateVitestResults({
        numTotalTests: 5,
        numFailedTests: 1,
        numFailedTestSuites: 0,
      }),
    ).toBe("killed");
    expect(
      evaluateVitestResults({
        numTotalTests: 5,
        numFailedTests: 0,
        numFailedTestSuites: 1,
      }),
    ).toBe("killed");
  });

  it("evaluates survived when all tests pass", () => {
    expect(
      evaluateVitestResults({
        numTotalTests: 5,
        numFailedTests: 0,
        numFailedTestSuites: 0,
      }),
    ).toBe("survived");
  });

  it("evaluates no-results when results are null or total tests is zero", () => {
    expect(evaluateVitestResults(null)).toBe("no-results");
    expect(evaluateVitestResults({ numTotalTests: 0 })).toBe("no-results");
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
