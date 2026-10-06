import { invoke } from "@tauri-apps/api/core";

export type PedalName = "throttle" | "brake" | "clutch";

/** Wizard order: brake first (the pedal that matters most), clutch can be skipped. */
export const WIZARD_STEPS: readonly { pedal: PedalName; optional: boolean }[] = [
  { pedal: "brake", optional: false },
  { pedal: "throttle", optional: false },
  { pedal: "clutch", optional: true },
];

/** Mirrors `sct_core::axis_detect::Detection`. */
export type Detection =
  | { kind: "noMovement" }
  | { kind: "ambiguous"; candidates: number[] }
  | { kind: "axis"; index: number; rest: number; min: number; max: number };

/** An axis picked for a pedal, with the raw range seen while picking it. */
export interface AxisAssignment {
  axis: number;
  rest: number;
  min: number;
  max: number;
}

export type Assignments = Partial<Record<PedalName, AxisAssignment>>;

/** Asks Rust which axis moved since `sinceUs` (sample clock), ignoring `exclude`. */
export function detectAxis(sinceUs: number, exclude: number[]): Promise<Detection> {
  return invoke<Detection>("detect_axis", { sinceUs, exclude });
}

/** True once the pedal is back within 10% of its travel from the rest position. */
export function isBackAtRest(range: Pick<AxisAssignment, "rest" | "min" | "max">, raw: number) {
  return Math.abs(raw - range.rest) <= (range.max - range.min) * 0.1;
}

/** Axes already taken by other pedals. */
export function assignedAxes(assignments: Assignments, except?: PedalName): number[] {
  return Object.entries(assignments)
    .filter(([pedal, a]) => pedal !== except && a !== undefined)
    .map(([, a]) => (a as AxisAssignment).axis);
}

export function pedalLabel(pedal: PedalName): string {
  return pedal.charAt(0).toUpperCase() + pedal.slice(1);
}

/** Returns `assignments` without `pedal`. */
export function unassign(assignments: Assignments, pedal: PedalName): Assignments {
  const next = { ...assignments };
  delete next[pedal];
  return next;
}
