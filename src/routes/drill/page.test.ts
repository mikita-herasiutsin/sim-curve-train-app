import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import DrillPage from "./+page.svelte";
import type { Preset } from "$lib/drill";

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
};

interface Channelish {
  onmessage: (e: unknown) => void;
}

describe("Drill page", () => {
  let startError: string | null = null;
  let drillChannel: Channelish | null = null;

  beforeEach(() => {
    startError = null;
    drillChannel = null;
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
          case "abort_drill_run":
            return null;
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
});
