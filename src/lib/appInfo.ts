import { invoke } from "@tauri-apps/api/core";

/** Mirrors `sct_core::AppInfo` on the Rust side. */
export interface AppInfo {
  name: string;
  version: string;
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
