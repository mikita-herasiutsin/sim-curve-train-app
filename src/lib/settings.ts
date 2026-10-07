const GRAPH_WINDOW_KEY = "sct:graph_window_seconds";

export const MIN_GRAPH_WINDOW_S = 3;
export const MAX_GRAPH_WINDOW_S = 10;
export const DEFAULT_GRAPH_WINDOW_S = 5;

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
