import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/svelte";
import { mockIPC } from "@tauri-apps/api/mocks";
import AppHeader from "./AppHeader.svelte";

describe("AppHeader", () => {
  it("shows the app name and version from the Rust core", async () => {
    mockIPC((cmd) => {
      if (cmd === "app_info") return { name: "SimCurveTrainApp", version: "0.1.0" };
      throw new Error(`unexpected command ${cmd}`);
    });

    render(AppHeader);

    expect(await screen.findByText("v0.1.0")).toBeInTheDocument();
    expect(screen.getByRole("heading", { level: 1 })).toHaveTextContent("SimCurveTrainApp");
  });

  it("shows a fallback when the version cannot be loaded", async () => {
    mockIPC(() => {
      throw new Error("ipc unavailable");
    });

    render(AppHeader);

    expect(await screen.findByText("version unavailable")).toBeInTheDocument();
    expect(screen.getByRole("heading", { level: 1 })).toHaveTextContent("SimCurveTrainApp");
  });
});
