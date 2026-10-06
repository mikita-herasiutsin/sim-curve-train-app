import { FrameHistory } from "./history";
import type { PedalFrame } from "./types";

export type BatchListener = (batch: PedalFrame[]) => void;

export interface BatchIntervalStats {
  avgMs: number;
  maxMs: number;
}

export class PedalStream {
  readonly history: FrameHistory;
  private readonly listeners = new Set<BatchListener>();
  private batchArrivalTimes: number[] = [];
  // Sample clock (Rust, µs) vs UI clock (performance.now, ms): see `dataNowUs`.
  private lastSampleUs = 0;
  private lastArrivalMs = 0;
  private lastDataNowUs = 0;
  private hasData = false;

  constructor(history = new FrameHistory()) {
    this.history = history;
  }

  /**
   * "Now" on the sample clock, for drawing. Frame timestamps come from the Rust input
   * thread, not `performance.now()`, so this extrapolates from the newest frame by the UI
   * time since its batch arrived. It never goes backwards, so the graph scrolls smoothly
   * even though batches arrive with some jitter.
   */
  dataNowUs(nowMs = performance.now()): number {
    if (!this.hasData) return nowMs * 1000;
    const estimate = this.lastSampleUs + (nowMs - this.lastArrivalMs) * 1000;
    this.lastDataNowUs = Math.max(this.lastDataNowUs, estimate);
    return this.lastDataNowUs;
  }

  ingest(batch: PedalFrame[]): void {
    if (batch.length === 0) {
      return;
    }

    const now = performance.now();
    this.batchArrivalTimes.push(now);
    this.pruneBatchArrivalTimes(now);

    this.history.pushMany(batch);
    this.lastSampleUs = batch[batch.length - 1].t;
    this.lastArrivalMs = now;
    this.hasData = true;

    for (const listener of this.listeners) {
      listener(batch);
    }
  }

  addListener(listener: BatchListener): void {
    this.listeners.add(listener);
  }

  removeListener(listener: BatchListener): void {
    this.listeners.delete(listener);
  }

  subscribe(listener: BatchListener): () => void {
    this.addListener(listener);
    return () => this.removeListener(listener);
  }

  getSampleRateHz(): number {
    const now = performance.now();
    this.pruneBatchArrivalTimes(now);

    // If no batches arrived in the last 1 second, sample rate is 0
    if (this.batchArrivalTimes.length === 0) {
      return 0;
    }

    const latest = this.history.latest();
    if (!latest) {
      return 0;
    }

    const windowUs = 1_000_000;
    const stats = this.history.getStatsSince(latest.t - windowUs);
    if (!stats || stats.count < 2) {
      return 0;
    }

    const dtSec = (stats.newestT - stats.oldestT) / 1_000_000;
    if (dtSec <= 0) {
      return 0;
    }

    return (stats.count - 1) / dtSec;
  }

  get sampleRateHz(): number {
    return this.getSampleRateHz();
  }

  getBatchIntervalStats(): BatchIntervalStats {
    const now = performance.now();
    this.pruneBatchArrivalTimes(now);

    const times = this.batchArrivalTimes;
    if (times.length < 2) {
      return { avgMs: 0, maxMs: 0 };
    }

    let maxDiff = 0;
    for (let i = 1; i < times.length; i++) {
      const diff = times[i] - times[i - 1];
      if (diff > maxDiff) {
        maxDiff = diff;
      }
    }

    const totalDiff = times[times.length - 1] - times[0];
    const avgMs = totalDiff / (times.length - 1);

    return {
      avgMs,
      maxMs: maxDiff,
    };
  }

  get batchIntervalStats(): BatchIntervalStats {
    return this.getBatchIntervalStats();
  }

  clear(): void {
    this.history.clear();
    this.batchArrivalTimes = [];
    this.lastSampleUs = 0;
    this.lastArrivalMs = 0;
    this.lastDataNowUs = 0;
    this.hasData = false;
  }

  private pruneBatchArrivalTimes(now: number): void {
    const threshold = now - 1000;
    const firstValidIdx = this.batchArrivalTimes.findIndex((t) => t >= threshold);
    if (firstValidIdx === -1) {
      this.batchArrivalTimes = [];
    } else if (firstValidIdx > 0) {
      this.batchArrivalTimes = this.batchArrivalTimes.slice(firstValidIdx);
    }
  }
}

export const pedalStream = new PedalStream();
