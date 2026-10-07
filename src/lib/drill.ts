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
  drill: Drill,
  onEvent: (e: DrillEvent) => void,
): Promise<void> {
  const channel = new Channel<DrillEvent>();
  channel.onmessage = onEvent;
  return invoke<void>("start_drill_run", { token, drill, onEvent: channel });
}

export async function abortDrillRun(token: number): Promise<void> {
  return invoke<void>("abort_drill_run", { token });
}
