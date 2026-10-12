import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { cleanup, render, screen, waitFor } from "@testing-library/svelte";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import DevicePanel from "./DevicePanel.svelte";
import type { DeviceInfo } from "$lib/devices";

describe("DevicePanel component", () => {
  let startStreamArgs: Record<string, unknown> | undefined;

  beforeEach(() => {
    startStreamArgs = undefined;
    mockIPC((cmd, args) => {
      switch (cmd) {
        case "load_profile":
          return null;
        case "start_stream":
          startStreamArgs = args as Record<string, unknown>;
          return 1;
        case "stop_stream":
          return null;
        default:
          throw new Error(`Unexpected command: ${cmd}`);
      }
    });
  });

  afterEach(() => {
    cleanup();
    clearMocks();
  });

  const baseDevice: DeviceInfo = {
    id: 1,
    name: "Fanatec CSL Pedals",
    guid: "x360:0001:0001:0001",
    vendorId: 0x0eb7,
    productId: 0x0001,
    axisCount: 3,
    buttonCount: 0,
    hatCount: 0,
    simulated: false,
  };

  it("does not show Simulated badge for regular devices", async () => {
    render(DevicePanel, { device: { ...baseDevice, simulated: false } });

    expect(await screen.findByText("Fanatec CSL Pedals")).toBeInTheDocument();
    expect(screen.queryByText("Simulated")).not.toBeInTheDocument();
  });

  it("shows small Simulated badge for simulated devices", async () => {
    render(DevicePanel, {
      device: { ...baseDevice, name: "SCT Simulated Pedals", simulated: true },
    });

    expect(await screen.findByText("SCT Simulated Pedals")).toBeInTheDocument();
    expect(await screen.findByText("Simulated")).toBeInTheDocument();
  });

  it("starts the stream with raw samples for the device setup screens", async () => {
    render(DevicePanel, { device: baseDevice });

    await waitFor(() => expect(startStreamArgs).toBeDefined());
    expect(startStreamArgs).toMatchObject({ deviceId: baseDevice.id, raw: true });
  });
});
