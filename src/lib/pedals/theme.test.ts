import { describe, expect, it, beforeEach } from "vitest";
import { readThemeColors, DEFAULT_DARK_THEME_COLORS, DEFAULT_LIGHT_THEME_COLORS } from "./theme";

function relativeLuminance(hex: string): number {
  const r = parseInt(hex.slice(1, 3), 16) / 255;
  const g = parseInt(hex.slice(3, 5), 16) / 255;
  const b = parseInt(hex.slice(5, 7), 16) / 255;
  const [R, G, B] = [r, g, b].map((v) =>
    v <= 0.04045 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4),
  );
  return 0.2126 * R + 0.7152 * G + 0.0722 * B;
}

function contrastRatio(hex1: string, hex2: string): number {
  const l1 = relativeLuminance(hex1);
  const l2 = relativeLuminance(hex2);
  const [bright, dark] = l1 > l2 ? [l1, l2] : [l2, l1];
  return (bright + 0.05) / (dark + 0.05);
}

describe("theme colors and WCAG AA contrast", () => {
  beforeEach(() => {
    document.documentElement.removeAttribute("data-theme");
  });

  it("reads dark theme colors by default", () => {
    document.documentElement.setAttribute("data-theme", "dark");
    const colors = readThemeColors();
    expect(colors.bg).toBe(DEFAULT_DARK_THEME_COLORS.bg);
    expect(colors.surface).toBe(DEFAULT_DARK_THEME_COLORS.surface);
    expect(colors.brake).toBe(DEFAULT_DARK_THEME_COLORS.brake);
    expect(colors.throttle).toBe(DEFAULT_DARK_THEME_COLORS.throttle);
  });

  it("reads light theme colors when data-theme is light", () => {
    document.documentElement.setAttribute("data-theme", "light");
    const colors = readThemeColors();
    expect(colors.bg).toBe(DEFAULT_LIGHT_THEME_COLORS.bg);
    expect(colors.surface).toBe(DEFAULT_LIGHT_THEME_COLORS.surface);
    expect(colors.brake).toBe(DEFAULT_LIGHT_THEME_COLORS.brake);
    expect(colors.throttle).toBe(DEFAULT_LIGHT_THEME_COLORS.throttle);
  });

  it("dark theme satisfies WCAG AA contrast (≥ 4.5:1 for normal text, ≥ 3.0:1 for large/graphical)", () => {
    const t = DEFAULT_DARK_THEME_COLORS;

    // Normal text against surface and bg
    expect(contrastRatio(t.text, t.surface)).toBeGreaterThanOrEqual(4.5);
    expect(contrastRatio(t.text, t.bg)).toBeGreaterThanOrEqual(4.5);
    expect(contrastRatio(t.text, t.surfaceRaised)).toBeGreaterThanOrEqual(4.5);

    // Muted text
    expect(contrastRatio(t.textMuted, t.surface)).toBeGreaterThanOrEqual(4.5);
    expect(contrastRatio(t.textMuted, t.bg)).toBeGreaterThanOrEqual(4.5);

    // Pedal colors against surface (bars and graphs)
    expect(contrastRatio(t.brake, t.surface)).toBeGreaterThanOrEqual(3.0);
    expect(contrastRatio(t.throttle, t.surface)).toBeGreaterThanOrEqual(3.0);
  });

  it("light theme satisfies WCAG AA contrast (≥ 4.5:1 for normal text, ≥ 3.0:1 for large/graphical)", () => {
    const t = DEFAULT_LIGHT_THEME_COLORS;

    // Normal text against surface and bg
    expect(contrastRatio(t.text, t.surface)).toBeGreaterThanOrEqual(4.5);
    expect(contrastRatio(t.text, t.bg)).toBeGreaterThanOrEqual(4.5);
    expect(contrastRatio(t.text, t.surfaceRaised)).toBeGreaterThanOrEqual(4.5);

    // Muted text
    expect(contrastRatio(t.textMuted, t.surface)).toBeGreaterThanOrEqual(4.5);
    expect(contrastRatio(t.textMuted, t.bg)).toBeGreaterThanOrEqual(4.5);

    // Pedal colors against surface (bars and graphs)
    expect(contrastRatio(t.brake, t.surface)).toBeGreaterThanOrEqual(3.0);
    expect(contrastRatio(t.throttle, t.surface)).toBeGreaterThanOrEqual(3.0);
  });
});
