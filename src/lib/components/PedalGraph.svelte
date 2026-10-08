<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { pedalStream, type PedalStream } from "$lib/pedals/stream";
  import { readThemeColors, type AppThemeColors } from "$lib/pedals/theme";
  import { onThemeChange } from "$lib/settings";
  import { ColumnDecimator, drawGraph, type TargetBand } from "$lib/pedals/graphDraw";
  import { getGraphX } from "$lib/pedals/geometry";

  interface Props {
    stream?: PedalStream;
    windowSeconds?: number;
    /** Optional target band drawn behind the lines (drill screen). */
    band?: TargetBand | null;
  }

  let { stream = pedalStream, windowSeconds = 5, band = null }: Props = $props();

  let containerEl = $state<HTMLDivElement | null>(null);
  let canvasEl = $state<HTMLCanvasElement | null>(null);

  let rafId: number | null = null;
  let resizeObserver: ResizeObserver | null = null;
  let unsubTheme: (() => void) | null = null;
  let width = 0;
  let height = 0;
  let dpr = 1;
  let theme: AppThemeColors | null = null;
  let decimator: ColumnDecimator | null = null;

  function updateResolution(): void {
    if (!canvasEl || width <= 0 || height <= 0) return;
    dpr = typeof window !== "undefined" ? window.devicePixelRatio || 1 : 1;
    canvasEl.width = Math.round(width * dpr);
    canvasEl.height = Math.round(height * dpr);

    const numCols = Math.max(1, Math.round(width * dpr));
    if (!decimator) {
      decimator = new ColumnDecimator(numCols);
    } else {
      decimator.resize(numCols);
    }
  }

  /** Whether the latest value of the band's pedal sits inside the band. */
  function inBand(b: TargetBand | null): boolean {
    const latest = b ? stream.history.latest() : undefined;
    if (!b || !latest) return false;
    const value = b.pedal === "brake" ? latest.brake : latest.throttle;
    return Math.abs(value - b.target) <= b.tolerance;
  }

  function render(): void {
    if (!canvasEl || !decimator || width <= 0 || height <= 0) return;
    const ctx = canvasEl.getContext("2d");
    if (!ctx) return;
    // Colours are cached: read on mount and refreshed by onThemeChange, not per frame.
    if (!theme) return;

    ctx.save();
    ctx.scale(dpr, dpr);

    const nowUs = stream.dataNowUs();
    const winSec = Math.max(3, Math.min(10, windowSeconds));
    const windowUs = winSec * 1_000_000;
    const tMinUs = nowUs - windowUs;

    decimator.reset();
    const cols = decimator.numCols;

    stream.history.forEachSince(tMinUs, (frame) => {
      const xDevice = Math.floor(getGraphX(frame.t, nowUs, windowUs, cols));
      decimator?.accumulate(xDevice, frame.brake, frame.throttle);
    });

    drawGraph(
      ctx,
      width,
      height,
      decimator,
      nowUs,
      windowUs,
      theme,
      18,
      26,
      dpr,
      band,
      inBand(band),
    );
    ctx.restore();
  }

  onMount(() => {
    if (!containerEl || !canvasEl) return;

    theme = readThemeColors(containerEl);

    width = containerEl.clientWidth || 600;
    height = containerEl.clientHeight || 360;
    updateResolution();

    // Redraw immediately when theme changes
    unsubTheme = onThemeChange(() => {
      theme = readThemeColors(containerEl);
      render();
    });

    if (typeof ResizeObserver !== "undefined") {
      resizeObserver = new ResizeObserver((entries) => {
        for (const entry of entries) {
          const cr = entry.contentRect;
          if (cr.width > 0 && cr.height > 0) {
            width = Math.floor(cr.width);
            height = Math.floor(cr.height);
            updateResolution();
          }
        }
      });
      resizeObserver.observe(containerEl);
    }

    // Initial render
    render();

    const loop = () => {
      render();
      rafId = requestAnimationFrame(loop);
    };
    rafId = requestAnimationFrame(loop);
  });

  onDestroy(() => {
    if (rafId !== null && typeof cancelAnimationFrame !== "undefined") {
      cancelAnimationFrame(rafId);
      rafId = null;
    }
    if (resizeObserver) {
      resizeObserver.disconnect();
      resizeObserver = null;
    }
    if (unsubTheme) {
      unsubTheme();
      unsubTheme = null;
    }
  });
</script>

<div class="pedal-graph-container" bind:this={containerEl}>
  <canvas
    bind:this={canvasEl}
    aria-label="Scrolling pedal telemetry graph"
    style="width: 100%; height: 100%;"
  ></canvas>
</div>

<style>
  .pedal-graph-container {
    width: 100%;
    height: 100%;
    min-height: 18rem;
    position: relative;
    overflow: hidden;
    border-radius: 0.75rem;
    border: 1px solid var(--border);
    background: var(--surface);
  }

  canvas {
    display: block;
    width: 100%;
    height: 100%;
  }
</style>
