import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/svelte";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import DrillPage from "./+page.svelte";
import type { Preset } from "$lib/drill";
import type { NewAttempt } from "$lib/attempts";
import { pedalStream } from "$lib/pedals/stream";

let mockUrl = new URL("http://localhost/drill");

vi.mock("$app/state", () => ({
  page: {
    get url() {
      return mockUrl;
    },
  },
}));

const preset: Preset = {
  schemaVersion: 1,
  id: "starter",
  name: "Starter Drills",
  description: "",
  drills: [
    {
      id: "brake-hold-70",
      name: "Brake hold 70%",
      type: "hold",
      pedal: "brake",
      reps: 3,
      leadInMs: 1000,
      tolerance: 5,
      target: 70,
      holdMs: 2000,
    },
  ],
};

const tracePreset: Preset = {
  schemaVersion: 1,
  id: "trace-only",
  name: "Trace Drills",
  description: "",
  drills: [
    {
      id: "brake-trace",
      name: "Brake trace 1.5s",
      type: "trace",
      pedal: "brake",
      reps: 3,
      leadInMs: 1000,
      tolerance: 6,
      points: [
        [0, 0],
        [150, 100],
        [1500, 0],
      ],
    },
  ],
};

const device = {
  id: 1,
  name: "Test Pedals",
  guid: "x360:0001:0001:0001",
  vendorId: null,
  productId: null,
  axisCount: 3,
  buttonCount: 0,
  hatCount: 0,
  simulated: false,
};

interface Channelish {
  onmessage: (e: unknown) => void;
}

