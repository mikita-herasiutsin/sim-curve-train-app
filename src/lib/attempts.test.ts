import { describe, expect, it } from "vitest";
import { mockIPC } from "@tauri-apps/api/mocks";
import {
  saveAttempt,
  listAttempts,
  bestTotal,
  buildAttempt,
  type Attempt,
  type NewAttempt,
} from "./attempts";
import type { TraceScore } from "./drill";

describe("attempts IPC wrappers", () => {
  it("saveAttempt invokes save_attempt command and returns attempt ID", async () => {
    const newAttempt: NewAttempt = {
      drillId: "hold-brake-70",
      presetId: "gt3",
      pedal: "brake",
      startedAt: "2026-10-07T12:00:00Z",
      aborted: false,
      best: 94.0,
      average: 91.25,
      consistency: 88.0,
      reps: [
        {
          repIndex: 0,
          total: 88.5,
          accuracy: 90.0,
          timing: 85.0,
          smoothness: 89.0,
          timeInBand: 0.92,
          rmse: 0.015,
          overshoot: 0.02,
          timeToBandMs: 180.0,
          jitter: 0.005,
        },
      ],
    };

    let calledCommand = "";
    let calledPayload: unknown = null;

    mockIPC((cmd, payload) => {
      calledCommand = cmd;
      calledPayload = payload;
      return 42;
    });

    const id = await saveAttempt(newAttempt);
    expect(calledCommand).toBe("save_attempt");
    expect(calledPayload).toEqual({ attempt: newAttempt });
    expect(id).toBe(42);
  });

  it("listAttempts invokes list_attempts with default and custom limits", async () => {
    const mockAttempt: Attempt = {
      id: 1,
      drillId: "hold-brake-70",
      presetId: "gt3",
      pedal: "brake",
      startedAt: "2026-10-07T12:00:00Z",
      aborted: false,
      best: 94.0,
      average: 91.25,
      consistency: 88.0,
      reps: [],
    };

    const calls: { cmd: string; payload: unknown }[] = [];

    mockIPC((cmd, payload) => {
      calls.push({ cmd, payload });
      return [mockAttempt];
    });

    // Default limit
    const results1 = await listAttempts("hold-brake-70");
    expect(results1).toEqual([mockAttempt]);
    expect(calls[0]).toEqual({
      cmd: "list_attempts",
      payload: { drillId: "hold-brake-70", limit: 10 },
    });

    // Custom limit
    const results2 = await listAttempts("hold-brake-70", 5);
    expect(results2).toEqual([mockAttempt]);
    expect(calls[1]).toEqual({
      cmd: "list_attempts",
      payload: { drillId: "hold-brake-70", limit: 5 },
    });
  });

  it("bestTotal invokes best_total and returns score or null", async () => {
    let response: number | null = 96.5;

    mockIPC((cmd, payload) => {
      expect(cmd).toBe("best_total");
      expect(payload).toEqual({ drillId: "hold-brake-70" });
      return response;
    });

    await expect(bestTotal("hold-brake-70")).resolves.toBe(96.5);

    response = null;
    await expect(bestTotal("hold-brake-70")).resolves.toBeNull();
  });
});

describe("buildAttempt", () => {
  const trace: TraceScore = {
    kind: "trace",
    total: 77,
    grade: "C",
    accuracy: 80,
    timing: 70,
    smoothness: 75,
    lagMs: 120,
    timeInBand: 0.6,
    rmse: 0.05,
    overshoot: 0.03,
    ldljUser: -6.1,
    ldljTarget: -5.2,
  };
  const set = {
    drillId: "trail-brake",
    presetId: "gt3",
    pedal: "brake" as const,
    startedAt: "2026-10-09T12:00:00.000Z",
    aborted: true,
  };

  it("stores trace sub-scores and sorts reps by index", () => {
    const attempt = buildAttempt({
      ...set,
      summary: null,
      scored: [
        { rep: 2, score: { ...trace, total: 60 } },
        { rep: 0, score: trace },
      ],
    });
    expect(attempt.reps.map((r) => [r.repIndex, r.total])).toEqual([
      [0, 77],
      [2, 60],
    ]);
    expect(attempt.reps[0]).toEqual({
      repIndex: 0,
      total: 77,
      accuracy: 80,
      timing: 70,
      smoothness: 75,
      timeInBand: 0.6,
      rmse: 0.05,
      overshoot: 0.03,
      lagMs: 120,
      ldljUser: -6.1,
      ldljTarget: -5.2,
    });
  });

  it("leaves the summary fields null without a summary", () => {
    const attempt = buildAttempt({ ...set, summary: null, scored: [] });
    expect(attempt).toEqual({ ...set, best: null, average: null, consistency: null, reps: [] });
  });

  it("copies best, average and consistency from the summary", () => {
    const attempt = buildAttempt({
      ...set,
      summary: { repTotals: [90], best: 90, average: 90, grade: "A", consistency: null, stdDev: 0 },
      scored: [],
    });
    expect(attempt).toMatchObject({ best: 90, average: 90, consistency: null });
  });
});
