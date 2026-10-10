import { invoke, Channel } from "@tauri-apps/api/core";

export interface BaseDrill {
  id: string;
  name: string;
  pedal: "brake" | "throttle" | "clutch";
  reps: number;
  leadInMs: number;
  /** Band half-width in percentage points; omitted means the D-17 default. */
  tolerance?: number;
  /** Decimal places (0 or 1) when showing percentages; omitted means 0. */
  decimals?: number;
  /** Brake drills only: throttle held before each rep. LIFT comes liftMs before the brake point; the rep starts at the brake point (SCT-037). */
  throttleLeadIn?: { level: number; holdMs: number; liftMs?: number };
}

export interface HoldDrill extends BaseDrill {
  type: "hold";
  target: number;
  holdMs: number;
}

export interface TraceDrill extends BaseDrill {
  type: "trace";
  points: [number, number][];
}

export type Drill = HoldDrill | TraceDrill;

/** Default lead-in lift window in ms when omitted (SCT-037). */
export const DEFAULT_LIFT_MS = 300;

/** The lift window in ms for a throttle lead-in. */
export function liftWindowMs(leadIn: { liftMs?: number }): number {
  return leadIn.liftMs ?? DEFAULT_LIFT_MS;
}

/** Tolerance used when a drill omits it (D-17), matching `DEFAULT_TOLERANCE` in Rust. */
export const DEFAULT_TOLERANCE = 10;

/** The drill's tolerance band half-width in percentage points. */
export function toleranceOf(drill: Drill): number {
  return drill.tolerance ?? DEFAULT_TOLERANCE;
}

/**
 * Whether the drill screen can run it. Hold and trace drills work; clutch has no bar.
 */
export function isPlayable(drill: Drill): boolean {
  return drill.pedal !== "clutch";
}

export function playableDrills(preset: Preset): Drill[] {
  return preset.drills.filter(isPlayable);
}

/** One step of a preset's warm-up (D-19, D-25): a drill id and the warm-up's own rep count. */
export interface WarmUpStep {
  drill: string;
  reps: number;
}

export interface WarmUp {
  steps: WarmUpStep[];
}

export interface Preset {
  schemaVersion: number;
  id: string;
  name: string;
  description: string;
  drills: Drill[];
  warmUp?: WarmUp;
}

export interface HoldScore {
  kind: "hold";
  total: number;
  grade: string;
  accuracy: number;
  timing: number;
  smoothness: number;
  timeInBand: number;
  rmse: number;
  timeToBandMs: number | null;
  overshoot: number;
  jitter: number;
}

// Mirrors sct_core::trace_scoring::TraceScore (camelCase).
export interface TraceScore {
  kind: "trace";
  total: number;
  grade: string;
  accuracy: number;
  timing: number;
  smoothness: number;
  lagMs: number;
  timeInBand: number;
  rmse: number;
  overshoot: number;
  ldljUser: number;
  ldljTarget: number;
}

export type RepScore = HoldScore | TraceScore;

// Mirrors sct_core::set_summary::SetSummary (camelCase).
export interface SetSummary {
  repTotals: number[];
  best: number;
  average: number;
  grade: string;
  // null with fewer than two scored reps.
  consistency: number | null;
  stdDev: number;
}

// Mirrors sct_core::drill_engine::Overlap (camelCase); only lead-in drills report it.
export interface Overlap {
  overlapMs: number;
  peakThrottle: number;
  coastMs?: number;
}

export type DrillEvent =
  | { event: "countdownStarted"; rep: number; startUs: number; endsUs: number }
  | { event: "throttleWait"; rep: number; sinceUs: number }
  | { event: "throttleHoldStarted"; rep: number; startUs: number; liftUs: number; endsUs: number }
  | { event: "repStarted"; rep: number; startUs: number }
  | { event: "repScored"; rep: number; score: RepScore; overlap?: Overlap }
  | { event: "repFailed"; rep: number; overlap?: Overlap }
  | { event: "setFinished"; summary: SetSummary | null };

