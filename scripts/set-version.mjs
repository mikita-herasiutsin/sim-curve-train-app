// Sets the workspace version across Cargo.toml, package.json, package-lock.json,
// and Cargo.lock.
//
// Usage:
//   node scripts/set-version.mjs <version>
//
// Examples:
//   node scripts/set-version.mjs 0.1.0
//   node scripts/set-version.mjs 0.1.0-rc.1
import { execFileSync, execSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";

const ROOT = resolve(import.meta.dirname, "..");
const SEMVER_RE = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(-[0-9A-Za-z.-]+)?$/;

function validateVersion(arg) {
  if (!arg) {
    console.error("Usage: node scripts/set-version.mjs <version>");
    process.exit(1);
  }
  if (arg.startsWith("v")) {
    console.error("Error: Leading 'v' is not allowed. Drop the leading 'v'.");
    console.error("Usage: node scripts/set-version.mjs <version>");
    process.exit(1);
  }
  if (!SEMVER_RE.test(arg)) {
    console.error(`Error: Invalid semver version '${arg}'.`);
    console.error("Usage: node scripts/set-version.mjs <version>");
    process.exit(1);
  }
  return arg;
}

function updateCargoToml(version) {
  const cargoPath = resolve(ROOT, "Cargo.toml");
  const content = readFileSync(cargoPath, "utf8");

  const headerMatch = /(?:^|\r?\n)\[workspace\.package\](?:\r?\n|$)/.exec(content);
  if (!headerMatch) {
    console.error("Error: [workspace.package] table not found in Cargo.toml");
    process.exit(1);
  }

  const bodyStart = headerMatch.index + headerMatch[0].length;
  const nextHeaderMatch = /\r?\n\[/.exec(content.slice(bodyStart));
  const bodyEnd = nextHeaderMatch ? bodyStart + nextHeaderMatch.index : content.length;

  const body = content.slice(bodyStart, bodyEnd);
  const versionLineRe = /^(\s*version\s*=\s*")[^"]*(".*)$/m;
  if (!versionLineRe.test(body)) {
    console.error("Error: version line not found in [workspace.package] in Cargo.toml");
    process.exit(1);
  }

  const updatedBody = body.replace(versionLineRe, `$1${version}$2`);
  const updatedContent = content.slice(0, bodyStart) + updatedBody + content.slice(bodyEnd);
  writeFileSync(cargoPath, updatedContent, "utf8");
}

// npm is npm.cmd on Windows, which needs a shell; `version` is already validated as semver.
function updateNpmVersion(version) {
  execSync(`npm version ${version} --no-git-tag-version --allow-same-version`, {
    cwd: ROOT,
    stdio: "inherit",
  });
}

function updateCargoLock() {
  execFileSync("cargo", ["metadata", "--format-version", "1"], {
    cwd: ROOT,
    stdio: ["ignore", "ignore", "inherit"],
  });
}

function main() {
  process.chdir(ROOT);
  const version = validateVersion(process.argv[2]);
  updateCargoToml(version);
  updateNpmVersion(version);
  updateCargoLock();
  console.log(`Version set to ${version}`);
}

main();
