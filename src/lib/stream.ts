import { Channel, invoke } from "@tauri-apps/api/core";

/** Mirrors `sct_core::input::RawSample`: raw SDL axis values (-32768..32767). */
export interface RawSample {
  /** Monotonic timestamp in microseconds (QPC on Windows). */
  tUs: number;
  axisCount: number;
  axes: number[];
}

/** Mirrors `sct_core::stream::StreamStats`. */
export interface StreamStats {
  sampleRateHz: number;
  /** Age of the oldest sample in the batch when Rust sent it. */
  batchAgeMs: number;
}

/** Mirrors `sct_core::stream::PedalFrame`: calibrated positions, each 0..1. */
export interface PedalFrame {
  tUs: number;
  throttle: number;
  brake: number;
  clutch: number;
}

/** Mirrors `sct_core::stream::SampleBatch`. */
export interface SampleBatch {
  samples: RawSample[];
  /** The samples with the device profile applied; empty while the device has none. */
  frames: PedalFrame[];
  stats: StreamStats;
}

/**
 * Starts streaming one device's samples (about 1 kHz, in batches every ≤8 ms).
 * Resolves to a function that stops the stream.
 */
export async function startStream(
  deviceId: number,
  onBatch: (batch: SampleBatch) => void,
): Promise<() => Promise<void>> {
  const channel = new Channel<SampleBatch>();
  channel.onmessage = onBatch;
  await invoke("start_stream", { deviceId, onBatch: channel });
  return () => invoke<void>("stop_stream");
}

/** Maps a raw SDL axis value to 0..1. */
export function normaliseRaw(raw: number): number {
  return Math.min(1, Math.max(0, (raw + 32768) / 65535));
}

/**
 * One device's sample stream shared by several consumers (raw monitor, axis wizard, …).
 * Consumers read `latest` when they need it or subscribe to every batch.
 */
export class DeviceStream {
  latest: RawSample | null = null;
  /** Newest calibrated frame; null until the device has a profile. */
  latestFrame: PedalFrame | null = null;
  stats: StreamStats | null = null;
  private listeners = new Set<(batch: SampleBatch) => void>();
  private stopFn: (() => Promise<void>) | undefined;
  private stopped = false;

  constructor(readonly deviceId: number) {}

  /** Starts the Rust stream. Rejects if the device is gone. */
  async start(): Promise<void> {
    const stop = await startStream(this.deviceId, (batch) => {
      const last = batch.samples.at(-1);
      if (last) this.latest = last;
      this.latestFrame = batch.frames.at(-1) ?? null;
      this.stats = batch.stats;
      for (const listener of this.listeners) listener(batch);
    });
    if (this.stopped) await stop();
    else this.stopFn = stop;
  }

  /** Calls `listener` for every batch; returns an unsubscribe function. */
  subscribe(listener: (batch: SampleBatch) => void): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  async stop(): Promise<void> {
    this.stopped = true;
    this.listeners.clear();
    await this.stopFn?.();
  }
}
