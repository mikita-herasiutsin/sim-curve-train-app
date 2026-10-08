export interface AppThemeColors {
  bg: string;
  surface: string;
  surfaceRaised: string;
  border: string;
  text: string;
  textMuted: string;
  brake: string;
  throttle: string;
  accent: string;
}

export const DEFAULT_DARK_THEME_COLORS: AppThemeColors = {
  bg: "#0e1116",
  surface: "#151a21",
  surfaceRaised: "#1c222b",
  border: "#2a313c",
  text: "#e8ecf1",
  textMuted: "#8b95a3",
  brake: "#ff4d5e",
  throttle: "#2ee59d",
  accent: "#5b8cff",
};

export const DEFAULT_LIGHT_THEME_COLORS: AppThemeColors = {
  bg: "#f4f6f8",
  surface: "#ffffff",
  surfaceRaised: "#e9ecef",
  border: "#cbd2db",
  text: "#111827",
  textMuted: "#526071",
  brake: "#d92338",
  throttle: "#09794d",
  accent: "#1e5dd8",
};

/**
 * Reads CSS variables from an element via getComputedStyle,
 * with fallbacks matching src/app.css for both dark and light themes.
 */
export function readThemeColors(element?: Element | null): AppThemeColors {
  const isLight =
    typeof document !== "undefined" &&
    (document.documentElement.getAttribute("data-theme") === "light" ||
      (!document.documentElement.getAttribute("data-theme") &&
        typeof window !== "undefined" &&
        typeof window.matchMedia === "function" &&
        window.matchMedia("(prefers-color-scheme: light)").matches));

  const defaults = isLight ? DEFAULT_LIGHT_THEME_COLORS : DEFAULT_DARK_THEME_COLORS;

  const targetEl = element ?? (typeof document !== "undefined" ? document.documentElement : null);

  if (typeof window === "undefined" || !targetEl) {
    return { ...defaults };
  }

  const style = window.getComputedStyle(targetEl);
  const get = (prop: string, fallback: string): string => {
    const val = style.getPropertyValue(prop).trim();
    return val.length > 0 ? val : fallback;
  };

  return {
    bg: get("--bg", defaults.bg),
    surface: get("--surface", defaults.surface),
    surfaceRaised: get("--surface-raised", defaults.surfaceRaised),
    border: get("--border", defaults.border),
    text: get("--text", defaults.text),
    textMuted: get("--text-muted", defaults.textMuted),
    brake: get("--brake", defaults.brake),
    throttle: get("--throttle", defaults.throttle),
    accent: get("--accent", defaults.accent),
  };
}
