import { listDevices, onDevicesChanged, type DeviceInfo } from "$lib/devices";
import { profiledDevices } from "$lib/profile";
import { DeviceStream } from "$lib/stream";
import type { PedalStream } from "./stream";

export type SourceStatus =
  | { kind: "connecting" }
  /** No connected device has a saved profile: the user has to set up pedals first. */
  | { kind: "noProfile" }
  | { kind: "live"; device: DeviceInfo }
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
  let deviceInfo: DeviceInfo | null = null;
  let stopped = false;
  let connecting = false;
  let unlisten: (() => void) | undefined;

  async function connect() {
    if (connecting || stopped) return;
    connecting = true;
    try {
      const [ids, snapshot] = await Promise.all([profiledDevices(), listDevices()]);
      const info = snapshot.devices.find((d) => d.id === ids[0]);
      if (!info) {
        onStatus({ kind: "noProfile" });
        return;
      }
      const next = new DeviceStream(info.id);
      next.subscribe(({ frames }) => {
        if (frames.length === 0) return;
        stream.ingest(
          frames.map((f) => ({ t: f.tUs, brake: f.brake, throttle: f.throttle, clutch: f.clutch })),
        );
      });
      await next.start();
      if (stopped) {
        await next.stop();
        return;
      }
      device = next;
      deviceInfo = info;
      onStatus({ kind: "live", device: info });
    } catch (e: unknown) {
      onStatus({ kind: "error", message: String(e) });
    } finally {
      connecting = false;
    }
  }

  onDevicesChanged((snapshot) => {
    if (deviceInfo && !snapshot.devices.some((d) => d.id === deviceInfo?.id)) {
      // Unplugged: drop the stream and wait for a profiled device to come back.
      void device?.stop();
      device = null;
      deviceInfo = null;
      onStatus({ kind: "noProfile" });
    }
    if (!device) void connect();
  })
    .then((fn) => (stopped ? fn() : (unlisten = fn)))
    .catch((e: unknown) => onStatus({ kind: "error", message: String(e) }));

  onStatus({ kind: "connecting" });
  void connect();

  return () => {
    stopped = true;
    unlisten?.();
    void device?.stop();
  };
}
