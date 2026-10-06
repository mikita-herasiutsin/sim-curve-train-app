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

/** Mirrors `sct_core::stream::SampleBatch`. */
export interface SampleBatch {
  samples: RawSample[];
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
