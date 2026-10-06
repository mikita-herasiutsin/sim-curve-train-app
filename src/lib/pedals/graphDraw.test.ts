import { describe, expect, it, vi } from "vitest";
import {
  ColumnDecimator,
  computeHorizontalGridLines,
  computeVerticalGridLines,
  buildSignalSegments,
  drawGraph,
} from "./graphDraw";
import type { AppThemeColors } from "./theme";

describe("graphDraw pure math and decimation", () => {
  describe("ColumnDecimator", () => {
    it("accumulates min and max values per column", () => {
      const decimator = new ColumnDecimator(10);
      decimator.reset();

      expect(decimator.hasData[3]).toBe(0);

      // Accumulate multiple frames into column 3
      decimator.accumulate(3, 0.2, 0.4);
      decimator.accumulate(3, 0.9, 0.1);
      decimator.accumulate(3, 0.1, 0.8);

      expect(decimator.hasData[3]).toBe(1);
      expect(decimator.brakeMin[3]).toBeCloseTo(0.1);
      expect(decimator.brakeMax[3]).toBeCloseTo(0.9);
      expect(decimator.throttleMin[3]).toBeCloseTo(0.1);
      expect(decimator.throttleMax[3]).toBeCloseTo(0.8);

      // Ignored out of bounds columns
      decimator.accumulate(-1, 0.5, 0.5);
      decimator.accumulate(10, 0.5, 0.5);
    });

    it("resets and resizes correctly", () => {
      const decimator = new ColumnDecimator(5);
      decimator.accumulate(2, 0.5, 0.5);
      expect(decimator.hasData[2]).toBe(1);

      decimator.reset();
      expect(decimator.hasData[2]).toBe(0);

      decimator.resize(20);
      expect(decimator.numCols).toBe(20);
      expect(decimator.hasData.length).toBe(20);
    });
  });

  describe("computeHorizontalGridLines", () => {
    it("returns 25%, 50%, and 75% grid lines", () => {
      const lines = computeHorizontalGridLines(200, 0, 0);
      expect(lines).toHaveLength(3);
      expect(lines.map((l) => l.fraction)).toEqual([0.25, 0.5, 0.75]);
      expect(lines.map((l) => l.label)).toEqual(["25%", "50%", "75%"]);
      // At fraction 0.5 in 200px: y = 100
      expect(lines[1].y).toBe(100);
    });
  });

  describe("computeVerticalGridLines", () => {
    it("generates lines at 1-second intervals scrolling with time", () => {
      const nowUs = 5_500_000; // 5.5s
      const windowUs = 3_000_000; // 3s window (range: 2.5s to 5.5s)
      const width = 300;

      const lines = computeVerticalGridLines(nowUs, windowUs, width);
      // Seconds in range: 5.0s, 4.0s, 3.0s
      expect(lines).toHaveLength(3);
      expect(lines[0].label).toBe("now"); // 5.0s is within 0.5s of 5.5s
      expect(lines[0].x).toBeGreaterThan(0);
      expect(lines[0].x).toBeLessThan(width);
    });
  });

  describe("buildSignalSegments", () => {
    it("preserves min and max spikes in column decimation", () => {
      const decimator = new ColumnDecimator(5);
      decimator.reset();

      decimator.accumulate(0, 0.0, 0);
      decimator.accumulate(1, 0.0, 0);
      // Spike at column 2: both 0.0 and 1.0 occur in this column
      decimator.accumulate(2, 0.0, 0);
      decimator.accumulate(2, 1.0, 0);
      decimator.accumulate(3, 0.0, 0);

      const segments = buildSignalSegments(
        decimator.hasData,
        decimator.brakeMin,
        decimator.brakeMax,
        5,
        100,
        0,
        0,
      );

      expect(segments).toHaveLength(1);
      const points = segments[0];

      // Points must contain column 2 with both high (y=0) and low (y=100) values
      const col2Points = points.filter((p) => p.x === 2);
      expect(col2Points).toHaveLength(2);
      expect(col2Points.some((p) => p.y === 0)).toBe(true);
      expect(col2Points.some((p) => p.y === 100)).toBe(true);
    });

    it("splits segments when column gap exceeds threshold", () => {
      const decimator = new ColumnDecimator(10);
      decimator.reset();
      decimator.accumulate(0, 0.1, 0);
      decimator.accumulate(1, 0.1, 0);
      // Gap from 1 to 8 (> 4 cols)
      decimator.accumulate(8, 0.1, 0);

      const segments = buildSignalSegments(
        decimator.hasData,
        decimator.brakeMin,
        decimator.brakeMax,
        10,
        100,
        0,
        0,
      );
      expect(segments).toHaveLength(2);
    });
  });

  describe("drawGraph", () => {
    it("executes drawing commands on canvas 2D context without error", () => {
      const mockCtx = {
        clearRect: vi.fn(),
        fillRect: vi.fn(),
        beginPath: vi.fn(),
        moveTo: vi.fn(),
        lineTo: vi.fn(),
        stroke: vi.fn(),
        fillText: vi.fn(),
        setLineDash: vi.fn(),
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

      const decimator = new ColumnDecimator(50);
      decimator.reset();
      decimator.accumulate(10, 0.5, 0.2);

      expect(() => {
        drawGraph(mockCtx, 100, 100, decimator, 2_000_000, 2_000_000, theme);
      }).not.toThrow();

      expect(mockCtx.clearRect).toHaveBeenCalled();
      expect(mockCtx.fillRect).toHaveBeenCalled();
      expect(mockCtx.stroke).toHaveBeenCalled();
    });
  });
});
