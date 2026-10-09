import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import DrillPage from "./+page.svelte";
import type { Preset } from "$lib/drill";
import type { NewAttempt } from "$lib/attempts";
import { pedalStream } from "$lib/pedals/stream";

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
  let saved: NewAttempt[] = [];
  let saveError: string | null = null;

  beforeEach(() => {
    startError = null;
    drillChannel = null;
    aborts = 0;
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
            return [preset];
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

  it("counts down 3, 2, 1, then shows GO! and the hold timer", async () => {
    let nowUs = 0;
    vi.spyOn(pedalStream, "dataNowUs").mockImplementation(() => nowUs);
    render(DrillPage);
    await startDrill();
    await waitFor(() => expect(drillChannel).not.toBeNull());
    drillChannel!.onmessage({ event: "countdownStarted", rep: 0, startUs: 0, endsUs: 3_000_000 });
    expect(await screen.findByText("3")).toBeInTheDocument();
    expect(screen.queryByText("Get Ready!")).not.toBeInTheDocument();
    nowUs = 1_500_000;
    expect(await screen.findByText("2")).toBeInTheDocument();
    nowUs = 2_500_000;
    expect(await screen.findByText("1")).toBeInTheDocument();

    nowUs = 3_400_000;
    drillChannel!.onmessage({ event: "repStarted", rep: 0, startUs: 3_000_000 });
    expect(await screen.findByText("GO!")).toBeInTheDocument();
    const hud = await screen.findByTestId("hold-hud");
    await waitFor(() => expect(hud).toHaveTextContent("Hold 1.6s"));
    expect(hud).toHaveTextContent("Target 70% ±5");
    expect(screen.getByRole("progressbar", { name: "Hold time left" })).toBeInTheDocument();
    await waitFor(() => expect(screen.queryByText("GO!")).not.toBeInTheDocument());
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
});