describe("Drill page", () => {
  let startError: string | null = null;
  let drillChannel: Channelish | null = null;
  let aborts = 0;
  let presetsList: Preset[] = [preset];
  let saved: NewAttempt[] = [];
  let saveError: string | null = null;

  beforeEach(() => {
    localStorage.clear();
    mockUrl = new URL("http://localhost/drill");
    startError = null;
    drillChannel = null;
    aborts = 0;
    presetsList = [preset];
    saved = [];
    saveError = null;
    (globalThis as { isTauri?: boolean }).isTauri = true;
    globalThis.ResizeObserver = class {
      observe() {}
      unobserve() {}
      disconnect() {}
    } as unknown as typeof ResizeObserver;
    vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(null);

    mockIPC(
      (cmd, args) => {
        switch (cmd) {
          case "list_presets":
            return presetsList;
          case "profiled_devices":
            return [device.id];
          case "list_devices":
            return { devices: [device], error: null };
          case "start_stream":
            return 7;
          case "stop_stream":
            return null;
          case "abort_drill_run":
            aborts++;
            return null;
          case "save_attempt":
            if (saveError) throw saveError;
            saved.push((args as { attempt: NewAttempt }).attempt);
            return saved.length;
          case "start_drill_run":
            drillChannel = (args as { onEvent: Channelish }).onEvent;
            if (startError) throw startError;
            return null;
          default:
            throw new Error(`unexpected command ${cmd}`);
        }
      },
      { shouldMockEvents: true },
    );
  });

  afterEach(() => {
    cleanup();
    clearMocks();
    vi.restoreAllMocks();
    delete (globalThis as { isTauri?: boolean }).isTauri;
  });

  async function startDrill() {
    const start = await screen.findByRole("button", { name: "Start Drill" });
    await waitFor(() => expect(start).toBeEnabled());
    await fireEvent.click(start);
  }

  it("lists the bundled presets in the picker", async () => {
    render(DrillPage);
    expect(await screen.findByRole("option", { name: "Starter Drills" })).toBeInTheDocument();
    expect(await screen.findByRole("option", { name: /Brake hold 70%/ })).toBeInTheDocument();
  });

  it("shows a start error inline", async () => {
    startError = "the device has no saved profile; calibrate it first";
    render(DrillPage);
    await startDrill();
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Failed to start drill");
    expect(alert).toHaveTextContent("calibrate it first");
  });

  it("shows 'No scored reps.' when the set finishes without a summary", async () => {
    render(DrillPage);
    await startDrill();
    await waitFor(() => expect(drillChannel).not.toBeNull());
    drillChannel!.onmessage({ event: "setFinished", summary: null });
    expect(await screen.findByText("No scored reps.")).toBeInTheDocument();
  });

  it("shows best, average and consistency from the Rust summary", async () => {
    render(DrillPage);
    await startDrill();
    await waitFor(() => expect(drillChannel).not.toBeNull());
    // Field names exactly as sct_core::set_summary::SetSummary serializes them.
    drillChannel!.onmessage({
      event: "setFinished",
      summary: {
        repTotals: [70, 90],
        best: 90,
        average: 80,
        grade: "B",
        consistency: 87.5,
        stdDev: 10,
      },
    });
    const card = (await screen.findByText("Set Summary")).closest("div")!;
    expect(card).toHaveTextContent("Best: 90");
    expect(card).toHaveTextContent("Average: 80 (B)");
    expect(card).toHaveTextContent("Consistency: 88%");
    expect(card).not.toHaveTextContent("NaN");
  });

  // Field names and shapes exactly as sct_core::scoring::HoldScore serializes them.
  const holdScore = {
    kind: "hold",
    total: 82.4,
    grade: "B",
    accuracy: 91.2,
    timing: 70.1,
    smoothness: 88.8,
    timeInBand: 0.83,
    rmse: 0.041,
    timeToBandMs: null,
    overshoot: 0.02,
    jitter: 0.013,
  };

  it("shows the rep result card for a real hold score", async () => {
    render(DrillPage);
    await startDrill();
    await waitFor(() => expect(drillChannel).not.toBeNull());
    drillChannel!.onmessage({ event: "repScored", rep: 0, score: holdScore });
    const card = (await screen.findByText("Rep Result")).closest("div")!;
    expect(card).toHaveTextContent("82");
    expect(card).toHaveTextContent("B");
    expect(card).toHaveTextContent("Accuracy: 91");
    expect(card).toHaveTextContent("Avg error ±4.1%");
    expect(card).toHaveTextContent("Shakiness 1.3%");
    expect(card).toHaveTextContent("What do these mean?");
    expect(card).toHaveTextContent("How fast you got into the band.");
    expect(card).not.toHaveTextContent("NaN");
  });

  it("counts down numbers, then shows GO in the last second and the hold timer on rep start", async () => {
    let nowUs = 0;
    vi.spyOn(pedalStream, "dataNowUs").mockImplementation(() => nowUs);
    render(DrillPage);
    await startDrill();
    await waitFor(() => expect(drillChannel).not.toBeNull());
    drillChannel!.onmessage({ event: "countdownStarted", rep: 0, startUs: 0, endsUs: 3_000_000 });
    expect(await screen.findByText("2")).toBeInTheDocument();
    expect(screen.queryByText("Get Ready!")).not.toBeInTheDocument();
    nowUs = 1_500_000;
    expect(await screen.findByText("1")).toBeInTheDocument();
    nowUs = 2_500_000;
    expect(await screen.findByText("GO")).toBeInTheDocument();

    nowUs = 3_400_000;
    drillChannel!.onmessage({ event: "repStarted", rep: 0, startUs: 3_000_000 });
    const hud = await screen.findByTestId("hold-hud");
    await waitFor(() => expect(hud).toHaveTextContent("Hold 1.6s"));
    expect(hud).toHaveTextContent("Target 70% ±5");
    expect(screen.getByRole("progressbar", { name: "Hold time left" })).toBeInTheDocument();
    expect(screen.queryByText("GO")).not.toBeInTheDocument();
  });

  it("renders Avg error from rmse as a percentage", async () => {
    render(DrillPage);
    await startDrill();
    await waitFor(() => expect(drillChannel).not.toBeNull());
    drillChannel!.onmessage({ event: "repScored", rep: 0, score: { ...holdScore, rmse: 0.07 } });
    expect(await screen.findByText("Avg error ±7.0%")).toBeInTheDocument();
  });

  it("keeps the aborted set's summary and lists failed reps", async () => {
    render(DrillPage);
    await startDrill();
    await waitFor(() => expect(drillChannel).not.toBeNull());
    drillChannel!.onmessage({ event: "repScored", rep: 0, score: holdScore });
    drillChannel!.onmessage({ event: "repFailed", rep: 1 });
    await fireEvent.click(await screen.findByRole("button", { name: "Abort Set" }));
    await waitFor(() => expect(aborts).toBe(1));
    // The engine answers an abort with the final event.
    drillChannel!.onmessage({
      event: "setFinished",
      summary: {
        repTotals: [82.4],
        best: 82.4,
        average: 82.4,
        grade: "B",
        consistency: null,
        stdDev: 0,
      },
    });
    expect(await screen.findByText("Set Finished!")).toBeInTheDocument();
    const card = screen.getByText("Set Summary").closest("div")!;
    expect(card).toHaveTextContent("Best: 82");
    expect(card).toHaveTextContent("#1: 82");
    expect(card).toHaveTextContent("#2: failed");
  });

  it("ignores events of a run that Play Again replaced", async () => {
    render(DrillPage);
    await startDrill();
    await waitFor(() => expect(drillChannel).not.toBeNull());
    const oldChannel = drillChannel!;
    oldChannel.onmessage({ event: "repScored", rep: 0, score: holdScore });
    oldChannel.onmessage({ event: "setFinished", summary: null });
    drillChannel = null;
    await fireEvent.click(await screen.findByRole("button", { name: "Play Again" }));
    await waitFor(() => expect(drillChannel).not.toBeNull());
    expect(drillChannel).not.toBe(oldChannel);

    oldChannel.onmessage({ event: "repScored", rep: 1, score: holdScore });
    oldChannel.onmessage({ event: "setFinished", summary: null });
    expect(screen.queryByText("Rep Result")).not.toBeInTheDocument();
    expect(screen.queryByText("Set Finished!")).not.toBeInTheDocument();
    expect(screen.getByText("COUNTDOWN")).toBeInTheDocument();
    // The first run was saved once, by its own first setFinished.
    expect(saved).toHaveLength(1);
    expect(saved[0].reps).toHaveLength(1);
  });

  const summary = {
    repTotals: [82.4, 82.4],
    best: 82.4,
    average: 82.4,
    grade: "B",
    consistency: 100,
    stdDev: 0,
  };

  it("saves a finished set with its scored reps and summary", async () => {
    render(DrillPage);
    await startDrill();
    await waitFor(() => expect(drillChannel).not.toBeNull());
    drillChannel!.onmessage({ event: "repScored", rep: 0, score: holdScore });
    drillChannel!.onmessage({ event: "repFailed", rep: 1 });
    drillChannel!.onmessage({ event: "repScored", rep: 2, score: holdScore });
    drillChannel!.onmessage({ event: "setFinished", summary });
    await waitFor(() => expect(saved).toHaveLength(1));
    const attempt = saved[0];
    expect(attempt).toMatchObject({
      drillId: "brake-hold-70",
      presetId: "starter",
      pedal: "brake",
      aborted: false,
      best: 82.4,
      average: 82.4,
      consistency: 100,
    });
    expect(new Date(attempt.startedAt).toISOString()).toBe(attempt.startedAt);
    // The failed rep has no scores, so only the two scored reps are stored.
    expect(attempt.reps.map((r) => r.repIndex)).toEqual([0, 2]);
    expect(attempt.reps[0]).toMatchObject({
      total: 82.4,
      accuracy: 91.2,
      timing: 70.1,
      smoothness: 88.8,
      timeInBand: 0.83,
      rmse: 0.041,
      overshoot: 0.02,
      timeToBandMs: null,
      jitter: 0.013,
    });
  });

  it("saves an aborted set as aborted", async () => {
    render(DrillPage);
    await startDrill();
    await waitFor(() => expect(drillChannel).not.toBeNull());
    drillChannel!.onmessage({ event: "repScored", rep: 0, score: holdScore });
    await fireEvent.click(await screen.findByRole("button", { name: "Abort Set" }));
    await waitFor(() => expect(aborts).toBe(1));
    drillChannel!.onmessage({ event: "setFinished", summary: { ...summary, repTotals: [82.4] } });
    await waitFor(() => expect(saved).toHaveLength(1));
    expect(saved[0].aborted).toBe(true);
    expect(saved[0].reps).toHaveLength(1);
  });

  it("saves a set the engine ended early as aborted, without an Abort click", async () => {
    // The pedals were unplugged after the first rep: the engine ends the set on its own.
    render(DrillPage);
    await startDrill();
    await waitFor(() => expect(drillChannel).not.toBeNull());
    drillChannel!.onmessage({ event: "repScored", rep: 0, score: holdScore });
    drillChannel!.onmessage({ event: "setFinished", summary: { ...summary, repTotals: [82.4] } });
    await waitFor(() => expect(saved).toHaveLength(1));
    expect(saved[0].aborted).toBe(true);
    expect(aborts).toBe(0);
  });

  it("saves a set as complete when Abort lands after the last rep ended", async () => {
    render(DrillPage);
    await startDrill();
    await waitFor(() => expect(drillChannel).not.toBeNull());
    drillChannel!.onmessage({ event: "repScored", rep: 0, score: holdScore });
    drillChannel!.onmessage({ event: "repScored", rep: 1, score: holdScore });
    drillChannel!.onmessage({ event: "repFailed", rep: 2 });
    await fireEvent.click(await screen.findByRole("button", { name: "Abort Set" }));
    drillChannel!.onmessage({ event: "setFinished", summary });
    await waitFor(() => expect(saved).toHaveLength(1));
    expect(saved[0].aborted).toBe(false);
  });

  it("saves the set as aborted when the page is left mid-set", async () => {
    const { unmount } = render(DrillPage);
    await startDrill();
    await waitFor(() => expect(drillChannel).not.toBeNull());
    drillChannel!.onmessage({ event: "repScored", rep: 0, score: holdScore });
    unmount();
    await waitFor(() => expect(aborts).toBe(1));
    // The engine answers the abort after the page is gone.
    drillChannel!.onmessage({ event: "setFinished", summary: { ...summary, repTotals: [82.4] } });
    await waitFor(() => expect(saved).toHaveLength(1));
    expect(saved[0]).toMatchObject({ drillId: "brake-hold-70", aborted: true });
  });

  it("does not save a set the engine ended before any rep", async () => {
    render(DrillPage);
    await startDrill();
    await waitFor(() => expect(drillChannel).not.toBeNull());
    drillChannel!.onmessage({ event: "setFinished", summary: null });
    expect(await screen.findByText("No scored reps.")).toBeInTheDocument();
    await new Promise((r) => setTimeout(r, 20));
    expect(saved).toHaveLength(0);
  });

  it("does not save a set aborted before any rep ended", async () => {
    render(DrillPage);
    await startDrill();
    await waitFor(() => expect(drillChannel).not.toBeNull());
    await fireEvent.click(await screen.findByRole("button", { name: "Abort Set" }));
    await waitFor(() => expect(aborts).toBe(1));
    drillChannel!.onmessage({ event: "setFinished", summary: null });
    expect(await screen.findByText("Set Finished!")).toBeInTheDocument();
    await new Promise((r) => setTimeout(r, 20));
    expect(saved).toHaveLength(0);
  });

  it("keeps the summary on screen and shows an alert when the save fails", async () => {
    saveError = "attempts store is unavailable";
    const logged = vi.spyOn(console, "error").mockImplementation(() => {});
    render(DrillPage);
    await startDrill();
    await waitFor(() => expect(drillChannel).not.toBeNull());
    drillChannel!.onmessage({ event: "repScored", rep: 0, score: holdScore });
    drillChannel!.onmessage({ event: "setFinished", summary });
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("This set was not saved");
    expect(alert).toHaveTextContent("attempts store is unavailable");
    expect(logged).toHaveBeenCalled();
    expect(screen.getByText("Set Summary")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Play Again" })).toBeInTheDocument();
  });

  const traceScore = {
    kind: "trace",
    total: 88.5,
    grade: "B",
    accuracy: 90.0,
    timing: 85.0,
    smoothness: 90.0,
    lagMs: 42.0,
    timeInBand: 0.85,
    rmse: 0.035,
    overshoot: 0.01,
    ldljUser: -12.5,
    ldljTarget: -10.2,
  };

  it("lists a preset with only a trace drill and shows Duration", async () => {
    presetsList = [tracePreset];
    render(DrillPage);
    expect(await screen.findByRole("option", { name: "Trace Drills" })).toBeInTheDocument();
    expect(await screen.findByRole("option", { name: /Brake trace 1.5s/ })).toBeInTheDocument();
    const info = (await screen.findByText("Type:")).closest(".drill-info")!;
    expect(info).toHaveTextContent("Duration: 1.5 s");
    expect(info).toHaveTextContent("Peak: 100%");
    expect(info).toHaveTextContent("Tolerance: ±6%");
  });

  it("shows the Playhead and Ghost view toggle for a trace drill and saves Ghost", async () => {
    presetsList = [tracePreset];
    render(DrillPage);
    const info = (await screen.findByText("Type:")).closest(".drill-info")!;
    const playhead = within(info as HTMLElement).getByRole("button", { name: "Playhead" });
    const ghost = within(info as HTMLElement).getByRole("button", { name: "Ghost" });
    expect(playhead).toHaveAttribute("aria-pressed", "true");
    expect(ghost).toHaveAttribute("aria-pressed", "false");

    await fireEvent.click(ghost);
    expect(localStorage.getItem("sct:trace_view")).toBe("ghost");
    expect(ghost).toHaveAttribute("aria-pressed", "true");
  });

  it("renders trace-view and trace-hud showing Target after repStarted for a trace drill", async () => {
    presetsList = [tracePreset];
    render(DrillPage);
    await startDrill();
    await waitFor(() => expect(drillChannel).not.toBeNull());
    drillChannel!.onmessage({ event: "repStarted", rep: 0, startUs: 1_000_000 });
    expect(await screen.findByTestId("trace-view")).toBeInTheDocument();
    const hud = await screen.findByTestId("trace-hud");
    expect(hud).toHaveTextContent("Target");
  });

  it("shows the Lag metric for a trace score", async () => {
    presetsList = [tracePreset];
    render(DrillPage);
    await startDrill();
    await waitFor(() => expect(drillChannel).not.toBeNull());
    drillChannel!.onmessage({ event: "repStarted", rep: 0, startUs: 1_000_000 });
    drillChannel!.onmessage({ event: "repScored", rep: 0, score: traceScore });
    const card = (await screen.findByText("Rep Result")).closest("div")!;
    expect(card).toHaveTextContent("Lag 42 ms");
    expect(card).toHaveTextContent("In band 85%");
    expect(card).toHaveTextContent("Off band ±3.5%");
    expect(
      screen.getByTitle("How late (positive) or early (negative) you followed the curve, in ms."),
    ).toHaveTextContent("Lag 42 ms");
    expect(screen.getByTitle("Share of the rep your pedal was inside the band.")).toHaveTextContent(
      "In band 85%",
    );
    expect(
      screen.getByTitle("Average distance outside the band, as a share of full pedal travel."),
    ).toHaveTextContent("Off band ±3.5%");
  });

  it("shows trace HUD during the GO second before rep starts", async () => {
    let nowUs = 0;
    vi.spyOn(pedalStream, "dataNowUs").mockImplementation(() => nowUs);
    const leadInPreset: Preset = {
      ...tracePreset,
      drills: [{ ...tracePreset.drills[0], leadInMs: 3000 }],
    };
    presetsList = [leadInPreset];
    render(DrillPage);
    await startDrill();
    await waitFor(() => expect(drillChannel).not.toBeNull());
    drillChannel!.onmessage({ event: "countdownStarted", rep: 0, startUs: 0, endsUs: 3_000_000 });
    nowUs = 500_000;
    expect(await screen.findByText("2")).toBeInTheDocument();
    expect(screen.queryByTestId("trace-hud")).not.toBeInTheDocument();

    nowUs = 2_500_000;
    expect(await screen.findByText("GO")).toBeInTheDocument();
    const hud = await screen.findByTestId("trace-hud");
    expect(hud).toBeInTheDocument();
    expect(hud).toHaveTextContent("Target 0%");
  });

  it("shows throttle value for a throttle trace drill in trace HUD", async () => {
    const throttleTracePreset: Preset = {
      schemaVersion: 1,
      id: "throttle-trace-only",
      name: "Throttle Trace Drills",
      description: "",
      drills: [
        {
          id: "throttle-trace",
          name: "Throttle trace 1.5s",
          type: "trace",
          pedal: "throttle",
          reps: 3,
          leadInMs: 1000,
          tolerance: 6,
          points: [
            [0, 0],
            [150, 100],
            [1500, 0],
          ],
        },
      ],
    };
    presetsList = [throttleTracePreset];
    render(DrillPage);
    await startDrill();
    await waitFor(() => expect(drillChannel).not.toBeNull());

    pedalStream.ingest([{ t: 1_000_000, brake: 0.15, throttle: 0.75 }]);
    drillChannel!.onmessage({ event: "repStarted", rep: 0, startUs: 1_000_000 });

    const hud = await screen.findByTestId("trace-hud");
    await waitFor(() => expect(hud).toHaveTextContent("You 75%"));
    expect(hud).not.toHaveTextContent("You 15%");
  });

  it("formats target with one decimal when decimals=1 on hold drill", async () => {
    const decimalsPreset: Preset = {
      schemaVersion: 1,
      id: "decimals-test",
      name: "Decimals Hold Drill",
      description: "",
      drills: [
        {
          id: "rolling-35",
          name: "Rolling start 35%",
          type: "hold",
          pedal: "throttle",
          target: 35,
          tolerance: 2,
          decimals: 1,
          holdMs: 2000,
          reps: 3,
          leadInMs: 1000,
        },
      ],
    };
    presetsList = [decimalsPreset];
    render(DrillPage);
    await startDrill();
    await waitFor(() => expect(drillChannel).not.toBeNull());
    drillChannel!.onmessage({ event: "repStarted", rep: 0, startUs: 1_000_000 });
    const hud = await screen.findByTestId("hold-hud");
    expect(hud).toHaveTextContent("Target 35.0% ±2");
  });

  const multiDrillPreset: Preset = {
    schemaVersion: 1,
    id: "advanced",
    name: "Advanced Drills",
    description: "",
    drills: [
      {
        id: "adv-drill-1",
        name: "First Advanced Drill",
        type: "hold",
        pedal: "brake",
        reps: 3,
        leadInMs: 1000,
        target: 40,
        holdMs: 2000,
      },
      {
        id: "adv-drill-2",
        name: "Second Advanced Drill",
        type: "hold",
        pedal: "throttle",
        reps: 3,
        leadInMs: 1000,
        target: 80,
        holdMs: 2000,
      },
    ],
  };

  it("selects preset and non-first drill from query params", async () => {
    presetsList = [preset, multiDrillPreset];
    mockUrl = new URL("http://localhost/drill?preset=advanced&drill=adv-drill-2");
    render(DrillPage);
    const presetOption = await screen.findByRole<HTMLOptionElement>("option", {
      name: "Advanced Drills",
    });
    expect(presetOption.selected).toBe(true);
    const drillOption = await screen.findByRole<HTMLOptionElement>("option", {
      name: /Second Advanced Drill/,
    });
    expect(drillOption.selected).toBe(true);
  });

  it("falls back to the preset's first playable drill when drill param is unknown", async () => {
    presetsList = [preset, multiDrillPreset];
    mockUrl = new URL("http://localhost/drill?preset=advanced&drill=unknown-drill");
    render(DrillPage);
    const presetOption = await screen.findByRole<HTMLOptionElement>("option", {
      name: "Advanced Drills",
    });
    expect(presetOption.selected).toBe(true);
    const drillOption = await screen.findByRole<HTMLOptionElement>("option", {
      name: /First Advanced Drill/,
    });
    expect(drillOption.selected).toBe(true);
  });

  it("falls back to the first preset when the preset param is unknown", async () => {
    presetsList = [preset, multiDrillPreset];
    mockUrl = new URL("http://localhost/drill?preset=no-such-preset");
    localStorage.setItem("sct:last_preset", "advanced");
    render(DrillPage);
    const presetOption = await screen.findByRole<HTMLOptionElement>("option", {
      name: "Starter Drills",
    });
    expect(presetOption.selected).toBe(true);
  });

  it("selects the remembered preset when there is no preset param", async () => {
    presetsList = [preset, multiDrillPreset];
    localStorage.setItem("sct:last_preset", "advanced");
    render(DrillPage);
    const presetOption = await screen.findByRole<HTMLOptionElement>("option", {
      name: "Advanced Drills",
    });
    expect(presetOption.selected).toBe(true);
    const drillOption = await screen.findByRole<HTMLOptionElement>("option", {
      name: /First Advanced Drill/,
    });
    expect(drillOption.selected).toBe(true);
  });

  it("saves the preset to sct:last_preset when the preset select changes", async () => {
    presetsList = [preset, multiDrillPreset];
    render(DrillPage);
    const advancedOption = await screen.findByRole<HTMLOptionElement>("option", {
      name: "Advanced Drills",
    });
    expect(localStorage.getItem("sct:last_preset")).toBeNull();
    advancedOption.selected = true;
    await fireEvent.change(advancedOption.closest("select")!);
    expect(localStorage.getItem("sct:last_preset")).toBe("advanced");
  });
});
