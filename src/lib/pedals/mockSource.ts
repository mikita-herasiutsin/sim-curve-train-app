import type { PedalStream } from "./stream";
import type { PedalFrame } from "./types";

export interface MockSourceOptions {
  /** Delivery interval in ms (default: 8) */
  intervalMs?: number;
  /** Frame generation sample rate in Hz (default: 1000) */
  sampleRateHz?: number;
}

const CYCLE_PERIOD_MS = 2650;
const BRAKE_RAMP_MS = 150;
const BRAKE_TRAIL_MS = 1500;
const THROTTLE_RAMP_MS = 400;

/**
 * Calculates brake and throttle values for a given time in the repeating cycle:
 * - 0..150 ms: brake ramps to 0.90, throttle drops to 0
 * - 150..1650 ms: brake trails off from 0.90 to 0, throttle stays at 0
 * - 1650..2650 ms: brake rests at 0, throttle ramps smoothly to 1.0 and stays at 1.0
 */
export function generatePedalSample(ms: number): { brake: number; throttle: number } {
  const cycleMs = ((ms % CYCLE_PERIOD_MS) + CYCLE_PERIOD_MS) % CYCLE_PERIOD_MS;

  if (cycleMs < BRAKE_RAMP_MS) {
    const p = cycleMs / BRAKE_RAMP_MS;
    const brake = 0.9 * p;
    const throttle = Math.max(0, 1 - cycleMs / 50);
    return { brake, throttle };
  }

  if (cycleMs < BRAKE_RAMP_MS + BRAKE_TRAIL_MS) {
    const p = (cycleMs - BRAKE_RAMP_MS) / BRAKE_TRAIL_MS;
    const brake = 0.9 * (1 - p);
    return { brake, throttle: 0 };
  }

  // Rest period for brake (1000 ms)
  const restMs = cycleMs - (BRAKE_RAMP_MS + BRAKE_TRAIL_MS);
  const throttle = Math.min(1, restMs / THROTTLE_RAMP_MS);
  return { brake: 0, throttle };
}

export function startMockSource(stream: PedalStream, opts?: MockSourceOptions): () => void {
  const intervalMs = opts?.intervalMs ?? 8;
  const sampleRateHz = opts?.sampleRateHz ?? 1000;
  const framesPerBatch = Math.max(1, Math.round((intervalMs * sampleRateHz) / 1000));
  const frameIntervalMs = 1000 / sampleRateHz;

  const tick = () => {
    const nowMs = performance.now();
    const batch: PedalFrame[] = new Array(framesPerBatch);

    for (let i = 0; i < framesPerBatch; i++) {
      const frameMs = nowMs - (framesPerBatch - 1 - i) * frameIntervalMs;
      const { brake, throttle } = generatePedalSample(frameMs);
      batch[i] = {
        t: Math.round(frameMs * 1000),
        brake,
        throttle,
      };
    }

    stream.ingest(batch);
  };

  const timer = setInterval(tick, intervalMs);

  return () => {
    clearInterval(timer);
  };
}
