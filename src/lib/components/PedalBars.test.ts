import { describe, expect, it, vi, beforeEach, afterEach } from "vitest";
import { render } from "@testing-library/svelte";
import PedalBars from "./PedalBars.svelte";
import { PedalStream } from "$lib/pedals/stream";
import { applyTheme, saveTheme } from "$lib/settings";
import * as barsDrawModule from "$lib/pedals/barsDraw";
import { DEFAULT_LIGHT_THEME_COLORS, DEFAULT_DARK_THEME_COLORS } from "$lib/pedals/theme";

describe("PedalBars component", () => {
  beforeEach(() => {
    localStorage.clear();
    applyTheme("dark");

    vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue({
      clearRect: vi.fn(),
      fillRect: vi.fn(),
      beginPath: vi.fn(),
      moveTo: vi.fn(),
      lineTo: vi.fn(),
      stroke: vi.fn(),
      fillText: vi.fn(),
      save: vi.fn(),
      restore: vi.fn(),
      scale: vi.fn(),
      arcTo: vi.fn(),
      closePath: vi.fn(),
      fill: vi.fn(),
      clip: vi.fn(),
    } as unknown as CanvasRenderingContext2D);

    globalThis.ResizeObserver = class {
      observe() {}
      unobserve() {}
      disconnect() {}
    } as unknown as typeof ResizeObserver;
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("renders canvas and redraws with light theme tokens when theme changes", async () => {
    const drawSpy = vi.spyOn(barsDrawModule, "drawPedalBars");
    const stream = new PedalStream();
    stream.ingest([{ t: 1000, brake: 0.5, throttle: 0.2 }]);

    render(PedalBars, { stream });

    // Initial draw should use dark theme
    expect(drawSpy).toHaveBeenCalled();
    const lastCallTheme = drawSpy.mock.calls.at(-1)?.[5];
    expect(lastCallTheme?.brake).toBe(DEFAULT_DARK_THEME_COLORS.brake);

    // Switch to light theme
    saveTheme("light");
    applyTheme("light");

    // Must have redrawn with light theme tokens
    expect(drawSpy).toHaveBeenCalled();
    const updatedCallTheme = drawSpy.mock.calls.at(-1)?.[5];
    expect(updatedCallTheme?.brake).toBe(DEFAULT_LIGHT_THEME_COLORS.brake);
    expect(updatedCallTheme?.throttle).toBe(DEFAULT_LIGHT_THEME_COLORS.throttle);
  });
});
