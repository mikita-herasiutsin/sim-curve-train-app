import { describe, expect, it, beforeEach, vi } from "vitest";
import {
  loadGraphWindow,
  saveGraphWindow,
  clampGraphWindow,
  DEFAULT_GRAPH_WINDOW_S,
} from "./settings";

describe("settings", () => {
  beforeEach(() => {
    localStorage.clear();
  });

  describe("clampGraphWindow", () => {
    it("clamps values between 3 and 10 and handles non-finite", () => {
      expect(clampGraphWindow(1)).toBe(3);
      expect(clampGraphWindow(2.9)).toBe(3);
      expect(clampGraphWindow(5)).toBe(5);
      expect(clampGraphWindow(8)).toBe(8);
      expect(clampGraphWindow(12)).toBe(10);
      expect(clampGraphWindow(NaN)).toBe(DEFAULT_GRAPH_WINDOW_S);
      expect(clampGraphWindow(Infinity)).toBe(DEFAULT_GRAPH_WINDOW_S);
    });
  });

  describe("loadGraphWindow & saveGraphWindow", () => {
    it("defaults to 5 when nothing is in localStorage", () => {
      expect(loadGraphWindow()).toBe(5);
    });

    it("saves and loads a valid window value", () => {
      saveGraphWindow(7);
      expect(loadGraphWindow()).toBe(7);
    });

    it("clamps stored values when saved", () => {
      saveGraphWindow(1);
      expect(loadGraphWindow()).toBe(3);

      saveGraphWindow(15);
      expect(loadGraphWindow()).toBe(10);
    });

    it("handles corrupt localStorage values safely", () => {
      localStorage.setItem("sct:graph_window_seconds", "not-a-number");
      expect(loadGraphWindow()).toBe(5);
    });

    it("handles localStorage exceptions gracefully", () => {
      const getSpy = vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
        throw new Error("SecurityError: Access is denied");
      });
      const setSpy = vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
        throw new Error("QuotaExceededError");
      });

      expect(() => saveGraphWindow(6)).not.toThrow();
      expect(loadGraphWindow()).toBe(DEFAULT_GRAPH_WINDOW_S);

      getSpy.mockRestore();
      setSpy.mockRestore();
    });
  });
});
