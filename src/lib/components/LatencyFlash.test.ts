import { describe, expect, it, vi, beforeEach, afterEach } from "vitest";
import { render, screen } from "@testing-library/svelte";
import LatencyFlash from "./LatencyFlash.svelte";
import { PedalStream } from "$lib/pedals/stream";

describe("LatencyFlash component", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("shows hint when enabled and hides when disabled", () => {
    const { rerender } = render(LatencyFlash, { enabled: true });
    expect(screen.getByTestId("latency-hint")).toBeInTheDocument();

    rerender({ enabled: false });
    expect(screen.queryByTestId("latency-hint")).not.toBeInTheDocument();
  });

  it("triggers white overlay flash for 100 ms on rising edge above threshold", async () => {
    const stream = new PedalStream();
    render(LatencyFlash, { enabled: true, stream, threshold: 0.5, rearm: 0.4 });

    expect(screen.queryByTestId("flash-overlay")).not.toBeInTheDocument();

    // Push brake value crossing above 0.5
    stream.ingest([{ t: 1000, brake: 0.8, throttle: 0 }]);

    // Run animation frame and flush microtasks
    await vi.advanceTimersByTimeAsync(20);

    // Overlay should now be present
    expect(screen.getByTestId("flash-overlay")).toBeInTheDocument();

    // Advance 100 ms
    await vi.advanceTimersByTimeAsync(120);

    // Overlay should be gone
    expect(screen.queryByTestId("flash-overlay")).not.toBeInTheDocument();
  });
});
