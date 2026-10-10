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
  warmUp: {
    steps: [{ drill: "brake-hold-70", reps: 2 }],
  },
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

  it("shows Warm-up button only for a preset with warm-up and navigates to /drill?preset=<id>&warmup=1", async () => {
    render(Page);

    const warmUpBtn = await screen.findByTestId("warm-up-start");
    expect(warmUpBtn).toHaveTextContent("Warm-up");
    await fireEvent.click(warmUpBtn);

    expect(localStorage.getItem("sct:last_preset")).toBe("starter");
    expect(goto).toHaveBeenCalledWith(expect.stringMatching(/\/drill\?preset=starter&warmup=1$/));

    const advancedBtn = screen.getByRole("button", { name: /Advanced Drills/ });
    await fireEvent.click(advancedBtn);
    expect(screen.queryByTestId("warm-up-start")).not.toBeInTheDocument();
  });

  interface HomeMock {
    presets?: Preset[];
    listError?: string;
    scores?: (presetId: string) => Record<string, number>;
  }

  /** Replaces the IPC mock. Returns the preset ids that best_totals was called with. */
  function mockHome({
    presets = [starterPreset, advancedPreset],
    listError,
    scores = () => ({}),
  }: HomeMock = {}) {
    const requested: string[] = [];
    mockIPC((cmd, args) => {
      switch (cmd) {
        case "list_presets":
          if (listError) throw listError;
          return presets;
        case "best_totals": {
          const { presetId } = (args ?? {}) as { presetId: string };
          requested.push(presetId);
          return scores(presetId);
        }
        case "app_info":
          return { name: "SimCurveTrainApp", version: "0.1.0" };
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });
    return { requested };
  }

  it("shows an alert when the preset list fails to load", async () => {
    mockHome({ listError: "presets file is corrupt" });
    render(Page);
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Failed to load presets");
    expect(alert).toHaveTextContent("presets file is corrupt");
  });

  it("shows a loading message until the presets arrive", async () => {
    render(Page);
    expect(screen.getByText("Loading presets…")).toBeInTheDocument();
    expect(await screen.findByRole("button", { name: /Starter Drills/ })).toBeInTheDocument();
    expect(screen.queryByText("Loading presets…")).not.toBeInTheDocument();
  });

  it("says no playable drills are found when every drill is hidden", async () => {
    const clutchOnly: Preset = {
      ...starterPreset,
      id: "clutch-only",
      name: "Clutch Only",
      drills: [{ ...starterPreset.drills[0] }],
    };
    mockHome({ presets: [clutchOnly] });
    render(Page);
    expect(await screen.findByText("No playable drills found.")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Clutch Only/ })).not.toBeInTheDocument();
  });

  it("loads the scores of the card that is opened", async () => {
    const { requested } = mockHome({
      scores: (presetId): Record<string, number> =>
        presetId === "advanced" ? { "throttle-trace": 55.2 } : {},
    });
    render(Page);

    const advancedBtn = await screen.findByRole("button", { name: /Advanced Drills/ });
    await fireEvent.click(advancedBtn);

    expect(advancedBtn).toHaveAttribute("aria-expanded", "true");
    expect(await screen.findByText("Best 55")).toBeInTheDocument();
    expect(screen.queryByText("Brake hold 70%")).not.toBeInTheDocument();
    expect(requested).toContain("advanced");
  });

  it("collapses the open card when it is clicked again", async () => {
    render(Page);

    const starterBtn = await screen.findByRole("button", { name: /Starter Drills/ });
    expect(starterBtn).toHaveAttribute("aria-expanded", "true");
    expect(await screen.findByText("Brake hold 70%")).toBeInTheDocument();

    await fireEvent.click(starterBtn);

    expect(starterBtn).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByText("Brake hold 70%")).not.toBeInTheDocument();
  });

  it("uses the singular for a preset with one drill", async () => {
    render(Page);
    const advancedBtn = await screen.findByRole("button", { name: /Advanced Drills/ });
    expect(advancedBtn).toHaveTextContent(/1 drill$/);
    expect(advancedBtn).not.toHaveTextContent("1 drills");
  });

  it("shows Best — for a drill whose id is a built-in object property", async () => {
    const ctorPreset: Preset = {
      ...starterPreset,
      id: "ctor",
      name: "Constructor Drills",
      drills: [{ ...starterPreset.drills[1], id: "constructor", name: "Constructor hold" }],
    };
    mockHome({ presets: [ctorPreset] });
    render(Page);
    expect(await screen.findByText("Constructor hold")).toBeInTheDocument();
    expect(await screen.findByText("Best —")).toBeInTheDocument();
    expect(screen.queryByText(/NaN/)).not.toBeInTheDocument();
  });
});
