import { describe, expect, it } from "vitest";
import { mockIPC } from "@tauri-apps/api/mocks";
import {
  listAttempts,
  bestTotal,
  saveWarmUpRun,
  listWarmUpRuns,
  type Attempt,
  type NewWarmUpRun,
  type WarmUpRun,
} from "./attempts";

describe("attempts IPC wrappers", () => {
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

  it("saveWarmUpRun invokes save_warm_up_run command and returns run ID", async () => {
    const newRun: NewWarmUpRun = {
      presetId: "gt3",
      startedAt: "2026-10-10T12:00:00Z",
      steps: [
        {
          drillId: "hold-brake-70",
          reps: 5,
          skipped: false,
          attemptId: 42,
          score: 88.5,
        },
      ],
    };

    let calledCommand = "";
    let calledPayload: unknown = null;

    mockIPC((cmd, payload) => {
      calledCommand = cmd;
      calledPayload = payload;
      return 10;
    });

    const id = await saveWarmUpRun(newRun);
    expect(calledCommand).toBe("save_warm_up_run");
    expect(calledPayload).toEqual({ run: newRun });
    expect(id).toBe(10);
  });

  it("listWarmUpRuns invokes list_warm_up_runs with default and custom limits", async () => {
    const mockRun: WarmUpRun = {
      id: 10,
      presetId: "gt3",
      startedAt: "2026-10-10T12:00:00Z",
      score: 88.5,
      steps: [],
    };

    const calls: { cmd: string; payload: unknown }[] = [];

    mockIPC((cmd, payload) => {
      calls.push({ cmd, payload });
      return [mockRun];
    });

    // Default limit
    const results1 = await listWarmUpRuns("gt3");
    expect(results1).toEqual([mockRun]);
    expect(calls[0]).toEqual({
      cmd: "list_warm_up_runs",
      payload: { presetId: "gt3", limit: 10 },
    });

    // Custom limit
    const results2 = await listWarmUpRuns("gt3", 5);
    expect(results2).toEqual([mockRun]);
    expect(calls[1]).toEqual({
      cmd: "list_warm_up_runs",
      payload: { presetId: "gt3", limit: 5 },
    });
  });
});
