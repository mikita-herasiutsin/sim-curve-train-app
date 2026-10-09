import { invoke } from "@tauri-apps/api/core";
import type { PedalName } from "$lib/wizard";
import type { RepScore, SetSummary } from "$lib/drill";

/** Mirrors `sct_core::attempts::AttemptRep`. Detailed scores and sub-scores for a single drill repetition. */
export interface AttemptRep {
  /** 0-based index of this repetition within the set. */
  repIndex: number;
  /** Overall composite repetition score in `0.0..=100.0`. */
  total: number;
  /** Accuracy sub-score in `0.0..=100.0`. */
  accuracy: number;
  /** Timing / reaction latency sub-score in `0.0..=100.0`. */
  timing: number;
  /** Smoothness sub-score in `0.0..=100.0`. */
  smoothness: number;
  /** Time-weighted fraction of settled/drill duration spent inside tolerance band (`0.0..=1.0`). */
  timeInBand?: number | null;
  /** Root-mean-square error from target position. */
  rmse?: number | null;
  /** Maximum excursion / overshoot beyond target maximum or tolerance band. */
  overshoot?: number | null;
  /** Hold-drill specific: time in milliseconds until first entering the tolerance band. */
  timeToBandMs?: number | null;
  /** Hold-drill specific: RMS jitter deviation from 50 ms moving average. */
  jitter?: number | null;
  /** Trace-drill specific: estimated driver reaction lag in milliseconds. */
  lagMs?: number | null;
  /** Trace-drill specific: log dimensionless jerk of the user's filtered trace. */
  ldljUser?: number | null;
  /** Trace-drill specific: log dimensionless jerk of the reference target curve. */
  ldljTarget?: number | null;
}

/** Data required to persist a new drill set attempt. Mirrors `sct_core::attempts::NewAttempt`. */
export interface NewAttempt {
  /** Identifier of the drill executed. */
  drillId: string;
  /** Identifier of the parent preset containing the drill. */
  presetId: string;
  /** Pedal targeted by the drill. */
  pedal: PedalName;
  /** UTC timestamp when the set started (formatted as an ISO 8601 string). */
  startedAt: string;
  /** Whether the user aborted the set before finishing all scheduled reps. */
  aborted: boolean;
  /** Highest rep total score achieved in this set. */
  best?: number | null;
  /** Arithmetic mean of completed rep total scores. */
  average?: number | null;
  /** Consistency sub-score in `0.0..=100.0` based on rep score spread. */
  consistency?: number | null;
  /** Completed repetition scores. */
  reps: AttemptRep[];
}

/** A persisted drill set attempt with its assigned unique ID. Mirrors `sct_core::attempts::Attempt`. */
export interface Attempt extends NewAttempt {
  /** Database primary key for this attempt. */
  id: number;
}

/** A scored rep of a set, numbered like the engine's zero-based `rep`. */
export interface ScoredRep {
  rep: number;
  score: RepScore;
}

/** Maps one engine rep score to the stored rep. Fields the score kind lacks are left out. */
export function attemptRep(rep: number, score: RepScore): AttemptRep {
  const common = {
    repIndex: rep,
    total: score.total,
    accuracy: score.accuracy,
    timing: score.timing,
    smoothness: score.smoothness,
    timeInBand: score.timeInBand,
    rmse: score.rmse,
    overshoot: score.overshoot,
  };
  return score.kind === "hold"
    ? { ...common, timeToBandMs: score.timeToBandMs, jitter: score.jitter }
    : { ...common, lagMs: score.lagMs, ldljUser: score.ldljUser, ldljTarget: score.ldljTarget };
}

/**
 * Builds the record for a finished or aborted set. Failed reps have no scores, so only scored
 * reps are stored; the summary fields are null when no rep was scored.
 */
export function buildAttempt(set: {
  drillId: string;
  presetId: string;
  pedal: PedalName;
  startedAt: string;
  aborted: boolean;
  summary: SetSummary | null;
  scored: ScoredRep[];
}): NewAttempt {
  return {
    drillId: set.drillId,
    presetId: set.presetId,
    pedal: set.pedal,
    startedAt: set.startedAt,
    aborted: set.aborted,
    best: set.summary?.best ?? null,
    average: set.summary?.average ?? null,
    consistency: set.summary?.consistency ?? null,
    reps: [...set.scored].sort((a, b) => a.rep - b.rep).map((r) => attemptRep(r.rep, r.score)),
  };
}

/** Saves a completed or aborted attempt set and each of its reps, returning the assigned attempt ID. */
export function saveAttempt(attempt: NewAttempt): Promise<number> {
  return invoke<number>("save_attempt", { attempt });
}

/** Lists the most recent recorded attempts for a drill, ordered from newest to oldest. */
export function listAttempts(drillId: string, limit = 10): Promise<Attempt[]> {
  return invoke<Attempt[]>("list_attempts", { drillId, limit });
}

/** Returns the highest total score recorded for a drill, or `null` if no attempts exist. */
export function bestTotal(drillId: string): Promise<number | null> {
  return invoke<number | null>("best_total", { drillId });
}
