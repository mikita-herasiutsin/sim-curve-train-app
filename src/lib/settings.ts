const GRAPH_WINDOW_KEY = "sct:graph_window_seconds";
export const THEME_KEY = "sct:theme";

export const MIN_GRAPH_WINDOW_S = 3;
export const MAX_GRAPH_WINDOW_S = 10;
export const DEFAULT_GRAPH_WINDOW_S = 5;

export type ThemeSetting = "system" | "light" | "dark";
export type ResolvedTheme = "light" | "dark";

export const DEFAULT_THEME: ThemeSetting = "system";

export function clampGraphWindow(seconds: number): number {
  if (Number.isNaN(seconds) || !Number.isFinite(seconds)) {
    return DEFAULT_GRAPH_WINDOW_S;
  }
  return Math.max(MIN_GRAPH_WINDOW_S, Math.min(MAX_GRAPH_WINDOW_S, Math.round(seconds)));
}

export function loadGraphWindow(): number {
  try {
    if (typeof localStorage === "undefined") {
      return DEFAULT_GRAPH_WINDOW_S;
    }
    const stored = localStorage.getItem(GRAPH_WINDOW_KEY);
    if (stored === null) {
      return DEFAULT_GRAPH_WINDOW_S;
    }
    const parsed = Number(stored);
    return clampGraphWindow(parsed);
  } catch {
    return DEFAULT_GRAPH_WINDOW_S;
  }
}

export function saveGraphWindow(seconds: number): void {
  try {
    if (typeof localStorage === "undefined") {
      return;
    }
    const clamped = clampGraphWindow(seconds);
    localStorage.setItem(GRAPH_WINDOW_KEY, String(clamped));
  } catch {
    // Ignore storage quota or access errors
  }
}

export function loadTheme(): ThemeSetting {
  try {
    if (typeof localStorage === "undefined") {
      return DEFAULT_THEME;
    }
    const stored = localStorage.getItem(THEME_KEY);
    if (stored === "light" || stored === "dark") {
      return stored;
    }
    return DEFAULT_THEME;
  } catch {
    return DEFAULT_THEME;
  }
}

export function getSystemTheme(): ResolvedTheme {
  if (
    typeof window !== "undefined" &&
    typeof window.matchMedia === "function" &&
    window.matchMedia("(prefers-color-scheme: light)").matches
  ) {
    return "light";
  }
  return "dark";
}

export function resolveTheme(setting: ThemeSetting): ResolvedTheme {
  if (setting === "system") {
    return getSystemTheme();
  }
  return setting;
}

export function applyTheme(setting: ThemeSetting): ResolvedTheme {
  const resolved = resolveTheme(setting);
  if (typeof document !== "undefined") {
    document.documentElement.setAttribute("data-theme", resolved);
    document.documentElement.style.colorScheme = resolved;
  }
  return resolved;
}

export type ThemeListener = (resolved: ResolvedTheme, setting: ThemeSetting) => void;
const themeListeners = new Set<ThemeListener>();

export function onThemeChange(listener: ThemeListener): () => void {
  themeListeners.add(listener);
  return () => {
    themeListeners.delete(listener);
  };
}

function notifyThemeListeners(setting: ThemeSetting): void {
  const resolved = resolveTheme(setting);
  for (const listener of themeListeners) {
    listener(resolved, setting);
  }
}

export function saveTheme(theme: ThemeSetting): void {
  try {
    if (typeof localStorage !== "undefined") {
      if (theme === "system") {
        localStorage.removeItem(THEME_KEY);
      } else {
        localStorage.setItem(THEME_KEY, theme);
      }
    }
  } catch {
    // Ignore storage quota or access errors
  }
  applyTheme(theme);
  notifyThemeListeners(theme);
}

export function toggleTheme(): ResolvedTheme {
  const currentSetting = loadTheme();
  const currentResolved = resolveTheme(currentSetting);
  const nextTheme: ResolvedTheme = currentResolved === "dark" ? "light" : "dark";
  saveTheme(nextTheme); // also applies it
  return nextTheme;
}

/**
 * Initializes theme listeners (for OS prefers-color-scheme changes and cross-window storage events)
 * and applies the initial theme. Returns a cleanup function.
 */
