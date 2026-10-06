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

/**
 * Reads CSS variables from an element via getComputedStyle,
 * with fallbacks matching src/app.css.
 */
export function readThemeColors(element: Element): AppThemeColors {
  if (typeof window === "undefined" || !element) {
    return {
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
  }

  const style = window.getComputedStyle(element);
  const get = (prop: string, fallback: string): string => {
    const val = style.getPropertyValue(prop).trim();
    return val.length > 0 ? val : fallback;
  };

  return {
    bg: get("--bg", "#0e1116"),
    surface: get("--surface", "#151a21"),
    surfaceRaised: get("--surface-raised", "#1c222b"),
    border: get("--border", "#2a313c"),
    text: get("--text", "#e8ecf1"),
    textMuted: get("--text-muted", "#8b95a3"),
    brake: get("--brake", "#ff4d5e"),
    throttle: get("--throttle", "#2ee59d"),
    accent: get("--accent", "#5b8cff"),
  };
}
