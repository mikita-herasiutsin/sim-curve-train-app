import { afterEach, describe, expect, it } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";
import SimPedalsPanel from "./SimPedalsPanel.svelte";
import type { DeviceInfo, DevicesSnapshot } from "$lib/devices";

describe("SimPedalsPanel component", () => {
  afterEach(() => {
    cleanup();
    clearMocks();
  });

  const realDevice: DeviceInfo = {
    id: 1,
    name: "Real Pedals",
    guid: "real:0001",
    vendorId: 0x045e,
    productId: 0x028e,
    axisCount: 3,
    buttonCount: 0,
    hatCount: 0,
    simulated: false,
  };

  const simDevice: DeviceInfo = {
    id: 99,
    name: "SCT Simulated Pedals",
    guid: "ff00:0001",
    vendorId: null,
    productId: null,
    axisCount: 3,
    buttonCount: 0,
    hatCount: 0,
    simulated: true,
  };

  function setupIPC(snapshot: DevicesSnapshot) {
    const simCalls: Array<{ values: [number, number, number]; auto: boolean }> = [];
    let simError: string | null = null;

    mockIPC(
      (cmd, args) => {
        if (cmd === "list_devices") {
          return snapshot;
        }
        if (cmd === "set_sim_pedals") {
          if (simError) throw simError;
          simCalls.push(args as { values: [number, number, number]; auto: boolean });
          return null;
        }
        throw new Error(`Unexpected command: ${cmd}`);
      },
      { shouldMockEvents: true },
    );

    return {
      simCalls,
      setSimError: (err: string | null) => {
        simError = err;
      },
    };
  }

  it("is hidden when there is no simulated device", async () => {
    setupIPC({ devices: [realDevice], error: null });

    render(SimPedalsPanel);

    // Wait a tick for listDevices to resolve
    await waitFor(() => {
      expect(screen.queryByText("Simulated pedals (dev)")).not.toBeInTheDocument();
    });
  });

  it("renders when list_devices reports a simulated device", async () => {
    setupIPC({ devices: [simDevice], error: null });

    render(SimPedalsPanel);

    expect(await screen.findByText("Simulated pedals (dev)")).toBeInTheDocument();
    expect(screen.getByLabelText("Throttle")).toBeInTheDocument();
    expect(screen.getByLabelText("Brake")).toBeInTheDocument();
    expect(screen.getByLabelText("Clutch")).toBeInTheDocument();
    expect(screen.getByLabelText("Auto")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Release all" })).toBeInTheDocument();
  });

  it("appears and disappears on devices-changed hot-plug events", async () => {
    setupIPC({ devices: [realDevice], error: null });

    render(SimPedalsPanel);

    await waitFor(() => {
      expect(screen.queryByText("Simulated pedals (dev)")).not.toBeInTheDocument();
    });

    // Plug in simulated device
    await emit("devices-changed", { devices: [realDevice, simDevice], error: null });
    expect(await screen.findByText("Simulated pedals (dev)")).toBeInTheDocument();

    // Unplug simulated device
    await emit("devices-changed", { devices: [realDevice], error: null });
    await waitFor(() => {
      expect(screen.queryByText("Simulated pedals (dev)")).not.toBeInTheDocument();
    });
  });

  it("invokes set_sim_pedals with values and auto on slider change", async () => {
    const { simCalls } = setupIPC({ devices: [simDevice], error: null });

    render(SimPedalsPanel);
    expect(await screen.findByText("Simulated pedals (dev)")).toBeInTheDocument();

    const throttle = screen.getByLabelText("Throttle");
    await fireEvent.input(throttle, { target: { value: "60" } });

    await waitFor(() => {
      expect(simCalls.length).toBeGreaterThanOrEqual(1);
      expect(simCalls[simCalls.length - 1]).toEqual({
        values: [0.6, 0, 0],
        auto: false,
      });
    });

    const brake = screen.getByLabelText("Brake");
    await fireEvent.input(brake, { target: { value: "85" } });

    await waitFor(() => {
      expect(simCalls[simCalls.length - 1]).toEqual({
        values: [0.6, 0.85, 0],
        auto: false,
      });
    });

    const clutch = screen.getByLabelText("Clutch");
    await fireEvent.input(clutch, { target: { value: "30" } });

    await waitFor(() => {
      expect(simCalls[simCalls.length - 1]).toEqual({
        values: [0.6, 0.85, 0.3],
        auto: false,
      });
    });
  });

  it("Auto checkbox disables sliders and sends auto: true", async () => {
    const { simCalls } = setupIPC({ devices: [simDevice], error: null });

    render(SimPedalsPanel);
    expect(await screen.findByText("Simulated pedals (dev)")).toBeInTheDocument();

    const autoCheckbox = screen.getByLabelText("Auto");
    const throttle = screen.getByLabelText("Throttle");
    const brake = screen.getByLabelText("Brake");
    const clutch = screen.getByLabelText("Clutch");
    const releaseBtn = screen.getByRole("button", { name: "Release all" });

    expect(throttle).toBeEnabled();
    expect(brake).toBeEnabled();
    expect(clutch).toBeEnabled();
    expect(releaseBtn).toBeEnabled();

    await fireEvent.click(autoCheckbox);

    expect(throttle).toBeDisabled();
    expect(brake).toBeDisabled();
    expect(clutch).toBeDisabled();
    expect(releaseBtn).toBeDisabled();

    await waitFor(() => {
      expect(simCalls[simCalls.length - 1]).toEqual({
        values: [0, 0, 0],
        auto: true,
      });
    });
  });

  it("Release all button resets values to 0 and sends update", async () => {
    const { simCalls } = setupIPC({ devices: [simDevice], error: null });

    render(SimPedalsPanel);
    expect(await screen.findByText("Simulated pedals (dev)")).toBeInTheDocument();

    const throttle = screen.getByLabelText("Throttle");
    const brake = screen.getByLabelText("Brake");
    const clutch = screen.getByLabelText("Clutch");
    await fireEvent.input(throttle, { target: { value: "50" } });
    await fireEvent.input(brake, { target: { value: "70" } });
    await fireEvent.input(clutch, { target: { value: "30" } });

    await waitFor(() => {
      expect(simCalls[simCalls.length - 1]).toEqual({
        values: [0.5, 0.7, 0.3],
        auto: false,
      });
    });

    const releaseBtn = screen.getByRole("button", { name: "Release all" });
    await fireEvent.click(releaseBtn);

    await waitFor(() => {
      expect(simCalls[simCalls.length - 1]).toEqual({
        values: [0, 0, 0],
        auto: false,
      });
      expect(throttle).toHaveValue("0");
      expect(brake).toHaveValue("0");
      expect(clutch).toHaveValue("0");
    });
  });

  it("shows errors inline without throwing", async () => {
    const { setSimError } = setupIPC({ devices: [simDevice], error: null });
    setSimError("simulated pedals are off; start with SCT_SIM_PEDALS=1");

    render(SimPedalsPanel);
    expect(await screen.findByText("Simulated pedals (dev)")).toBeInTheDocument();

    const throttle = screen.getByLabelText("Throttle");
    await fireEvent.input(throttle, { target: { value: "40" } });

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("simulated pedals are off; start with SCT_SIM_PEDALS=1");
  });

  it("can collapse and expand the panel", async () => {
    setupIPC({ devices: [simDevice], error: null });

    render(SimPedalsPanel);
    expect(await screen.findByText("Simulated pedals (dev)")).toBeInTheDocument();

    const toggleBtn = screen.getByRole("button", { name: "Collapse" });
    expect(toggleBtn).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByLabelText("Throttle")).toBeInTheDocument();

    await fireEvent.click(toggleBtn);

    expect(screen.getByRole("button", { name: "Expand" })).toHaveAttribute(
      "aria-expanded",
      "false",
    );
    expect(screen.queryByLabelText("Throttle")).not.toBeInTheDocument();

    await fireEvent.click(screen.getByRole("button", { name: "Expand" }));
    expect(screen.getByLabelText("Throttle")).toBeInTheDocument();
  });
});
