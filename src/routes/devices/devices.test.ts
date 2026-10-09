import { afterEach, describe, expect, it } from "vitest";
import { cleanup, render, screen } from "@testing-library/svelte";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";
import DevicesPage from "./+page.svelte";
import { formatUsbIds, type DevicesSnapshot } from "$lib/devices";

describe("DevicesPage", () => {
  afterEach(() => {
    cleanup();
    clearMocks();
  });

  function mockDevices(snapshot: DevicesSnapshot) {
    mockIPC(
      (cmd) => {
        if (cmd === "app_info") {
          return { name: "SimCurveTrainApp", version: "0.1.0" };
        }
        if (cmd === "list_devices") {
          return snapshot;
        }
        throw new Error(`unexpected command ${cmd}`);
      },
      { shouldMockEvents: true },
    );
  }

  it("shows a device row returned by list_devices", async () => {
    const device = {
      id: 1,
      name: "Logitech Pedals",
      guid: "x360:0001:0001:0001",
      vendorId: 0x045e,
      productId: 0x028e,
      axisCount: 3,
      buttonCount: 12,
      hatCount: 1,
      simulated: false,
    };

    mockDevices({ devices: [device], error: null });
    render(DevicesPage);

    expect(await screen.findByText("Logitech Pedals")).toBeInTheDocument();
    expect(await screen.findByText("045e:028e")).toBeInTheDocument();
    expect(await screen.findByText("3")).toBeInTheDocument();
  });

  it("shows no controllers found when list is empty", async () => {
    mockDevices({ devices: [], error: null });
    render(DevicesPage);

    expect(await screen.findByText(/No controllers found/)).toBeInTheDocument();
  });

  it("shows the snapshot error in an alert", async () => {
    mockDevices({ devices: [], error: "SDL init failed: x" });
    render(DevicesPage);

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("SDL init failed: x");
  });

  it("updates when devices-changed is emitted", async () => {
    const hotDevice = {
      id: 2,
      name: "Hot Plugged Controller",
      guid: "x360:0002:0002:0002",
      vendorId: 1,
      productId: 0x00ab,
      axisCount: 2,
      buttonCount: 8,
      hatCount: 0,
      simulated: false,
    };

    mockDevices({ devices: [], error: null });
    render(DevicesPage);

    await screen.findByText(/No controllers found/);
    await emit("devices-changed", { devices: [hotDevice], error: null });
    expect(await screen.findByText("Hot Plugged Controller")).toBeInTheDocument();
  });

  it("formats USB ids", () => {
    expect(formatUsbIds({ vendorId: null, productId: 0x00ab })).toBe("n/a");
    expect(formatUsbIds({ vendorId: 1, productId: 0x00ab })).toBe("0001:00ab");
  });
});