export async function listPresets(): Promise<Preset[]> {
  return invoke<Preset[]>("list_presets");
}

export async function startDrillRun(
  token: number,
  presetId: string,
  drillId: string,
  onEvent: (e: DrillEvent) => void,
  warmUp = false,
): Promise<void> {
  const channel = new Channel<DrillEvent>();
  channel.onmessage = onEvent;
  return invoke<void>("start_drill_run", { token, presetId, drillId, warmUp, onEvent: channel });
}

export async function abortDrillRun(token: number): Promise<void> {
  return invoke<void>("abort_drill_run", { token });
}

export type RunState = "idle" | "countdown" | "throttle" | "active" | "scored" | "finished";

/** The outcome of one rep, numbered like the engine's `rep` (zero-based). `total` is null for a failed rep. */
export interface RepResult {
  rep: number;
  total: number | null;
}

/** What the drill screen shows for a run, advanced by [`applyDrillEvent`]. */
export interface RunView {
  runState: RunState;
  currentRep: number;
  countdownEndsUs: number;
  /** Sample-clock start of the active rep (µs); 0 until a rep starts. */
  repStartUs: number;
  /** Sample-clock µs of the LIFT cue; 0 while waiting. */
  throttleLiftUs: number;
  /** Sample-clock µs of the brake point; 0 while waiting. */
  throttleHoldEndsUs: number;
  lastScore: RepScore | null;
  lastOverlap: Overlap | null;
  /** Set summary; `null` after a finished set means no rep was scored. */
  summary: SetSummary | null;
  /** Every rep that ended, failed ones included. */
  reps: RepResult[];
}

export const IDLE_VIEW: RunView = {
  runState: "idle",
  currentRep: 0,
  countdownEndsUs: 0,
  repStartUs: 0,
  throttleLiftUs: 0,
  throttleHoldEndsUs: 0,
  lastScore: null,
  lastOverlap: null,
  summary: null,
  reps: [],
};

function withRep(reps: RepResult[], result: RepResult): RepResult[] {
  return [...reps.filter((r) => r.rep !== result.rep), result].sort((a, b) => a.rep - b.rep);
}

/**
 * Advances the view by one engine event. A rep's score stays visible through the rest
 * countdown that follows it (both events arrive together) and is cleared when the next rep
 * starts. After an abort the engine still sends `setFinished`, which ends the set the same way.
 */
export function applyDrillEvent(view: RunView, e: DrillEvent): RunView {
  switch (e.event) {
    case "countdownStarted":
      return {
        ...view,
        runState: "countdown",
        currentRep: e.rep,
        countdownEndsUs: e.endsUs,
        throttleLiftUs: 0,
        throttleHoldEndsUs: 0,
      };
    case "throttleWait":
      return {
        ...view,
        runState: "throttle",
        currentRep: e.rep,
        throttleLiftUs: 0,
        throttleHoldEndsUs: 0,
      };
    case "throttleHoldStarted":
      return {
        ...view,
        runState: "throttle",
        currentRep: e.rep,
        throttleLiftUs: e.liftUs,
        throttleHoldEndsUs: e.endsUs,
      };
    case "repStarted":
      return {
        ...view,
        runState: "active",
        currentRep: e.rep,
        repStartUs: e.startUs,
        throttleLiftUs: 0,
        throttleHoldEndsUs: 0,
        lastScore: null,
        lastOverlap: null,
      };
    case "repScored":
      return {
        ...view,
        runState: "scored",
        currentRep: e.rep,
        lastScore: e.score,
        lastOverlap: e.overlap ?? null,
        reps: withRep(view.reps, { rep: e.rep, total: e.score.total }),
      };
    case "repFailed":
      return {
        ...view,
        runState: "scored",
        currentRep: e.rep,
        lastScore: null,
        lastOverlap: e.overlap ?? null,
        reps: withRep(view.reps, { rep: e.rep, total: null }),
      };
    case "setFinished":
      return { ...view, runState: "finished", summary: e.summary };
  }
}
