import { invoke, Channel } from "@tauri-apps/api/core";

export interface BaseDrill {
  id: string;
  name: string;
  pedal: "brake" | "throttle" | "clutch";
  reps: number;
  leadInMs: number;
  tolerance: number;
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

export interface TraceScore {
  kind: "trace";
  total: number;
  grade: string;
  rmse: number;
}

export type RepScore = HoldScore | TraceScore;

export interface SetSummary {
  repsCount: number;
  repTotals: number[];
  bestTotal: number;
  avgTotal: number;
  consistency: number;
}

export type DrillEvent =
  | { event: "countdownStarted"; rep: number; startUs: number; endsUs: number }
  | { event: "repStarted"; rep: number; startUs: number }
  | { event: "repScored"; rep: number; score: RepScore }
  | { event: "repFailed"; rep: number }
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

export type RunState = "idle" | "countdown" | "active" | "scored" | "finished";

/** What the drill screen shows for a run, advanced by [`applyDrillEvent`]. */
export interface RunView {
  runState: RunState;
  currentRep: number;
  countdownEndsUs: number;
  lastScore: RepScore | null;
  /** Set summary; `null` after a finished set means no rep was scored. */
  summary: SetSummary | null;
}

export const IDLE_VIEW: RunView = {
  runState: "idle",
  currentRep: 0,
  countdownEndsUs: 0,
  lastScore: null,
  summary: null,
};

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
      };
    case "repStarted":
      return { ...view, runState: "active", currentRep: e.rep, lastScore: null };
    case "repScored":
      return { ...view, runState: "scored", currentRep: e.rep, lastScore: e.score };
    case "repFailed":
      return { ...view, runState: "scored", currentRep: e.rep, lastScore: null };
    case "setFinished":
      return { ...view, runState: "finished", summary: e.summary };
  }
}
