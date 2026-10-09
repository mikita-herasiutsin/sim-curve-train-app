<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { pedalStream, type PedalStream } from "$lib/pedals/stream";
  import { readThemeColors, type AppThemeColors } from "$lib/pedals/theme";
  import { onThemeChange } from "$lib/settings";
  import { toleranceOf, type TraceDrill } from "$lib/drill";
  import {
    drawTrace,
    TraceCurve,
    GO_LEAD_MS,
    traceViewPhase,
    type TraceViewState,
  } from "$lib/pedals/traceDraw";

  interface Props {
    stream?: PedalStream;
    drill: TraceDrill;
    curve: TraceCurve;
    repStartUs: number;
    active: boolean;
    countdownEndsUs?: number;
    countingDown?: boolean;
  }

  let {
    stream = pedalStream,
    drill,
    curve,
    repStartUs,
    active,
    countdownEndsUs = 0,
    countingDown = false,
  }: Props = $props();

  let lastShownStartUs = 0;

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
    if (!ctx || !theme) return;

    ctx.save();
    ctx.scale(dpr, dpr);

    const durationMs = curve.durationMs;
    const tolerance = toleranceOf(drill) / 100;
    const pedal = drill.pedal === "throttle" ? "throttle" : "brake";

    const now = stream.dataNowUs();
    if (countingDown && (countdownEndsUs <= 0 || countdownEndsUs - now > GO_LEAD_MS * 1000)) {
      lastShownStartUs = 0;
    }
    const phase = traceViewPhase({
      nowUs: now,
      countingDown,
      countdownEndsUs,
      active,
      repStartUs,
      lastShownStartUs,
      durationMs,
    });
    if (phase.playheadMs !== null) {
      lastShownStartUs = phase.shownStartUs;
    }
    const shownStartUs = phase.shownStartUs;
    const playheadMs = phase.playheadMs;

    const user: [number, number][] = [];
    if (shownStartUs > 0) {
      const windowStartUs = shownStartUs - GO_LEAD_MS * 1000;
      const windowEndUs = shownStartUs + durationMs * 1000;
      stream.history.forEachSince(windowStartUs, (frame) => {
        if (frame.t <= windowEndUs) {
          const val = pedal === "brake" ? frame.brake : frame.throttle;
          user.push([(frame.t - shownStartUs) / 1000, val]);
        }
      });
    }

    let inBand = false;
    if (playheadMs !== null) {
      const latest = stream.history.latest();
      const val = latest ? (pedal === "brake" ? latest.brake : latest.throttle) : 0;
      const [lo, hi] = curve.envelopeAt(playheadMs);
      inBand = val >= lo - tolerance && val <= hi + tolerance;
    }

    const state: TraceViewState = {
      curve,
      pedal,
      tolerance,
      playheadMs,
      user,
      inBand,
    };

    drawTrace(ctx, width, height, state, theme, 18, 26);
    ctx.restore();
  }

  onMount(() => {
    if (!containerEl || !canvasEl) return;

    theme = readThemeColors(containerEl);

    width = containerEl.clientWidth || 600;
    height = containerEl.clientHeight || 360;
    updateResolution();

    unsubTheme = onThemeChange(() => {
      if (containerEl) {
        theme = readThemeColors(containerEl);
        render();
      }
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

<div class="trace-view-container" data-testid="trace-view" bind:this={containerEl}>
  <canvas
    bind:this={canvasEl}
    aria-label="Trace drill target curve"
    style="width: 100%; height: 100%;"
  ></canvas>
</div>

<style>
  .trace-view-container {
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
