import { invoke } from "@tauri-apps/api/core";

/** Git state of a dev build, so several worktree builds can be told apart. */
export interface BuildInfo {
  branch: string;
  commit: string;
  worktree: string;
}

/** Mirrors `sct_core::AppInfo` on the Rust side. */
export interface AppInfo {
  name: string;
  version: string;
  build?: BuildInfo;
  testData?: boolean;
  simPedals?: boolean;
}

/** Fetches app name and version from the Rust core. */
export function getAppInfo(): Promise<AppInfo> {
  return invoke<AppInfo>("app_info");
}

/** Formats a semver string for display, e.g. `0.1.0` -> `v0.1.0`. */
export function formatVersion(version: string): string {
  const trimmed = version.trim();
  if (trimmed === "") return "";
  return trimmed.startsWith("v") ? trimmed : `v${trimmed}`;
}
