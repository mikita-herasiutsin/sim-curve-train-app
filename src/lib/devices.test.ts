import { afterEach, describe, expect, it } from "vitest";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";
import {
  formatUsbIds,
  listDevices,
  onDevicesChanged,
  setSimPedals,
  type DevicesSnapshot,
} from "./devices";

describe("devices module", () => {
  afterEach(() => {
    clearMocks();
  });

  describe("setSimPedals", () => {
    it("invokes set_sim_pedals with values and auto flag", async () => {
      const calls: Array<{ cmd: string; args: unknown }> = [];
      mockIPC((cmd, args) => {
        calls.push({ cmd, args });
        return null;
      });

      await setSimPedals([0.25, 0.75, 0.1], false);

      expect(calls).toEqual([
        {
          cmd: "set_sim_pedals",
          args: { values: [0.25, 0.75, 0.1], auto: false },
        },
      ]);
    });

    it("invokes set_sim_pedals with auto: true", async () => {
      const calls: Array<{ cmd: string; args: unknown }> = [];
      mockIPC((cmd, args) => {
        calls.push({ cmd, args });
        return null;
      });

      await setSimPedals([0, 0, 0], true);

      expect(calls).toEqual([
        {
          cmd: "set_sim_pedals",
          args: { values: [0, 0, 0], auto: true },
        },
      ]);
    });
  });

  describe("listDevices", () => {
    it("invokes list_devices and returns snapshot", async () => {
      const snapshot: DevicesSnapshot = {
        devices: [
          {
            id: 1,
            name: "SCT Simulated Pedals",
            guid: "ff00:0001",
            vendorId: null,
            productId: null,
            axisCount: 3,
            buttonCount: 0,
            hatCount: 0,
            simulated: true,
          },
        ],
        error: null,
      };

      mockIPC((cmd) => {
        if (cmd === "list_devices") return snapshot;
        throw new Error(`Unexpected command: ${cmd}`);
      });

      const result = await listDevices();
      expect(result).toEqual(snapshot);
    });
  });

  describe("onDevicesChanged", () => {
    it("receives emitted devices-changed events", async () => {
      mockIPC(() => null, { shouldMockEvents: true });

      const received: DevicesSnapshot[] = [];
      const unlisten = await onDevicesChanged((s) => received.push(s));

      const snapshot: DevicesSnapshot = {
        devices: [
          {
            id: 1,
            name: "Pedals",
            guid: "g",
            vendorId: 0x1234,
            productId: 0x5678,
            axisCount: 3,
            buttonCount: 0,
            hatCount: 0,
            simulated: false,
          },
        ],
        error: null,
      };

      await emit("devices-changed", snapshot);
      expect(received).toEqual([snapshot]);

      unlisten();
    });
  });

  describe("formatUsbIds", () => {
    it("formats vendor and product hex ids", () => {
      expect(formatUsbIds({ vendorId: 0x045e, productId: 0x028e })).toBe("045e:028e");
      expect(formatUsbIds({ vendorId: null, productId: 0x028e })).toBe("n/a");
      expect(formatUsbIds({ vendorId: 0x045e, productId: null })).toBe("n/a");
    });
  });
});
