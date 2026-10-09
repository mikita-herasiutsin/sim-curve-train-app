import { afterEach, describe, expect, it, vi } from "vitest";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";
import { PedalStream } from "./stream";
import { startRealSource, type SourceStatus } from "./realSource";

type DeviceLike = {
  id: number;
  name: string;
  guid: string;
  vendorId: null;
  productId: null;
  axisCount: number;
  buttonCount: number;
  hatCount: number;
  simulated: boolean;
};

function makeDevice(id: number): DeviceLike {
  return {
    id,
    name: "device-" + id,
    guid: "g",
    vendorId: null,
    productId: null,
    axisCount: 4,
    buttonCount: 0,
    hatCount: 0,
    simulated: false,
  };
}

function createMockState() {
  const state = {
    devices: [] as DeviceLike[],
    profiled: [] as number[],
  };
  const calls = {
    startStream: [] as number[],
    stopStream: [] as number[],
  };
  let token = 0;

  const handler = (cmd: string, args: unknown) => {
    switch (cmd) {
      case "list_devices":
        return { devices: state.devices, error: null };
      case "profiled_devices":
        return state.profiled;
      case "start_stream":
        token += 1;
        calls.startStream.push((args as { deviceId: number }).deviceId);
        return token;
      case "stop_stream":
        calls.stopStream.push((args as { token: number }).token);
        return null;
      default:
        throw new Error("Unexpected command: " + cmd);
    }
  };

  return { state, calls, handler };
}

afterEach(() => {
  clearMocks();
});

describe("startRealSource", () => {
  it("reports noProfile when no device has a profile", async () => {
    const { state, handler } = createMockState();
    mockIPC(handler, { shouldMockEvents: true });

    const pedals = makeDevice(1);
    state.devices = [pedals];
    state.profiled = [];

    const statuses: SourceStatus[] = [];
    const stream = { clear: vi.fn(), ingest: vi.fn() } as unknown as PedalStream;
    startRealSource(stream, (status) => statuses.push(status));

    await vi.waitFor(() => {
      expect(statuses[statuses.length - 1]).toEqual({ kind: "noProfile" });
    });
  });

  it("starts the profiled device and reports live with that device", async () => {
    const { state, calls, handler } = createMockState();
    mockIPC(handler, { shouldMockEvents: true });

    const pedals = makeDevice(1);
    state.devices = [pedals];
    state.profiled = [1];

    const statuses: SourceStatus[] = [];
    const stream = { clear: vi.fn(), ingest: vi.fn() } as unknown as PedalStream;
    startRealSource(stream, (status) => statuses.push(status));

    await vi.waitFor(() => {
      expect(statuses[statuses.length - 1]).toMatchObject({ kind: "live", device: pedals });
      expect(calls.startStream).toContain(1);
    });
  });

  it("hot-plug: starts with no devices, then emits devices-changed", async () => {
    const { state, calls, handler } = createMockState();
    mockIPC(handler, { shouldMockEvents: true });

    const pedals = makeDevice(1);
    state.devices = [];
    state.profiled = [];

    const statuses: SourceStatus[] = [];
    const stream = { clear: vi.fn(), ingest: vi.fn() } as unknown as PedalStream;
    startRealSource(stream, (status) => statuses.push(status));

    await vi.waitFor(() => {
      expect(statuses[statuses.length - 1]).toEqual({ kind: "noProfile" });
    });

    state.devices = [pedals];
    state.profiled = [1];
    await emit("devices-changed", { devices: state.devices, error: null });

    await vi.waitFor(() => {
      expect(statuses[statuses.length - 1]).toMatchObject({ kind: "live", device: pedals });
      expect(calls.startStream).toContain(1);
    });
  });

  it("unplug: stops the live device and reports it disconnected", async () => {
    const { state, calls, handler } = createMockState();
    mockIPC(handler, { shouldMockEvents: true });

    const pedals = makeDevice(1);
    state.devices = [pedals];
    state.profiled = [1];

    const statuses: SourceStatus[] = [];
    const stream = { clear: vi.fn(), ingest: vi.fn() } as unknown as PedalStream;
    startRealSource(stream, (status) => statuses.push(status));

    await vi.waitFor(() => {
      expect(statuses[statuses.length - 1]).toMatchObject({ kind: "live", device: pedals });
    });

    state.devices = [];
    state.profiled = [];
    await emit("devices-changed", { devices: state.devices, error: null });

    await vi.waitFor(() => {
      expect(calls.stopStream).toContain(1);
      expect(statuses[statuses.length - 1]).toEqual({ kind: "disconnected", device: pedals });
      expect(stream.clear).toHaveBeenCalled();
    });
  });

  it("the returned stop function calls stop_stream with the token", async () => {
    const { state, calls, handler } = createMockState();
    mockIPC(handler, { shouldMockEvents: true });

    const pedals = makeDevice(1);
    state.devices = [pedals];
    state.profiled = [1];

    const statuses: SourceStatus[] = [];
    const stream = { clear: vi.fn(), ingest: vi.fn() } as unknown as PedalStream;
    const stop = startRealSource(stream, (status) => statuses.push(status));

    await vi.waitFor(() => {
      expect(statuses[statuses.length - 1]).toMatchObject({ kind: "live", device: pedals });
    });

    stop();
    await vi.waitFor(() => {
      expect(calls.stopStream).toContain(1);
    });
  });
});
