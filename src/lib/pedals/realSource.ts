import { listDevices, onDevicesChanged, type DeviceInfo } from "$lib/devices";
import { profiledDevices } from "$lib/profile";
import { DeviceStream } from "$lib/stream";
import type { PedalStream } from "./stream";

export type SourceStatus =
  | { kind: "connecting" }
  /** No connected device has a saved profile: the user has to set up pedals first. */
  | { kind: "noProfile" }
  | { kind: "live"; device: DeviceInfo; token: number }
  /** The device that was live got unplugged; it reconnects when it comes back. */
  | { kind: "disconnected"; device: DeviceInfo }
  | { kind: "error"; message: string };

/**
 * Streams the calibrated frames of the first connected device with a saved profile into
 * `stream`. Reconnects when that device is unplugged and plugged back in, or when a profiled
 * device shows up later. Returns a stop function.
 */
export function startRealSource(
  stream: PedalStream,
  onStatus: (status: SourceStatus) => void,
): () => void {
  let device: DeviceStream | null = null;
  let lastLive: DeviceInfo | null = null;
  let stopped = false;
  let syncing = false;
  // Set when the device list changes during a sync, so the sync runs again with fresh data.
  let dirty = false;
  let unlisten: (() => void) | undefined;

  async function detach() {
    const old = device;
    device = null;
    // Old frames would sit at the wrong place on the time axis of a new stream.
    stream.clear();
    await old?.stop();
  }

  /** Makes the active stream match the current device list. Runs one at a time. */
  async function sync() {
    if (syncing) {
      dirty = true;
      return;
    }
    syncing = true;
    try {
      do {
        dirty = false;
        const [ids, snapshot] = await Promise.all([profiledDevices(), listDevices()]);
        if (stopped) return;
        const target = snapshot.devices.find((d) => ids.includes(d.id));
        if (device && device.deviceId === target?.id) continue;
        await detach();
        if (!target) {
          onStatus(lastLive ? { kind: "disconnected", device: lastLive } : { kind: "noProfile" });
          continue;
        }
        const next = new DeviceStream(target.id);
        next.subscribe(({ frames }) => {
          if (frames.length === 0) return;
          stream.ingest(
            frames.map((f) => ({
              t: f.tUs,
              brake: f.brake,
              throttle: f.throttle,
              clutch: f.clutch,
            })),
          );
        });
        await next.start();
        if (stopped) {
          await next.stop();
          return;
        }
        device = next;
        lastLive = target;
        // A device that disappeared during start() set `dirty`, and the next pass detaches it.
        onStatus({ kind: "live", device: target, token: next.streamToken! });
      } while (dirty && !stopped);
    } catch (e: unknown) {
      onStatus({ kind: "error", message: String(e) });
    } finally {
      syncing = false;
    }
  }

  onDevicesChanged(() => void sync())
    .then((fn) => (stopped ? fn() : (unlisten = fn)))
    .catch((e: unknown) => onStatus({ kind: "error", message: String(e) }));

  onStatus({ kind: "connecting" });
  void sync();

  return () => {
    stopped = true;
    unlisten?.();
    void detach();
  };
}
