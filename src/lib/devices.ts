import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

/** Mirrors `sct_core::device::DeviceInfo` on the Rust side. */
export interface DeviceInfo {
  id: number;
  name: string;
  guid: string;
  vendorId: number | null;
  productId: number | null;
  axisCount: number;
  buttonCount: number;
  hatCount: number;
}

/** Mirrors `sct_core::device::DevicesSnapshot` on the Rust side. */
export interface DevicesSnapshot {
  devices: DeviceInfo[];
  error: string | null;
}

/** Event the input thread emits on hot-plug. */
export const DEVICES_CHANGED_EVENT = "devices-changed";

/** Fetches the current controller list from the input thread. */
export function listDevices(): Promise<DevicesSnapshot> {
  return invoke<DevicesSnapshot>("list_devices");
}

/** Calls `onChange` with the new list whenever a controller is plugged in or removed. */
export function onDevicesChanged(onChange: (s: DevicesSnapshot) => void): Promise<UnlistenFn> {
  return listen<DevicesSnapshot>(DEVICES_CHANGED_EVENT, (event) => onChange(event.payload));
}

/** Formats USB ids as `VID:PID` in lowercase hex, e.g. `045e:028e`. */
export function formatUsbIds(device: Pick<DeviceInfo, "vendorId" | "productId">): string {
  if (device.vendorId === null || device.productId === null) return "n/a";
  const hex = (n: number) => n.toString(16).padStart(4, "0");
  return `${hex(device.vendorId)}:${hex(device.productId)}`;
}
