import process from "node:process";

// Mutation testing for the TS quality gate; `scripts/quality.mjs` passes `--mutate` and sets
// QUALITY_RELATED to the mutated files. The Stryker Vitest runner runs no tests after its first
// pass with Vitest 5, so each mutant runs `vitest related` as a command instead.
const related = process.env.QUALITY_RELATED ?? "";

/** @type {import('@stryker-mutator/api/core').PartialStrykerOptions} */
export default {
  testRunner: "command",
  commandRunner: { command: `npx vitest related --run ${related}` },
  coverageAnalysis: "off",
  // A sandbox copy can't load node_modules through Vite on Windows, so mutate in place.
  // Stryker restores the files when it ends; don't run it while a dev server watches this tree.
  inPlace: true,
  concurrency: 4,
  timeoutMS: 30000,
  reporters: ["clear-text", "json"],
  jsonReporter: { fileName: "reports/mutation/mutation.json" },
  thresholds: { high: 90, low: 80, break: null },
  tempDirName: ".stryker-tmp",
};
