import { describe, expect, it, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/svelte";
import LivePage from "./+page.svelte";
import { loadGraphWindow, saveGraphWindow } from "$lib/settings";

describe("Live page (page.test.ts)", () => {
  beforeEach(() => {
    localStorage.clear();

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
      setLineDash: vi.fn(),
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

  it("renders heading, home link, HUD, and demo data badge", async () => {
    render(LivePage);

    expect(screen.getByRole("heading", { level: 1 })).toHaveTextContent("Live Pedal View");

    const homeLink = screen.getByRole("link", { name: /home/i });
    expect(homeLink).toHaveAttribute("href", "/");

    expect(screen.getByTestId("demo-badge")).toHaveTextContent("Demo data");
    expect(screen.getByTestId("frame-time-hud")).toBeInTheDocument();
  });

  it("loads window setting, updates on slider input, and saves to settings", async () => {
    saveGraphWindow(7);
    render(LivePage);

    const slider = screen.getByRole("slider", { name: /graph time window/i }) as HTMLInputElement;
    expect(slider.value).toBe("7");
    expect(screen.getByText("7 s")).toBeInTheDocument();

    await fireEvent.input(slider, { target: { value: "4" } });

    expect(slider.value).toBe("4");
    expect(screen.getByText("4 s")).toBeInTheDocument();
    expect(loadGraphWindow()).toBe(4);
  });

  it("toggles latency check mode and renders flash hint", async () => {
    render(LivePage);

    const toggleBtn = screen.getByTestId("latency-toggle");
    expect(toggleBtn).toHaveAttribute("aria-pressed", "false");
    expect(screen.queryByTestId("latency-hint")).not.toBeInTheDocument();

    await fireEvent.click(toggleBtn);
    expect(toggleBtn).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByTestId("latency-hint")).toBeInTheDocument();

    await fireEvent.click(toggleBtn);
    expect(toggleBtn).toHaveAttribute("aria-pressed", "false");
    expect(screen.queryByTestId("latency-hint")).not.toBeInTheDocument();
  });
});
