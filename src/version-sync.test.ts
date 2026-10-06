// @vitest-environment node
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

// The release version lives in the Cargo workspace (Tauri reads it from there);
// package.json must stay in sync so npm tooling and the app agree.
const read = (path: string) => readFileSync(resolve(import.meta.dirname, "..", path), "utf8");

describe("version sync", () => {
  it("package.json version matches the Cargo workspace version", () => {
    const cargoVersion = /\[workspace\.package\][^[]*?^version\s*=\s*"([^"]+)"/m.exec(
      read("Cargo.toml"),
    )?.[1];
    const npmVersion = JSON.parse(read("package.json")).version;

    expect(cargoVersion).toBeDefined();
    expect(npmVersion).toBe(cargoVersion);
  });

  it("tauri.conf.json does not override the Cargo version", () => {
    expect(JSON.parse(read("src-tauri/tauri.conf.json"))).not.toHaveProperty("version");
  });
});
