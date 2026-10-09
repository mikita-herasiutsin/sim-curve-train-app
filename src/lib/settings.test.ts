import { describe, expect, it, beforeEach, vi } from "vitest";
import {
  loadGraphWindow,
  saveGraphWindow,
  clampGraphWindow,
  DEFAULT_GRAPH_WINDOW_S,
  loadAudioEnabled,
  saveAudioEnabled,
  loadAudioVolume,
  saveAudioVolume,
  clampAudioVolume,
  DEFAULT_AUDIO_ENABLED,
  DEFAULT_AUDIO_VOLUME,
  loadTheme,
  saveTheme,
  getSystemTheme,
  resolveTheme,
  applyTheme,
  toggleTheme,
  onThemeChange,
  initTheme,
  THEME_KEY,
  DEFAULT_THEME,
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

  describe("theme settings", () => {
    it("defaults to system when nothing is in localStorage", () => {
      expect(loadTheme()).toBe("system");
    });

    it("saves and loads explicit themes", () => {
      saveTheme("dark");
      expect(loadTheme()).toBe("dark");
      expect(localStorage.getItem(THEME_KEY)).toBe("dark");

      saveTheme("light");
      expect(loadTheme()).toBe("light");
      expect(localStorage.getItem(THEME_KEY)).toBe("light");
    });

    it("removes localStorage item when saved as system", () => {
      saveTheme("dark");
      expect(localStorage.getItem(THEME_KEY)).toBe("dark");

      saveTheme("system");
      expect(loadTheme()).toBe("system");
      expect(localStorage.getItem(THEME_KEY)).toBeNull();
    });

    it("handles corrupt localStorage values safely", () => {
      localStorage.setItem(THEME_KEY, "invalid-theme");
      expect(loadTheme()).toBe("system");
    });

    it("handles localStorage exceptions gracefully", () => {
      const getSpy = vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
        throw new Error("SecurityError: Access is denied");
      });
      const setSpy = vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
        throw new Error("QuotaExceededError");
      });

      expect(() => saveTheme("light")).not.toThrow();
      expect(loadTheme()).toBe(DEFAULT_THEME);

      getSpy.mockRestore();
      setSpy.mockRestore();
    });

    it("resolves system theme according to prefers-color-scheme", () => {
      const original = window.matchMedia;
      window.matchMedia = vi.fn().mockImplementation((query: string) => ({
        matches: query.includes("light"),
        media: query,
        addEventListener: vi.fn(),
        removeEventListener: vi.fn(),
      }));

      expect(getSystemTheme()).toBe("light");
      expect(resolveTheme("system")).toBe("light");
      expect(resolveTheme("dark")).toBe("dark");

      window.matchMedia = vi.fn().mockImplementation((query: string) => ({
        matches: false,
        media: query,
        addEventListener: vi.fn(),
        removeEventListener: vi.fn(),
      }));

      expect(getSystemTheme()).toBe("dark");
      expect(resolveTheme("system")).toBe("dark");
      expect(resolveTheme("light")).toBe("light");

      window.matchMedia = original;
    });

    it("applies theme to documentElement attributes and styles", () => {
      applyTheme("light");
      expect(document.documentElement.getAttribute("data-theme")).toBe("light");
      expect(document.documentElement.style.colorScheme).toBe("light");

      applyTheme("dark");
      expect(document.documentElement.getAttribute("data-theme")).toBe("dark");
      expect(document.documentElement.style.colorScheme).toBe("dark");
    });

    it("notifies listeners on saveTheme and allows unsubscription", () => {
      const listener = vi.fn();
      const unsub = onThemeChange(listener);

      saveTheme("light");
      expect(listener).toHaveBeenCalledWith("light", "light");

      saveTheme("dark");
      expect(listener).toHaveBeenCalledWith("dark", "dark");

      unsub();
      saveTheme("light");
      expect(listener).toHaveBeenCalledTimes(2);
    });

    it("toggles theme between dark and light correctly", () => {
      // Starting from dark, toggles to light
      saveTheme("dark");
      applyTheme("dark");
      const next1 = toggleTheme();
      expect(next1).toBe("light");
      expect(loadTheme()).toBe("light");
      expect(document.documentElement.getAttribute("data-theme")).toBe("light");

      // Starting from light, toggles to dark
      const next2 = toggleTheme();
      expect(next2).toBe("dark");
      expect(loadTheme()).toBe("dark");
      expect(document.documentElement.getAttribute("data-theme")).toBe("dark");
    });

    it("inits theme and cleans up listeners", () => {
      const original = window.matchMedia;
      const addListenerSpy = vi.fn();
      const removeListenerSpy = vi.fn();
      window.matchMedia = vi.fn().mockReturnValue({
        matches: false,
        media: "(prefers-color-scheme: light)",
        addEventListener: addListenerSpy,
        removeEventListener: removeListenerSpy,
      } as unknown as MediaQueryList);

      const cleanup = initTheme();
      expect(addListenerSpy).toHaveBeenCalledWith("change", expect.any(Function));

      cleanup();
      expect(removeListenerSpy).toHaveBeenCalledWith("change", expect.any(Function));

      window.matchMedia = original;
    });
  });

  describe("clampAudioVolume", () => {
    it("clamps values between 0 and 1 and handles non-finite", () => {
      expect(clampAudioVolume(-0.5)).toBe(0);
      expect(clampAudioVolume(0)).toBe(0);
      expect(clampAudioVolume(0.5)).toBe(0.5);
      expect(clampAudioVolume(1)).toBe(1);
      expect(clampAudioVolume(1.5)).toBe(1);
      expect(clampAudioVolume(NaN)).toBe(DEFAULT_AUDIO_VOLUME);
      expect(clampAudioVolume(Infinity)).toBe(DEFAULT_AUDIO_VOLUME);
      expect(clampAudioVolume(-Infinity)).toBe(DEFAULT_AUDIO_VOLUME);
    });
  });

  describe("loadAudioEnabled & saveAudioEnabled", () => {
    it("defaults to true when nothing is in localStorage", () => {
      expect(loadAudioEnabled()).toBe(DEFAULT_AUDIO_ENABLED);
    });

    it("saves and loads audio enabled state", () => {
      saveAudioEnabled(false);
      expect(loadAudioEnabled()).toBe(false);

      saveAudioEnabled(true);
      expect(loadAudioEnabled()).toBe(true);
    });

    it("handles corrupt localStorage values safely", () => {
      localStorage.setItem("sct:audio_enabled", "invalid-value");
      expect(loadAudioEnabled()).toBe(false); // only "true" parses to true
    });

    it("handles localStorage exceptions gracefully", () => {
      const getSpy = vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
        throw new Error("SecurityError");
      });
      const setSpy = vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
        throw new Error("QuotaExceededError");
      });

      expect(() => saveAudioEnabled(false)).not.toThrow();
      expect(loadAudioEnabled()).toBe(DEFAULT_AUDIO_ENABLED);

      getSpy.mockRestore();
      setSpy.mockRestore();
    });
  });

  describe("loadAudioVolume & saveAudioVolume", () => {
    it("defaults to 0.2 when nothing is in localStorage", () => {
      expect(loadAudioVolume()).toBe(DEFAULT_AUDIO_VOLUME);
    });

    it("saves and loads a valid volume value", () => {
      saveAudioVolume(0.7);
      expect(loadAudioVolume()).toBe(0.7);
    });

    it("clamps values when saved and loaded", () => {
      saveAudioVolume(-0.3);
      expect(loadAudioVolume()).toBe(0);

      saveAudioVolume(1.8);
      expect(loadAudioVolume()).toBe(1);

      localStorage.setItem("sct:audio_volume", "1.5");
      expect(loadAudioVolume()).toBe(1);

      localStorage.setItem("sct:audio_volume", "-0.5");
      expect(loadAudioVolume()).toBe(0);
    });

    it("handles NaN safely on save and load", () => {
      saveAudioVolume(NaN);
      expect(loadAudioVolume()).toBe(DEFAULT_AUDIO_VOLUME);

      localStorage.setItem("sct:audio_volume", "NaN");
      expect(loadAudioVolume()).toBe(DEFAULT_AUDIO_VOLUME);

      localStorage.setItem("sct:audio_volume", "not-a-number");
      expect(loadAudioVolume()).toBe(DEFAULT_AUDIO_VOLUME);
    });

    it("handles localStorage exceptions gracefully", () => {
      const getSpy = vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
        throw new Error("SecurityError");
      });
      const setSpy = vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
        throw new Error("QuotaExceededError");
      });

      expect(() => saveAudioVolume(0.5)).not.toThrow();
      expect(loadAudioVolume()).toBe(DEFAULT_AUDIO_VOLUME);

      getSpy.mockRestore();
      setSpy.mockRestore();
    });
  });
});
