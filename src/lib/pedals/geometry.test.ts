import { describe, expect, it } from "vitest";
import { getBarFillHeight, getPercentLabel, formatPercent, getGraphX, getGraphY } from "./geometry";

describe("geometry helpers", () => {
  describe("getBarFillHeight", () => {
    it("computes fill height accurately and clamps values", () => {
      expect(getBarFillHeight(0, 200)).toBe(0);
      expect(getBarFillHeight(1, 200)).toBe(200);
      expect(getBarFillHeight(0.5, 200)).toBe(100);
      expect(getBarFillHeight(-0.2, 200)).toBe(0);
      expect(getBarFillHeight(1.5, 200)).toBe(200);
    });
  });

  describe("getPercentLabel & formatPercent", () => {
    it("returns rounded integer percentage between 0 and 100", () => {
      expect(getPercentLabel(0)).toBe(0);
      expect(getPercentLabel(0.456)).toBe(46);
      expect(getPercentLabel(1)).toBe(100);
      expect(getPercentLabel(-0.1)).toBe(0);
      expect(getPercentLabel(1.2)).toBe(100);
    });

    it("formats percentage string", () => {
      expect(formatPercent(0.72)).toBe("72%");
      expect(formatPercent(0)).toBe("0%");
      expect(formatPercent(1)).toBe("100%");
    });
  });

  describe("getGraphX", () => {
    const width = 800;
    const windowUs = 5_000_000; // 5s
    const nowUs = 10_000_000;

    it("places current timestamp at the right edge", () => {
      expect(getGraphX(nowUs, nowUs, windowUs, width)).toBe(width);
    });

    it("places timestamp windowUs ago at the left edge", () => {
      const leftUs = nowUs - windowUs;
      expect(getGraphX(leftUs, nowUs, windowUs, width)).toBe(0);
    });

    it("places intermediate timestamps linearly", () => {
      const midUs = nowUs - windowUs / 2;
      expect(getGraphX(midUs, nowUs, windowUs, width)).toBe(width / 2);

      const quarterUs = nowUs - windowUs * 0.75;
      expect(getGraphX(quarterUs, nowUs, windowUs, width)).toBe(width * 0.25);
    });

    it("handles zero width or window gracefully", () => {
      expect(getGraphX(nowUs, nowUs, 0, width)).toBe(0);
      expect(getGraphX(nowUs, nowUs, windowUs, 0)).toBe(0);
    });
  });

  describe("getGraphY", () => {
    const height = 400;

    it("places 0 (idle) at the bottom and 1 (100%) at the top", () => {
      expect(getGraphY(0, height)).toBe(height);
      expect(getGraphY(1, height)).toBe(0);
      expect(getGraphY(0.5, height)).toBe(height / 2);
    });

    it("clamps values outside 0..1", () => {
      expect(getGraphY(-0.5, height)).toBe(height);
      expect(getGraphY(1.5, height)).toBe(0);
    });

    it("respects top and bottom padding", () => {
      const topPad = 20;
      const bottomPad = 20;
      // Usable height is 400 - 40 = 360
      expect(getGraphY(1, height, topPad, bottomPad)).toBe(topPad); // 20
      expect(getGraphY(0, height, topPad, bottomPad)).toBe(height - bottomPad); // 380
      expect(getGraphY(0.5, height, topPad, bottomPad)).toBe(topPad + 180); // 200
    });
  });
});
