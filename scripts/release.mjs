// Triggers the Release workflow on main via the GitHub CLI and waits for it.
//
// Usage:
//   npm run release
//   npm run release -- <version>
//
// Examples:
//   npm run release
//   npm run release -- 0.1.0
//   npm run release -- v0.1.0
import { execFileSync, spawnSync } from "node:child_process";

const SEMVER_RE = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(-[0-9A-Za-z.-]+)?$/;

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

function parseVersionArg(arg) {
  if (!arg) return undefined;
  const version = arg.startsWith("v") ? arg.slice(1) : arg;
  if (!SEMVER_RE.test(version)) {
    console.error(`Invalid semver version: '${arg}'.`);
    console.error("Usage: npm run release [-- <version>]");
    process.exit(1);
  }
  return version;
}

function checkAuth() {
  const res = spawnSync("gh", ["auth", "status"], { stdio: "ignore" });
  if (res.status !== 0) {
    console.error("GitHub CLI is not authenticated. Please run 'gh auth login'.");
    process.exit(1);
  }
}

function triggerWorkflow(version) {
  const recordedTime = Date.now();
  const args = ["workflow", "run", "release.yml", "--ref", "main"];
  if (version) {
    args.push("-f", `version=${version}`);
  }
  const res = spawnSync("gh", args, { stdio: "inherit" });
  if (res.status !== 0) {
    console.error("Failed to trigger release workflow.");
    process.exit(1);
  }
  return recordedTime;
}

const gh = (...args) => execFileSync("gh", args, { encoding: "utf8" });

async function findRun(recordedTime) {
  const threshold = recordedTime - 10_000;
  const deadline = Date.now() + 60_000;

  while (Date.now() < deadline) {
    const res = spawnSync(
      "gh",
      [
        "run",
        "list",
        "--workflow",
        "release.yml",
        "--event",
        "workflow_dispatch",
        "--limit",
        "5",
        "--json",
        "databaseId,url,createdAt",
      ],
      { encoding: "utf8" },
    );

    if (res.status === 0 && res.stdout) {
      try {
        const runs = JSON.parse(res.stdout);
        const matching = runs.filter((r) => new Date(r.createdAt).getTime() >= threshold);
        if (matching.length > 0) {
          matching.sort(
            (a, b) => new Date(b.createdAt).getTime() - new Date(a.createdAt).getTime(),
          );
          const run = matching[0];
          console.log(`Release run: ${run.url}`);
          return run;
        }
      } catch {
        // Retry on parse error
      }
    }

    await sleep(3000);
  }

  console.error(`No release run found within 60 s. Check the Release workflow in the Actions tab.`);
  process.exit(1);
}

function watchRun(run) {
  const res = spawnSync("gh", ["run", "watch", String(run.databaseId), "--exit-status"], {
    stdio: "inherit",
  });
  if (res.status !== 0) {
    console.error(`Release failed: ${run.url}`);
    process.exit(1);
  }
}

/** The newest published release; `gh release list` has no url field, so view it by tag. */
function getPublishedRelease() {
  const [latest] = JSON.parse(
    gh("release", "list", "--limit", "1", "--exclude-drafts", "--json", "tagName"),
  );
  if (!latest) {
    console.error("No published release found.");
    process.exit(1);
  }
  const { url } = JSON.parse(gh("release", "view", latest.tagName, "--json", "url"));
  return { tagName: latest.tagName, url };
}

async function main() {
  const version = parseVersionArg(process.argv[2]);
  checkAuth();
  const recordedTime = triggerWorkflow(version);
  const run = await findRun(recordedTime);
  watchRun(run);
  const { tagName, url } = getPublishedRelease();
  console.log(`Published ${tagName}: ${url}`);
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
