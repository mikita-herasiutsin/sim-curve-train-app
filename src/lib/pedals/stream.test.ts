import { describe, expect, it, vi, beforeEach, afterEach } from "vitest";
import { PedalStream } from "./stream";
import type { PedalFrame } from "./types";

describe("PedalStream", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("ingests batches and adds to history", () => {
    const stream = new PedalStream();
    const batch: PedalFrame[] = [
      { t: 1000, brake: 0.2, throttle: 0.8 },
      { t: 2000, brake: 0.3, throttle: 0.7 },
    ];

    stream.ingest(batch);
    expect(stream.history.length).toBe(2);
    expect(stream.history.latest()?.t).toBe(2000);
  });

  it("notifies listeners on ingest and handles unsubscribe", () => {
    const stream = new PedalStream();
    const listenerA = vi.fn();
    const listenerB = vi.fn();

    const unsub = stream.subscribe(listenerA);
    stream.addListener(listenerB);

    const batch1: PedalFrame[] = [{ t: 1000, brake: 0.1, throttle: 0 }];
    stream.ingest(batch1);

    expect(listenerA).toHaveBeenCalledWith(batch1);
    expect(listenerB).toHaveBeenCalledWith(batch1);

    unsub();
    stream.removeListener(listenerB);

    const batch2: PedalFrame[] = [{ t: 2000, brake: 0.2, throttle: 0 }];
    stream.ingest(batch2);

    expect(listenerA).toHaveBeenCalledTimes(1);
    expect(listenerB).toHaveBeenCalledTimes(1);
  });

  it("calculates sample rate in Hz over the last second based on frame timestamps", () => {
    const stream = new PedalStream();

    // Ingest 501 frames spanning exactly 500,000 µs (0.5 s) at 1000 Hz
    const frames: PedalFrame[] = [];
    for (let i = 0; i <= 500; i++) {
      frames.push({ t: 1_000_000 + i * 1000, brake: 0, throttle: 0 });
    }
    stream.ingest(frames);

    // 501 frames over 500 ms = 500 / 0.5s = 1000 Hz
    expect(Math.round(stream.sampleRateHz)).toBe(1000);
  });

  it("calculates batch interval average and max in ms", () => {
    const stream = new PedalStream();

    // 1st batch at t = 100ms
    vi.setSystemTime(100);
    vi.spyOn(performance, "now").mockReturnValue(100);
    stream.ingest([{ t: 1000, brake: 0, throttle: 0 }]);

    // 2nd batch at t = 110ms (interval 10ms)
    vi.setSystemTime(110);
    vi.spyOn(performance, "now").mockReturnValue(110);
    stream.ingest([{ t: 2000, brake: 0, throttle: 0 }]);

    // 3rd batch at t = 130ms (interval 20ms)
    vi.setSystemTime(130);
    vi.spyOn(performance, "now").mockReturnValue(130);
    stream.ingest([{ t: 3000, brake: 0, throttle: 0 }]);

    const stats = stream.batchIntervalStats;
    // Intervals are 10ms and 20ms: avg = 15ms, max = 20ms
    expect(stats.avgMs).toBeCloseTo(15, 1);
    expect(stats.maxMs).toBeCloseTo(20, 1);
  });

  it("returns zero stats when stream is empty or idle for more than 1s", () => {
    const stream = new PedalStream();
    expect(stream.sampleRateHz).toBe(0);
    expect(stream.batchIntervalStats).toEqual({ avgMs: 0, maxMs: 0 });

    vi.spyOn(performance, "now").mockReturnValue(1000);
    stream.ingest([{ t: 1000, brake: 0, throttle: 0 }]);

    // Advance 1500 ms (past the 1-second retention window)
    vi.spyOn(performance, "now").mockReturnValue(2500);
    expect(stream.sampleRateHz).toBe(0);
    expect(stream.batchIntervalStats).toEqual({ avgMs: 0, maxMs: 0 });
  });

  describe("dataNowUs", () => {
    it("falls back to the UI clock before any data", () => {
      expect(new PedalStream().dataNowUs(10)).toBe(10_000);
    });

    it("extrapolates on the sample clock, not the UI clock", () => {
      const stream = new PedalStream();
      vi.spyOn(performance, "now").mockReturnValue(5_000);
      // Rust timestamps start near zero when the input thread starts.
      stream.ingest([{ t: 1_000_000, brake: 0.5, throttle: 0 }]);
      expect(stream.dataNowUs(5_000)).toBe(1_000_000);
      expect(stream.dataNowUs(5_004)).toBe(1_004_000);
    });

    it("never goes backwards when a batch arrives early", () => {
      const stream = new PedalStream();
      vi.spyOn(performance, "now").mockReturnValue(0);
      stream.ingest([{ t: 1_000_000, brake: 0, throttle: 0 }]);
      expect(stream.dataNowUs(10)).toBe(1_010_000);
      // The next batch's newest sample is older than the extrapolated time.
      stream.ingest([{ t: 1_008_000, brake: 0, throttle: 0 }]);
      expect(stream.dataNowUs(0)).toBe(1_010_000);
      expect(stream.dataNowUs(5)).toBe(1_013_000);
    });
  });
});