export function initTheme(): () => void {
  applyTheme(loadTheme());

  let mediaQuery: MediaQueryList | null = null;
  const handleMediaChange = () => {
    if (loadTheme() === "system") {
      applyTheme("system");
      notifyThemeListeners("system");
    }
  };

  if (typeof window !== "undefined" && typeof window.matchMedia === "function") {
    mediaQuery = window.matchMedia("(prefers-color-scheme: light)");
    mediaQuery.addEventListener?.("change", handleMediaChange);
  }

  const handleStorage = (event: StorageEvent) => {
    if (event.key === THEME_KEY) {
      const nextSetting = loadTheme();
      applyTheme(nextSetting);
      notifyThemeListeners(nextSetting);
    }
  };

  if (typeof window !== "undefined") {
    window.addEventListener?.("storage", handleStorage);
  }

  return () => {
    if (mediaQuery && mediaQuery.removeEventListener) {
      mediaQuery.removeEventListener("change", handleMediaChange);
    }
    if (typeof window !== "undefined" && window.removeEventListener) {
      window.removeEventListener("storage", handleStorage);
    }
  };
}

const AUDIO_ENABLED_KEY = "sct:audio_enabled";
const AUDIO_VOLUME_KEY = "sct:audio_volume";
export const DEFAULT_AUDIO_ENABLED = true;
export const DEFAULT_AUDIO_VOLUME = 0.2;

export function clampAudioVolume(volume: number): number {
  if (Number.isNaN(volume) || !Number.isFinite(volume)) {
    return DEFAULT_AUDIO_VOLUME;
  }
  return Math.max(0, Math.min(1, volume));
}

export function loadAudioEnabled(): boolean {
  try {
    if (typeof localStorage === "undefined") return DEFAULT_AUDIO_ENABLED;
    const stored = localStorage.getItem(AUDIO_ENABLED_KEY);
    return stored !== null ? stored === "true" : DEFAULT_AUDIO_ENABLED;
  } catch {
    return DEFAULT_AUDIO_ENABLED;
  }
}

export function saveAudioEnabled(enabled: boolean): void {
  try {
    if (typeof localStorage === "undefined") return;
    localStorage.setItem(AUDIO_ENABLED_KEY, String(enabled));
  } catch {
    // Ignore error
  }
}

export function loadAudioVolume(): number {
  try {
    if (typeof localStorage === "undefined") return DEFAULT_AUDIO_VOLUME;
    const stored = localStorage.getItem(AUDIO_VOLUME_KEY);
    if (stored === null) return DEFAULT_AUDIO_VOLUME;
    const parsed = Number(stored);
    return clampAudioVolume(parsed);
  } catch {
    return DEFAULT_AUDIO_VOLUME;
  }
}

export function saveAudioVolume(volume: number): void {
  try {
    if (typeof localStorage === "undefined") return;
    const clamped = clampAudioVolume(volume);
    localStorage.setItem(AUDIO_VOLUME_KEY, String(clamped));
  } catch {
    // Ignore error
  }
}

const LAST_PRESET_KEY = "sct:last_preset";

export function loadLastPreset(): string | null {
  try {
    if (typeof localStorage === "undefined") return null;
    const stored = localStorage.getItem(LAST_PRESET_KEY);
    if (!stored) return null;
    return stored;
  } catch {
    return null;
  }
}

export function saveLastPreset(id: string): void {
  try {
    if (typeof localStorage === "undefined") return;
    localStorage.setItem(LAST_PRESET_KEY, id);
  } catch {
    // Ignore storage quota or access errors
  }
}

export type TraceViewMode = "playhead" | "ghost";
export const DEFAULT_TRACE_VIEW: TraceViewMode = "playhead";
const TRACE_VIEW_KEY = "sct:trace_view";

export function loadTraceView(): TraceViewMode {
  try {
    if (typeof localStorage === "undefined") return DEFAULT_TRACE_VIEW;
    const stored = localStorage.getItem(TRACE_VIEW_KEY);
    if (stored === "playhead" || stored === "ghost") {
      return stored;
    }
    return DEFAULT_TRACE_VIEW;
  } catch {
    return DEFAULT_TRACE_VIEW;
  }
}

export function saveTraceView(mode: TraceViewMode): void {
  try {
    if (typeof localStorage === "undefined") return;
    localStorage.setItem(TRACE_VIEW_KEY, mode);
  } catch {
    // Ignore storage quota or access errors
  }
}
