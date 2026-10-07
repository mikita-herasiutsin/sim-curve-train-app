<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { pedalStream, type PedalStream } from "$lib/pedals/stream";
  import { readThemeColors, type AppThemeColors } from "$lib/pedals/theme";
  import { drawPedalBars } from "$lib/pedals/barsDraw";

  interface Props {
    stream?: PedalStream;
  }

  let { stream = pedalStream }: Props = $props();

  let containerEl = $state<HTMLDivElement | null>(null);
  let canvasEl = $state<HTMLCanvasElement | null>(null);

  let rafId: number | null = null;
  let resizeObserver: ResizeObserver | null = null;
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
    if (!canvasEl || !theme || width <= 0 || height <= 0) return;
    const ctx = canvasEl.getContext("2d");
    if (!ctx) return;

    ctx.save();
    ctx.scale(dpr, dpr);

    const latest = stream.history.latest();
    const brake = latest?.brake ?? 0;
    const throttle = latest?.throttle ?? 0;

    drawPedalBars(ctx, width, height, brake, throttle, theme);
    ctx.restore();
  }

  onMount(() => {
    if (!containerEl || !canvasEl) return;

    theme = readThemeColors(containerEl);

    // Initial size
    width = containerEl.clientWidth || 180;
    height = containerEl.clientHeight || 360;
    updateResolution();

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
