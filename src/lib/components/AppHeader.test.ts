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
    expect(screen.queryByTestId("build-info")).not.toBeInTheDocument();
    expect(screen.queryByTestId("test-data-badge")).not.toBeInTheDocument();
    expect(screen.queryByTestId("sim-pedals-badge")).not.toBeInTheDocument();
  });

  it("shows build info, test data marker, and sim pedals marker when present", async () => {
    mockIPC((cmd) => {
      if (cmd === "app_info") {
        return {
          name: "SimCurveTrainApp",
          version: "0.1.0",
          build: {
            branch: "ui-polish",
            commit: "abc1234",
            worktree: "sct-wt-ui-polish",
          },
          testData: true,
          simPedals: true,
        };
      }
      throw new Error(`unexpected command ${cmd}`);
    });

    render(AppHeader);

    const buildInfo = await screen.findByTestId("build-info");
    expect(buildInfo).toHaveTextContent("ui-polish @ abc1234");
    expect(buildInfo).toHaveAttribute("title", "Worktree: sct-wt-ui-polish");

    const testData = screen.getByTestId("test-data-badge");
    expect(testData).toHaveTextContent("TEST DATA");

    const simPedals = screen.getByTestId("sim-pedals-badge");
    expect(simPedals).toHaveTextContent("SIM PEDALS");
  });

  it("shows a fallback when the version cannot be loaded", async () => {
    mockIPC(() => {
      throw new Error("ipc unavailable");
    });

    render(AppHeader);

    expect(await screen.findByText("version unavailable")).toBeInTheDocument();
    expect(screen.getByRole("heading", { level: 1 })).toHaveTextContent("SimCurveTrainApp");
  });

  it("renders theme toggle button and toggles between dark and light", async () => {
    localStorage.clear();
    mockIPC((cmd) => {
      if (cmd === "app_info") return { name: "SimCurveTrainApp", version: "0.1.0" };
      throw new Error(`unexpected command ${cmd}`);
    });

    render(AppHeader);

    const toggleBtn = await screen.findByTestId("theme-toggle");
    expect(toggleBtn).toBeInTheDocument();
    expect(toggleBtn).toHaveAttribute("aria-label", "Switch to light theme");

    // Click toggle to switch to light
    const { fireEvent } = await import("@testing-library/svelte");
    await fireEvent.click(toggleBtn);

    expect(toggleBtn).toHaveAttribute("aria-label", "Switch to dark theme");
    expect(localStorage.getItem("sct:theme")).toBe("light");
    expect(document.documentElement.getAttribute("data-theme")).toBe("light");

    // Click toggle again to switch back to dark
    await fireEvent.click(toggleBtn);

    expect(toggleBtn).toHaveAttribute("aria-label", "Switch to light theme");
    expect(localStorage.getItem("sct:theme")).toBe("dark");
    expect(document.documentElement.getAttribute("data-theme")).toBe("dark");
  });
});
