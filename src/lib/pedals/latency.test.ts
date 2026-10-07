import { describe, expect, it } from "vitest";
import { createCrossingDetector } from "./latency";

describe("createCrossingDetector", () => {
  it("fires exactly once when value crosses above threshold and re-arms when below rearm", () => {
    const detector = createCrossingDetector(0.5, 0.4);

    expect(detector(0.1)).toBe(false);
    expect(detector(0.3)).toBe(false);
    expect(detector(0.49)).toBe(false);

    // Rising edge crossing
    expect(detector(0.51)).toBe(true);

    // Continuing to rise or stay high should NOT fire
    expect(detector(0.7)).toBe(false);
    expect(detector(1.0)).toBe(false);
    expect(detector(0.8)).toBe(false);

    // Drops slightly but stays above rearm (0.4) -> remains disarmed
    expect(detector(0.45)).toBe(false);
    expect(detector(0.6)).toBe(false);

    // Drops below rearm
    expect(detector(0.35)).toBe(false);

    // Now re-armed! Next crossing above 0.5 must fire again
    expect(detector(0.48)).toBe(false);
    expect(detector(0.55)).toBe(true);

    // Immediately subsequent call is false
    expect(detector(0.55)).toBe(false);
  });

  it("handles exact threshold and exact rearm boundaries", () => {
    const detector = createCrossingDetector(0.5, 0.4);

    expect(detector(0.4)).toBe(false);
    expect(detector(0.5)).toBe(true);
    expect(detector(0.5)).toBe(false);

    // Drops to exactly rearm
    expect(detector(0.4)).toBe(false);

    // Should fire again at exact threshold
    expect(detector(0.5)).toBe(true);
  });

  it("throws if rearm is greater than or equal to threshold", () => {
    expect(() => createCrossingDetector(0.5, 0.5)).toThrow();
    expect(() => createCrossingDetector(0.4, 0.5)).toThrow();
  });
});
