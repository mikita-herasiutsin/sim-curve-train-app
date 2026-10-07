import { afterEach, describe, expect, it } from "vitest";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { isBackAtRest, assignedAxes, unassign, pedalLabel } from "./wizard";
import { profileFromAssignments, assignmentsFromProfile, DEFAULT_DEADZONE } from "./profile";
import type { Assignments } from "./wizard";
import type { DeviceProfile } from "./profile";

afterEach(() => {
  clearMocks();
});

function mockCalibrate() {
  mockIPC((cmd, args) => {
    if (cmd === "calibrate") {
      const a = args as {
        min: number;
        max: number;
        rest: number;
        deadzoneLow: number;
        deadzoneHigh: number;
      };
      return {
        min: a.min,
        max: a.max,
        invert: Math.abs(a.rest - a.max) < Math.abs(a.rest - a.min),
        deadzoneLow: a.deadzoneLow,
        deadzoneHigh: a.deadzoneHigh,
      };
    }
    throw new Error(cmd);
  });
}

describe("isBackAtRest", () => {
  it("returns true within 10% of travel from rest", () => {
    const range = { rest: 0, min: 0, max: 100 };
    expect(isBackAtRest(range, 0)).toBe(true);
    expect(isBackAtRest(range, 10)).toBe(true);
    expect(isBackAtRest(range, 11)).toBe(false);
  });

  it("returns false beyond 10% of travel", () => {
    const range = { rest: 0, min: 0, max: 100 };
    expect(isBackAtRest(range, 50)).toBe(false);
  });

  it("works for inverted where rest equals max", () => {
    const range = { rest: 100, min: 0, max: 100 };
    expect(isBackAtRest(range, 100)).toBe(true);
    expect(isBackAtRest(range, 90)).toBe(true);
    expect(isBackAtRest(range, 89)).toBe(false);
  });
});

describe("assignedAxes", () => {
  it("lists other pedals' axes and excludes the given pedal", () => {
    const assignments: Assignments = {
      throttle: { axis: 1, rest: 0, min: 0, max: 100 },
      brake: { axis: 2, rest: 100, min: 0, max: 100 },
      clutch: { axis: 3, rest: 0, min: 0, max: 100 },
    };
    expect(assignedAxes(assignments)).toEqual([1, 2, 3]);
    expect(assignedAxes(assignments, "brake")).toEqual([1, 3]);
    expect(assignedAxes(assignments, "throttle")).toEqual([2, 3]);
  });

  it("ignores missing pedals", () => {
    const assignments: Assignments = {
      throttle: { axis: 1, rest: 0, min: 0, max: 100 },
      clutch: { axis: 3, rest: 0, min: 0, max: 100 },
    };
    expect(assignedAxes(assignments)).toEqual([1, 3]);
  });
});

describe("unassign", () => {
  it("removes one pedal and does not mutate input", () => {
    const assignments: Assignments = {
      throttle: { axis: 1, rest: 0, min: 0, max: 100 },
      brake: { axis: 2, rest: 100, min: 0, max: 100 },
    };
    const next = unassign(assignments, "brake");
    expect(next).toEqual({
      throttle: { axis: 1, rest: 0, min: 0, max: 100 },
    });
    expect(assignments.brake).toBeDefined();
  });
});

describe("pedalLabel", () => {
  it("capitalizes pedal names", () => {
    expect(pedalLabel("throttle")).toBe("Throttle");
    expect(pedalLabel("brake")).toBe("Brake");
    expect(pedalLabel("clutch")).toBe("Clutch");
  });
});

describe("assignmentsFromProfile", () => {
  it("maps rest to min when invert false and max when invert true, skipping null pedals", () => {
    const profile: DeviceProfile = {
      throttle: {
        axis: 0,
        calibration: { min: 0, max: 100, invert: false, deadzoneLow: 0.02, deadzoneHigh: 0.02 },
      },
      brake: {
        axis: 1,
        calibration: { min: 0, max: 100, invert: true, deadzoneLow: 0.02, deadzoneHigh: 0.02 },
      },
      clutch: null,
    };
    const assignments = assignmentsFromProfile(profile);
    expect(assignments.throttle).toEqual({ axis: 0, min: 0, max: 100, rest: 0 });
    expect(assignments.brake).toEqual({ axis: 1, min: 0, max: 100, rest: 100 });
    expect(assignments.clutch).toBeUndefined();
  });
});

describe("profileFromAssignments", () => {
  it("uses DEFAULT_DEADZONE for new pedals", async () => {
    mockCalibrate();
    const assignments: Assignments = {
      throttle: { axis: 0, rest: 0, min: 0, max: 100 },
    };
    const profile = await profileFromAssignments(assignments, null);
    expect(profile.throttle).toEqual({
      axis: 0,
      calibration: {
        min: 0,
        max: 100,
        invert: false,
        deadzoneLow: DEFAULT_DEADZONE,
        deadzoneHigh: DEFAULT_DEADZONE,
      },
    });
    expect(profile.brake).toBeNull();
    expect(profile.clutch).toBeNull();
  });

  it("keeps previous deadzones for the same axis and defaults when the axis changed", async () => {
    mockCalibrate();
    const previous: DeviceProfile = {
      throttle: {
        axis: 0,
        calibration: { min: 0, max: 100, invert: false, deadzoneLow: 0.05, deadzoneHigh: 0.07 },
      },
      brake: {
        axis: 2,
        calibration: { min: 0, max: 100, invert: true, deadzoneLow: 0.09, deadzoneHigh: 0.11 },
      },
      clutch: null,
    };
    const assignments: Assignments = {
      throttle: { axis: 0, rest: 0, min: 0, max: 100 },
      brake: { axis: 1, rest: 100, min: 0, max: 100 },
    };
    const profile = await profileFromAssignments(assignments, previous);
    expect(profile.clutch).toBeNull();
    expect(profile.throttle).toEqual({
      axis: 0,
      calibration: { min: 0, max: 100, invert: false, deadzoneLow: 0.05, deadzoneHigh: 0.07 },
    });
    expect(profile.brake).toEqual({
      axis: 1,
      calibration: {
        min: 0,
        max: 100,
        invert: true,
        deadzoneLow: DEFAULT_DEADZONE,
        deadzoneHigh: DEFAULT_DEADZONE,
      },
    });
  });
});
