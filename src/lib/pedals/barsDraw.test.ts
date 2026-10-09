import { describe, expect, it, vi } from "vitest";
import { bandColor, computeBarsLayout, drawPedalBars } from "./barsDraw";
import type { AppThemeColors } from "./theme";
import { TraceCurve } from "./traceDraw";

describe("barsDraw", () => {
  it("computes reasonable column layout for pedal bars", () => {
    const layout = computeBarsLayout(200, 400);

    expect(layout.brake.width).toBeGreaterThan(0);
    expect(layout.throttle.width).toBeGreaterThan(0);
    expect(layout.throttle.x).toBeGreaterThan(layout.brake.x);
    expect(layout.brake.barHeight).toBeGreaterThan(50);
  });

  it("draws pedal bars without error using mock context", () => {
    const mockCtx = {
      clearRect: vi.fn(),
      fillRect: vi.fn(),
      beginPath: vi.fn(),
      moveTo: vi.fn(),
      lineTo: vi.fn(),
      arcTo: vi.fn(),
      closePath: vi.fn(),
      fill: vi.fn(),
      stroke: vi.fn(),
      fillText: vi.fn(),
      save: vi.fn(),
      restore: vi.fn(),
      clip: vi.fn(),
    } as unknown as CanvasRenderingContext2D;

    const theme: AppThemeColors = {
      bg: "#000",
      surface: "#111",
      surfaceRaised: "#222",
      border: "#333",
      text: "#fff",
      textMuted: "#888",
      brake: "#f00",
      throttle: "#0f0",
      accent: "#00f",
    };

    expect(() => {
      drawPedalBars(mockCtx, 150, 300, 0.75, 0.3, theme);
    }).not.toThrow();

    expect(mockCtx.clearRect).toHaveBeenCalled();
    expect(mockCtx.fillText).toHaveBeenCalledWith("75%", expect.any(Number), expect.any(Number));
    expect(mockCtx.fillText).toHaveBeenCalledWith("30%", expect.any(Number), expect.any(Number));
  });

  describe("target band colour", () => {
    const theme: AppThemeColors = {
      bg: "#000",
      surface: "#111",
      surfaceRaised: "#222",
      border: "#333",
      text: "#fff",
      textMuted: "#888",
      brake: "#f00000",
      throttle: "#00f000",
      accent: "#0000f0",
    };

    it("is green inside and red outside", () => {
      expect(bandColor(true, theme)).toBe("#00f00050");
      expect(bandColor(false, theme)).toBe("#f0000050");
    });

    it.each([
      ["brake", 0.3, "#00f00050"],
      ["throttle", 0.3, "#00f00050"],
      ["brake", 0.9, "#f0000050"],
      ["throttle", 0.9, "#f0000050"],
    ] as const)("paints the %s band by whether %s is inside it", (pedal, value, expected) => {
      const fills: string[] = [];
      const ctx = {
        fillStyle: "",
        strokeStyle: "",
        clearRect: vi.fn(),
        fillRect: vi.fn(function (this: { fillStyle: string }) {
          fills.push(this.fillStyle);
        }),
        beginPath: vi.fn(),
        moveTo: vi.fn(),
        lineTo: vi.fn(),
        arcTo: vi.fn(),
        closePath: vi.fn(),
        fill: vi.fn(),
        stroke: vi.fn(),
        fillText: vi.fn(),
        save: vi.fn(),
        restore: vi.fn(),
        clip: vi.fn(),
      } as unknown as CanvasRenderingContext2D;
      const brake = pedal === "brake" ? value : 0;
      const throttle = pedal === "throttle" ? value : 0;
      drawPedalBars(ctx, 150, 300, brake, throttle, theme, pedal, 0.3, 0.05);
      expect(fills).toContain(expected);
      expect(fills).not.toContain(expected === "#00f00050" ? "#f0000050" : "#00f00050");
    });

    it("evaluates in-band with targetRange on a trace envelope", () => {
      const points: [number, number][] = [
        [0, 0],
        [150, 100],
        [300, 95],
        [600, 70],
        [1000, 40],
        [1500, 0],
      ];
      const curve = new TraceCurve(points);
      const targetRange = curve.envelopeAt(75);

      const fillsWithRange: string[] = [];
      const ctxWithRange = {
        fillStyle: "",
        strokeStyle: "",
        clearRect: vi.fn(),
        fillRect: vi.fn(function (this: { fillStyle: string }) {
          fillsWithRange.push(this.fillStyle);
        }),
        beginPath: vi.fn(),
        moveTo: vi.fn(),
        lineTo: vi.fn(),
        arcTo: vi.fn(),
        closePath: vi.fn(),
        fill: vi.fn(),
        stroke: vi.fn(),
        fillText: vi.fn(),
        save: vi.fn(),
        restore: vi.fn(),
        clip: vi.fn(),
      } as unknown as CanvasRenderingContext2D;

      drawPedalBars(ctxWithRange, 150, 300, 0.0, 0, theme, "brake", 0.5, 0.02, 0, targetRange);
      expect(fillsWithRange).toContain("#00f00050");
      expect(fillsWithRange).not.toContain("#f0000050");

      const fillsWithoutRange: string[] = [];
      const ctxWithoutRange = {
        fillStyle: "",
        strokeStyle: "",
        clearRect: vi.fn(),
        fillRect: vi.fn(function (this: { fillStyle: string }) {
          fillsWithoutRange.push(this.fillStyle);
        }),
        beginPath: vi.fn(),
        moveTo: vi.fn(),
        lineTo: vi.fn(),
        arcTo: vi.fn(),
        closePath: vi.fn(),
        fill: vi.fn(),
        stroke: vi.fn(),
        fillText: vi.fn(),
        save: vi.fn(),
        restore: vi.fn(),
        clip: vi.fn(),
      } as unknown as CanvasRenderingContext2D;

      drawPedalBars(ctxWithoutRange, 150, 300, 0.0, 0, theme, "brake", 0.5, 0.02, 0);
      expect(fillsWithoutRange).toContain("#f0000050");
      expect(fillsWithoutRange).not.toContain("#00f00050");
    });
  });
});
