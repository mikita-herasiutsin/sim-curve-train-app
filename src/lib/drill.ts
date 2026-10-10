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
  /** Brake drills only: throttle held before each rep; the rep start is the cue to lift (SCT-037). */
  throttleLeadIn?: { level: number; holdMs: number };
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

export interface Preset {
  schemaVersion: number;
  id: string;
  name: string;
  description: string;
  drills: Drill[];
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
}

export type DrillEvent =
  | { event: "countdownStarted"; rep: number; startUs: number; endsUs: number }
  | { event: "throttleWait"; rep: number; sinceUs: number }
  | { event: "throttleHoldStarted"; rep: number; startUs: number; endsUs: number }
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
): Promise<void> {
  const channel = new Channel<DrillEvent>();
  channel.onmessage = onEvent;
  return invoke<void>("start_drill_run", { token, presetId, drillId, onEvent: channel });
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
  /** Sample-clock µs when the lift cue comes; 0 while waiting. */
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
        throttleHoldEndsUs: 0,
      };
    case "throttleWait":
      return {
        ...view,
        runState: "throttle",
        currentRep: e.rep,
        throttleHoldEndsUs: 0,
      };
    case "throttleHoldStarted":
      return {
        ...view,
        runState: "throttle",
        currentRep: e.rep,
        throttleHoldEndsUs: e.endsUs,
      };
    case "repStarted":
      return {
        ...view,
        runState: "active",
        currentRep: e.rep,
        repStartUs: e.startUs,
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
