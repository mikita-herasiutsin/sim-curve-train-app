import { invoke } from "@tauri-apps/api/core";
import type { AxisAssignment, Assignments, PedalName } from "$lib/wizard";

/** Mirrors `sct_core::calibration::AxisCalibration`. Deadzones are fractions of travel. */
export interface AxisCalibration {
  min: number;
  max: number;
  invert: boolean;
  deadzoneLow: number;
  deadzoneHigh: number;
}

/** Mirrors `sct_core::profile::PedalAxis`. */
export interface PedalAxis {
  axis: number;
  calibration: AxisCalibration;
}

/** Mirrors `sct_core::profile::DeviceProfile`. */
export type DeviceProfile = Record<PedalName, PedalAxis | null>;

/** Default deadzone at each end: absorbs sensor noise so released reads 0% and full reads 100%. */
export const DEFAULT_DEADZONE = 0.02;

export const EMPTY_PROFILE: DeviceProfile = { throttle: null, brake: null, clutch: null };

export function loadProfile(deviceId: number): Promise<DeviceProfile | null> {
  return invoke<DeviceProfile | null>("load_profile", { deviceId });
}

export function saveProfile(deviceId: number, profile: DeviceProfile): Promise<void> {
  return invoke<void>("save_profile", { deviceId, profile });
}

export function resetProfile(deviceId: number): Promise<boolean> {
  return invoke<boolean>("reset_profile", { deviceId });
}

/** Ids of connected devices that have a saved profile. */
export function profiledDevices(): Promise<number[]> {
  return invoke<number[]>("profiled_devices");
}

/** Builds a calibration in Rust from a sweep (raw min/max) and the released position. */
export function calibrate(
  range: Pick<AxisAssignment, "min" | "max" | "rest">,
  deadzoneLow = DEFAULT_DEADZONE,
  deadzoneHigh = DEFAULT_DEADZONE,
): Promise<AxisCalibration> {
  return invoke<AxisCalibration>("calibrate", { ...range, deadzoneLow, deadzoneHigh });
}

/** Turns wizard assignments into a profile, keeping deadzones already set for the same axis. */
export async function profileFromAssignments(
  assignments: Assignments,
  previous: DeviceProfile | null,
): Promise<DeviceProfile> {
  const profile: DeviceProfile = { ...EMPTY_PROFILE };
  for (const pedal of Object.keys(profile) as PedalName[]) {
    const a = assignments[pedal];
    if (!a) continue;
    const old = previous?.[pedal];
    const keep = old && old.axis === a.axis ? old.calibration : null;
    profile[pedal] = {
      axis: a.axis,
      calibration: await calibrate(a, keep?.deadzoneLow, keep?.deadzoneHigh),
    };
  }
  return profile;
}

/** The wizard view of a saved profile (rest is the end the pedal reads 0% at). */
export function assignmentsFromProfile(profile: DeviceProfile): Assignments {
  const assignments: Assignments = {};
  for (const [pedal, p] of Object.entries(profile) as [PedalName, PedalAxis | null][]) {
    if (!p) continue;
    const { min, max, invert } = p.calibration;
    assignments[pedal] = { axis: p.axis, min, max, rest: invert ? max : min };
  }
  return assignments;
}
