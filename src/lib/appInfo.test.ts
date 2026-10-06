import { describe, expect, it } from "vitest";
import { mockIPC } from "@tauri-apps/api/mocks";
import { formatVersion, getAppInfo } from "./appInfo";

describe("formatVersion", () => {
  it("prefixes a bare semver with v", () => {
    expect(formatVersion("0.1.0")).toBe("v0.1.0");
  });

  it("keeps an existing v prefix", () => {
    expect(formatVersion("v1.2.3")).toBe("v1.2.3");
  });

  it("trims whitespace and handles empty input", () => {
    expect(formatVersion(" 2.0.0 ")).toBe("v2.0.0");
    expect(formatVersion("   ")).toBe("");
  });
});

describe("getAppInfo", () => {
  it("invokes the app_info command", async () => {
    const calls: string[] = [];
    mockIPC((cmd) => {
      calls.push(cmd);
      return { name: "SimCurveTrainApp", version: "0.1.0" };
    });

    await expect(getAppInfo()).resolves.toEqual({ name: "SimCurveTrainApp", version: "0.1.0" });
    expect(calls).toEqual(["app_info"]);
  });
});
