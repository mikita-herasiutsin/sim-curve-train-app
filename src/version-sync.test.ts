// @vitest-environment node
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

// The app version lives in the Cargo workspace (Tauri reads it from there); package.json must
// stay in sync so npm tooling and the app agree. Main holds the 0.0.0-dev placeholder; the
// Release workflow sets the real version with scripts/set-version.mjs.
const read = (path: string) => readFileSync(resolve(import.meta.dirname, "..", path), "utf8");

/** Returns the body of a TOML table, up to the next `[header]` line. */
function tomlTable(toml: string, header: string): string {
  const start = toml.indexOf(`\n[${header}]\n`);
  if (start === -1) return "";
  const body = toml.slice(start + header.length + 4);
  const next = body.search(/^\[/m);
  return next === -1 ? body : body.slice(0, next);
}

const majorMinor = (version: string | undefined) => version?.split(".").slice(0, 2).join(".");

describe("version sync", () => {
  it("package.json version matches the Cargo workspace version", () => {
    const workspace = tomlTable(read("Cargo.toml"), "workspace.package");
    const cargoVersion = /^version\s*=\s*"([^"]+)"/m.exec(workspace)?.[1];
    const npmVersion = JSON.parse(read("package.json")).version;

    expect(cargoVersion).toBeDefined();
    expect(npmVersion).toBe(cargoVersion);
  });

  // Release builds get their version from the tag (scripts/set-version.mjs), never from a commit.
  it("main holds the 0.0.0-dev placeholder version", () => {
    expect(JSON.parse(read("package.json")).version).toBe("0.0.0-dev");
  });

  it("tauri.conf.json does not override the Cargo version", () => {
    expect(JSON.parse(read("src-tauri/tauri.conf.json"))).not.toHaveProperty("version");
  });

  // The Tauri CLI refuses to build when these differ, but only the slow release build runs it.
  it("locked tauri crate and @tauri-apps/api share major.minor", () => {
    const crateVersion = /^name = "tauri"\nversion = "([^"]+)"/m.exec(read("Cargo.lock"))?.[1];
    const npmVersion = JSON.parse(read("package-lock.json")).packages[
      "node_modules/@tauri-apps/api"
    ]?.version;

    expect(crateVersion).toBeDefined();
    expect(majorMinor(npmVersion)).toBe(majorMinor(crateVersion));
  });
});
