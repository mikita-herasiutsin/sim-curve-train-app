import { afterEach, describe, expect, it } from "vitest";
import { cleanup, render, screen, waitFor } from "@testing-library/svelte";
import PedalWizard from "./PedalWizard.svelte";
import { DeviceStream } from "$lib/stream";

describe("PedalWizard live input", () => {
  afterEach(cleanup);

  it("shows a live bar and percentage for an assigned axis", async () => {
    const stream = new DeviceStream(1);
    stream.latest = { tUs: 1, axisCount: 3, axes: [0, 0, 0] };
    render(PedalWizard, {
      stream,
      axisCount: 3,
      assignments: { throttle: { axis: 1, rest: -32768, min: -32768, max: 32767 } },
    });
    expect(screen.getByRole("meter", { name: "Throttle live input" })).toBeInTheDocument();
    // Raw 0 is the middle of the full range.
    await waitFor(() => expect(screen.getByText("50%")).toBeInTheDocument());
    stream.latest = { tUs: 2, axisCount: 3, axes: [0, 32767, 0] };
    await waitFor(() => expect(screen.getByText("100%")).toBeInTheDocument());
  });

  it("shows no bar for an unassigned pedal", () => {
    render(PedalWizard, { stream: new DeviceStream(1), axisCount: 3, assignments: {} });
    expect(screen.queryByRole("meter")).not.toBeInTheDocument();
  });
});
