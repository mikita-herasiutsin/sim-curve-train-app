<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { pedalStream, type PedalStream } from "$lib/pedals/stream";
  import { readThemeColors, type AppThemeColors } from "$lib/pedals/theme";
  import { ColumnDecimator, drawGraph } from "$lib/pedals/graphDraw";
  import { getGraphX } from "$lib/pedals/geometry";

  interface Props {
    stream?: PedalStream;
    windowSeconds?: number;
  }

  let { stream = pedalStream, windowSeconds = 5 }: Props = $props();

  let containerEl = $state<HTMLDivElement | null>(null);
  let canvasEl = $state<HTMLCanvasElement | null>(null);

  let rafId: number | null = null;
  let resizeObserver: ResizeObserver | null = null;
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

  function render(): void {
    if (!canvasEl || !theme || !decimator || width <= 0 || height <= 0) return;
    const ctx = canvasEl.getContext("2d");
    if (!ctx) return;

    ctx.save();
    ctx.scale(dpr, dpr);

    const nowUs = performance.now() * 1000;
    const winSec = Math.max(3, Math.min(10, windowSeconds));
    const windowUs = winSec * 1_000_000;
    const tMinUs = nowUs - windowUs;

    decimator.reset();
    const cols = decimator.numCols;

    stream.history.forEachSince(tMinUs, (frame) => {
      const xDevice = Math.floor(getGraphX(frame.t, nowUs, windowUs, cols));
      decimator?.accumulate(xDevice, frame.brake, frame.throttle);
    });

    drawGraph(ctx, width, height, decimator, nowUs, windowUs, theme, 18, 26, dpr);
    ctx.restore();
  }

  onMount(() => {
    if (!containerEl || !canvasEl) return;

    theme = readThemeColors(containerEl);

    width = containerEl.clientWidth || 600;
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
