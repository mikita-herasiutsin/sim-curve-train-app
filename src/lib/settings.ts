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
  saveTheme(nextTheme);
  applyTheme(nextTheme);
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
