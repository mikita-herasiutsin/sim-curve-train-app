import { describe, expect, it } from "vitest";
import { mockIPC } from "@tauri-apps/api/mocks";
import { saveAttempt, listAttempts, bestTotal, type Attempt, type NewAttempt } from "./attempts";

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
