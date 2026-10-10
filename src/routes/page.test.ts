import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/svelte";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { goto } from "$app/navigation";
import Page from "./+page.svelte";
import type { Preset } from "$lib/drill";

vi.mock("$app/navigation", () => ({
  goto: vi.fn(),
}));

const starterPreset: Preset = {
  schemaVersion: 1,
  id: "starter",
  name: "Starter Drills",
  description: "Introductory drills",
  drills: [
    {
      id: "clutch-hold",
      name: "Clutch hold 50%",
      type: "hold",
      pedal: "clutch",
      reps: 3,
      leadInMs: 1000,
      target: 50,
      holdMs: 2000,
    },
    {
      id: "brake-hold-70",
      name: "Brake hold 70%",
      type: "hold",
      pedal: "brake",
      reps: 3,
      leadInMs: 1000,
      target: 70,
      holdMs: 2000,
    },
  ],
};

const advancedPreset: Preset = {
  schemaVersion: 1,
  id: "advanced",
  name: "Advanced Drills",
  description: "Trace drills",
  drills: [
    {
      id: "throttle-trace",
      name: "Throttle trace 1.5s",
      type: "trace",
      pedal: "throttle",
      reps: 3,
      leadInMs: 1000,
      points: [
        [0, 0],
        [1500, 100],
      ],
    },
  ],
};

describe("Home page preset picker", () => {
  beforeEach(() => {
    localStorage.clear();
    (globalThis as { isTauri?: boolean }).isTauri = true;
    vi.clearAllMocks();

    mockIPC((cmd, args) => {
      switch (cmd) {
        case "list_presets":
          return [starterPreset, advancedPreset];
        case "best_totals": {
          const { presetId } = (args ?? {}) as { presetId?: string };
          if (presetId === "starter") {
            return { "brake-hold-70": 87.6 };
          }
          return {};
        }
        case "app_info":
          return { name: "SimCurveTrainApp", version: "0.1.0" };
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });
  });

  afterEach(() => {
    cleanup();
    clearMocks();
    vi.restoreAllMocks();
    delete (globalThis as { isTauri?: boolean }).isTauri;
  });

  it("renders both cards and initially opens the first card when nothing is stored", async () => {
    render(Page);

    const starterBtn = await screen.findByRole("button", { name: /Starter Drills/ });
    const advancedBtn = screen.getByRole("button", { name: /Advanced Drills/ });

    expect(starterBtn).toHaveAttribute("aria-expanded", "true");
    expect(advancedBtn).toHaveAttribute("aria-expanded", "false");
  });

  it("opens the stored last preset when localStorage has it", async () => {
    localStorage.setItem("sct:last_preset", "advanced");
    render(Page);

    const starterBtn = await screen.findByRole("button", { name: /Starter Drills/ });
    const advancedBtn = screen.getByRole("button", { name: /Advanced Drills/ });

    expect(starterBtn).toHaveAttribute("aria-expanded", "false");
    expect(advancedBtn).toHaveAttribute("aria-expanded", "true");
  });

  it("hides clutch drills and displays Best 88 for scored drills", async () => {
    render(Page);

    expect(await screen.findByText("Brake hold 70%")).toBeInTheDocument();
    expect(screen.queryByText("Clutch hold 50%")).not.toBeInTheDocument();
    expect(await screen.findByText("Best 88")).toBeInTheDocument();
  });

  it("displays Best — when a drill has no recorded score", async () => {
    localStorage.setItem("sct:last_preset", "advanced");
    render(Page);

    expect(await screen.findByText("Throttle trace 1.5s")).toBeInTheDocument();
    expect(await screen.findByText("Best —")).toBeInTheDocument();
  });

  it("stores preset id under sct:last_preset when clicking another card", async () => {
    render(Page);

    const advancedBtn = await screen.findByRole("button", { name: /Advanced Drills/ });
    expect(localStorage.getItem("sct:last_preset")).toBeNull();

    await fireEvent.click(advancedBtn);
    expect(advancedBtn).toHaveAttribute("aria-expanded", "true");
    expect(localStorage.getItem("sct:last_preset")).toBe("advanced");
  });

  it("clicking a drill calls goto with a URL ending in /drill?preset=<id>&drill=<id>", async () => {
    render(Page);

    const drillBtn = await screen.findByRole("button", { name: /Brake hold 70%/ });
    await fireEvent.click(drillBtn);

    expect(localStorage.getItem("sct:last_preset")).toBe("starter");
    expect(goto).toHaveBeenCalledWith(
      expect.stringMatching(/\/drill\?preset=starter&drill=brake-hold-70$/),
    );
  });
});
