// Starts the Release workflow on main via the GitHub CLI, follows it and prints the release URL.
// See docs/releasing.md.
//
// Usage:
//   npm run release                   next patch, as a pre-release
//   npm run release -- 0.1.0          that version, as a pre-release
//   npm run release -- 0.1.0 --full   that version, as a full release (marked Latest)
import { execFileSync, spawnSync } from "node:child_process";

const SEMVER_RE = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(-[0-9A-Za-z.-]+)?$/;
const USAGE = "Usage: npm run release [-- [<version>] [--full]]";

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const gh = (...args) => execFileSync("gh", args, { encoding: "utf8" });

function fail(message) {
  console.error(message);
  process.exit(1);
}

function parseArgs(args) {
  const full = args.includes("--full");
  const rest = args.filter((arg) => arg !== "--full");
  if (rest.length > 1) fail(USAGE);
  if (!rest[0]) return { full };
  const version = rest[0].replace(/^v/, "");
  if (!SEMVER_RE.test(version)) fail(`Invalid version '${rest[0]}'.\n${USAGE}`);
  return { version, full };
}

/** Starts the workflow and returns its run id, or undefined when gh doesn't print the run URL. */
function startWorkflow({ version, full }) {
  const args = ["workflow", "run", "release.yml", "--ref", "main"];
  if (version) args.push("-f", `version=${version}`);
  if (full) args.push("-f", "prerelease=false");
  const res = spawnSync("gh", args, { encoding: "utf8" });
  const output = `${res.stdout ?? ""}${res.stderr ?? ""}`;
  process.stdout.write(output);
  if (res.status !== 0) fail("Couldn't start the Release workflow.");
  return /\/actions\/runs\/(\d+)/.exec(output)?.[1];
}

const latestRunId = () =>
  JSON.parse(
    gh(
      "run",
      "list",
      "--workflow",
      "release.yml",
      "--event",
      "workflow_dispatch",
      "--limit",
      "1",
      "--json",
      "databaseId",
    ),
  )[0]?.databaseId ?? 0;

const publishedTags = () =>
  JSON.parse(gh("release", "list", "--limit", "20", "--exclude-drafts", "--json", "tagName")).map(
    (r) => r.tagName,
  );

/** Older gh versions don't print the run URL, so wait for a run newer than `previousId`. */
async function findRunId(previousId) {
  for (let attempt = 0; attempt < 20; attempt++) {
    const id = latestRunId();
    if (id > previousId) return String(id);
    await sleep(3000);
  }
  fail("No Release run appeared within 60 s. Check the Release workflow in the Actions tab.");
}

/** Returns the release this run published, which is the one missing from `before`. */
async function newRelease(before) {
  for (let attempt = 0; attempt < 5; attempt++) {
    const tag = publishedTags().find((t) => !before.includes(t));
    if (tag) return { tag, ...JSON.parse(gh("release", "view", tag, "--json", "url")) };
    await sleep(2000);
  }
  return undefined;
}

async function main() {
  const options = parseArgs(process.argv.slice(2));
  if (spawnSync("gh", ["auth", "status"], { stdio: "ignore" }).status !== 0) {
    fail("The GitHub CLI isn't logged in. Run 'gh auth login' first.");
  }
  const previousRunId = latestRunId();
  const before = publishedTags();
  const runId = startWorkflow(options) ?? (await findRunId(previousRunId));
  const runUrl = gh("run", "view", runId, "--json", "url", "--jq", ".url").trim();
  console.log(`Release run: ${runUrl}`);

  const watch = spawnSync("gh", ["run", "watch", runId, "--exit-status"], { stdio: "inherit" });
  if (watch.status !== 0) fail(`Release failed: ${runUrl}`);

  const release = await newRelease(before);
  if (!release) fail(`The run passed but no new release was found. Check ${runUrl}`);
  console.log(`Published ${release.tag}: ${release.url}`);
}

await main();
