import { describe, expect, it } from "vitest";
import { FrameHistory } from "./history";
import type { PedalFrame } from "./types";

describe("FrameHistory", () => {
  it("handles the empty case correctly", () => {
    const history = new FrameHistory(10);
    expect(history.length).toBe(0);
    expect(history.latest()).toBeUndefined();

    const seen: PedalFrame[] = [];
    history.forEachSince(0, (f) => seen.push(f));
    expect(seen).toHaveLength(0);
  });

  it("stores and retrieves frames up to capacity", () => {
    const history = new FrameHistory(5);
    history.push({ t: 100, brake: 0.1, throttle: 0.2 });
    history.push({ t: 200, brake: 0.3, throttle: 0.4, clutch: 0.5 });

    expect(history.length).toBe(2);
    expect(history.latest()).toEqual({
      t: 200,
      brake: 0.30000001192092896,
      throttle: 0.4000000059604645,
      clutch: 0.5,
    });

    const seen: PedalFrame[] = [];
    history.forEachSince(150, (f) => seen.push(f));
    expect(seen).toHaveLength(1);
    expect(seen[0].t).toBe(200);
  });

  it("clears all frames properly", () => {
    const history = new FrameHistory(5);
    history.pushMany([
      { t: 10, brake: 0.1, throttle: 0.1 },
      { t: 20, brake: 0.2, throttle: 0.2 },
    ]);
    expect(history.length).toBe(2);

    history.clear();
    expect(history.length).toBe(0);
    expect(history.latest()).toBeUndefined();

    const seen: PedalFrame[] = [];
    history.forEachSince(0, (f) => seen.push(f));
    expect(seen).toHaveLength(0);
  });

  it("correctly handles wraparound and retains newest capacity elements", () => {
    const capacity = 4;
    const history = new FrameHistory(capacity);

    // Push 7 items: t = 10, 20, 30, 40, 50, 60, 70
    for (let i = 1; i <= 7; i++) {
      history.push({ t: i * 10, brake: i * 0.1, throttle: 1 - i * 0.1 });
    }

    expect(history.length).toBe(4);
    expect(history.latest()?.t).toBe(70);

    // Oldest to newest elements stored should be t = 40, 50, 60, 70
    const allSeen: number[] = [];
    history.forEachSince(0, (f) => allSeen.push(f.t));
    expect(allSeen).toEqual([40, 50, 60, 70]);
  });

  it("iterates across the wrap point with forEachSince using binary search", () => {
    const capacity = 5;
    const history = new FrameHistory(capacity);

    // Fill completely: [10, 20, 30, 40, 50]
    for (let i = 1; i <= 5; i++) {
      history.push({ t: i * 10, brake: 0, throttle: 0 });
    }

    // Overwrite first 2:
    // Physical layout will be:
    // idx 0: t=60
    // idx 1: t=70
    // idx 2: t=30
    // idx 3: t=40
    // idx 4: t=50
    // Logical order: 30, 40, 50, 60, 70
    history.push({ t: 60, brake: 0.6, throttle: 0 });
    history.push({ t: 70, brake: 0.7, throttle: 0 });

    expect(history.length).toBe(5);
    expect(history.latest()?.t).toBe(70);

    // Query starting before the wrap boundary in logical order (e.g. t >= 45)
    // Should return 50 (phys 4), 60 (phys 0), 70 (phys 1)
    const seen: number[] = [];
    history.forEachSince(45, (f) => seen.push(f.t));
    expect(seen).toEqual([50, 60, 70]);

    // Query starting exactly at the wrap point (e.g. t >= 60)
    const seenAtWrap: number[] = [];
    history.forEachSince(60, (f) => seenAtWrap.push(f.t));
    expect(seenAtWrap).toEqual([60, 70]);

    // Query for everything
    const seenAll: number[] = [];
    history.forEachSince(10, (f) => seenAll.push(f.t));
    expect(seenAll).toEqual([30, 40, 50, 60, 70]);

    // Query for timestamp greater than any stored frame
    const seenNone: number[] = [];
    history.forEachSince(100, (f) => seenNone.push(f.t));
    expect(seenNone).toEqual([]);
  });

  it("handles pushMany with arrays larger than capacity", () => {
    const history = new FrameHistory(3);
    const frames: PedalFrame[] = [
      { t: 1, brake: 0, throttle: 0 },
      { t: 2, brake: 0, throttle: 0 },
      { t: 3, brake: 0, throttle: 0 },
      { t: 4, brake: 0, throttle: 0 },
      { t: 5, brake: 0, throttle: 0 },
    ];
    history.pushMany(frames);
    expect(history.length).toBe(3);
    expect(history.latest()?.t).toBe(5);

    const seen: number[] = [];
    history.forEachSince(0, (f) => seen.push(f.t));
    expect(seen).toEqual([3, 4, 5]);
  });
});
