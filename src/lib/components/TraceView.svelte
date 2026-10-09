<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { pedalStream, type PedalStream } from "$lib/pedals/stream";
  import { readThemeColors, type AppThemeColors } from "$lib/pedals/theme";
  import { onThemeChange } from "$lib/settings";
  import { toleranceOf, type TraceDrill } from "$lib/drill";
  import { drawTrace, TraceCurve, GO_LEAD_MS, type TraceViewState } from "$lib/pedals/traceDraw";

  interface Props {
    stream?: PedalStream;
    drill: TraceDrill;
    repStartUs: number;
    active: boolean;
    countdownEndsUs?: number;
    countingDown?: boolean;
  }

  let {
    stream = pedalStream,
    drill,
    repStartUs,
    active,
    countdownEndsUs = 0,
    countingDown = false,
  }: Props = $props();

  const curve = $derived(new TraceCurve(drill.points));
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
    let playheadMs: number | null = null;
    let shownStartUs = 0;

    if (countingDown && countdownEndsUs > 0 && countdownEndsUs - now <= GO_LEAD_MS * 1000) {
      shownStartUs = countdownEndsUs;
      lastShownStartUs = shownStartUs;
      playheadMs = (now - countdownEndsUs) / 1000;
    } else if (active) {
      shownStartUs = repStartUs;
      lastShownStartUs = shownStartUs;
      const repMs = (now - repStartUs) / 1000;
      playheadMs = Math.max(-GO_LEAD_MS, Math.min(durationMs, repMs));
    } else {
      shownStartUs = lastShownStartUs > 0 ? lastShownStartUs : repStartUs;
    }

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
      if (playheadMs < 0) {
        const firstVal = curve.valueAt(0);
        inBand = val >= firstVal - tolerance && val <= firstVal + tolerance;
      } else {
        const [lo, hi] = curve.envelopeAt(playheadMs);
        inBand = val >= lo - tolerance && val <= hi + tolerance;
      }
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
