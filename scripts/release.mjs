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

/** Older gh versions don't print the run URL: take the newest dispatch run since `since`. */
async function findRunId(since) {
  for (let attempt = 0; attempt < 20; attempt++) {
    const runs = JSON.parse(
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
        "databaseId,createdAt",
      ),
    );
    if (runs[0] && Date.parse(runs[0].createdAt) >= since - 10_000) {
      return String(runs[0].databaseId);
    }
    await sleep(3000);
  }
  fail("No Release run appeared within 60 s. Check the Release workflow in the Actions tab.");
}

/** The release this run published: the newest one published after `since`. */
function publishedRelease(since) {
  const releases = JSON.parse(
    gh("release", "list", "--limit", "5", "--exclude-drafts", "--json", "tagName,publishedAt"),
  );
  const release = releases.find((r) => Date.parse(r.publishedAt) >= since);
  if (!release) return undefined;
  // `gh release list` has no url field.
  return {
    tag: release.tagName,
    ...JSON.parse(gh("release", "view", release.tagName, "--json", "url")),
  };
}

async function main() {
  const options = parseArgs(process.argv.slice(2));
  if (spawnSync("gh", ["auth", "status"], { stdio: "ignore" }).status !== 0) {
    fail("The GitHub CLI isn't logged in. Run 'gh auth login' first.");
  }
  const since = Date.now();
  const runId = startWorkflow(options) ?? (await findRunId(since));
  const runUrl = gh("run", "view", runId, "--json", "url", "--jq", ".url").trim();
  console.log(`Release run: ${runUrl}`);

  const watch = spawnSync("gh", ["run", "watch", runId, "--exit-status"], { stdio: "inherit" });
  if (watch.status !== 0) fail(`Release failed: ${runUrl}`);

  const release = publishedRelease(since);
  if (!release) fail(`The run passed but no new release was found. Check ${runUrl}`);
  console.log(`Published ${release.tag}: ${release.url}`);
}

await main();
