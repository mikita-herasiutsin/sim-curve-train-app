import { describe, expect, it, vi, beforeEach, afterEach } from "vitest";
import { startMockSource, generatePedalSample } from "./mockSource";
import { PedalStream } from "./stream";

describe("mockSource", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  describe("generatePedalSample", () => {
    it("follows the expected brake cycle", () => {
      // At start (0 ms): brake is 0
      const start = generatePedalSample(0);
      expect(start.brake).toBeCloseTo(0);

      // At peak of brake ramp (150 ms): brake is 0.90
      const peak = generatePedalSample(150);
      expect(peak.brake).toBeCloseTo(0.9);

      // Mid trail-off (150 + 750 = 900 ms): brake is 0.45
      const midTrail = generatePedalSample(900);
      expect(midTrail.brake).toBeCloseTo(0.45);
      expect(midTrail.throttle).toBe(0);

      // End of trail-off (1650 ms): brake is 0
      const endTrail = generatePedalSample(1650);
      expect(endTrail.brake).toBeCloseTo(0);

      // During rest (2000 ms): brake is 0, throttle is rising or full
      const rest = generatePedalSample(2100);
      expect(rest.brake).toBe(0);
      expect(rest.throttle).toBe(1.0);
    });
  });

  describe("startMockSource", () => {
    it("delivers frames at 1000 Hz every 8 ms to the stream and stops cleanly", () => {
      const stream = new PedalStream();
      let fakeTime = 10_000;
      vi.setSystemTime(fakeTime);
      vi.spyOn(performance, "now").mockImplementation(() => fakeTime);

      const stop = startMockSource(stream);

      expect(stream.history.length).toBe(0);

      // Advance by 8 ms
      fakeTime += 8;
      vi.advanceTimersByTime(8);

      expect(stream.history.length).toBe(8);
      const latest = stream.history.latest();
      expect(latest).toBeDefined();
      expect(latest?.t).toBe(fakeTime * 1000);

      // Advance another 16 ms (2 batches)
      fakeTime += 16;
      vi.advanceTimersByTime(16);
      expect(stream.history.length).toBe(24);

      // Stop source
      stop();

      // Advancing further should not deliver more frames
      fakeTime += 100;
      vi.advanceTimersByTime(100);
      expect(stream.history.length).toBe(24);
    });
  });
});
