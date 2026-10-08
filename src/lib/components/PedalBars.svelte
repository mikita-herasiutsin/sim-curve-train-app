<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { pedalStream, type PedalStream } from "$lib/pedals/stream";
  import { readThemeColors, type AppThemeColors } from "$lib/pedals/theme";
  import { onThemeChange } from "$lib/settings";
  import { drawPedalBars } from "$lib/pedals/barsDraw";

  interface Props {
    stream?: PedalStream;
    targetPedal?: "brake" | "throttle" | "clutch";
    targetVal?: number | null;
    targetTolerance?: number | null;
  }

  let {
    stream = pedalStream,
    targetPedal,
    targetVal = null,
    targetTolerance = null,
  }: Props = $props();

  let containerEl = $state<HTMLDivElement | null>(null);
  let canvasEl = $state<HTMLCanvasElement | null>(null);

  let rafId: number | null = null;
  let resizeObserver: ResizeObserver | null = null;
  let unsubTheme: (() => void) | null = null;
  let width = 0;
  let height = 0;
  let dpr = 1;
  let theme: AppThemeColors | null = null;

  function updateResolution(): void {
    if (!canvasEl || width <= 0 || height <= 0) return;
    dpr = typeof window !== "undefined" ? window.devicePixelRatio || 1 : 1;
    canvasEl.width = Math.round(width * dpr);
    canvasEl.height = Math.round(height * dpr);
  }

  function render(): void {
    if (!canvasEl || width <= 0 || height <= 0) return;
    const ctx = canvasEl.getContext("2d");
    if (!ctx) return;
    // Colours are cached: read on mount and refreshed by onThemeChange, not per frame.
    if (!theme) return;

    ctx.save();
    ctx.scale(dpr, dpr);

    const latest = stream.history.latest();
    const brake = latest?.brake ?? 0;
    const throttle = latest?.throttle ?? 0;

    drawPedalBars(
      ctx,
      width,
      height,
      brake,
      throttle,
      theme,
      targetPedal,
      targetVal,
      targetTolerance,
    );
    ctx.restore();
  }

  onMount(() => {
    if (!containerEl || !canvasEl) return;

    theme = readThemeColors(containerEl);

    // Initial size
    width = containerEl.clientWidth || 180;
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

<div class="pedal-bars-container" bind:this={containerEl}>
  <canvas
    bind:this={canvasEl}
    aria-label="Live brake and throttle pedal bars"
    style="width: 100%; height: 100%;"
  ></canvas>
</div>

<style>
  .pedal-bars-container {
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
