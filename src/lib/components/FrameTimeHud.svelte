<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { pedalStream, type PedalStream } from "$lib/pedals/stream";

  interface Props {
    stream?: PedalStream;
  }

  let { stream = pedalStream }: Props = $props();

  let fps = $state(0);
  let avgFrameTimeMs = $state(0);
  let maxFrameTimeMs = $state(0);
  let sampleRateHz = $state(0);
  let batchAvgMs = $state(0);
  let batchMaxMs = $state(0);

  let rafId: number | null = null;
  let lastFrameTime = 0;
  let lastTextUpdate = 0;

  // Sliding window of frame times over the last 1 second
  interface FrameSample {
    time: number;
    deltaMs: number;
  }
  let frameSamples: FrameSample[] = [];

  function pruneSamples(now: number): void {
    const threshold = now - 1000;
    const firstValid = frameSamples.findIndex((s) => s.time >= threshold);
    if (firstValid === -1) {
      frameSamples = [];
    } else if (firstValid > 0) {
      frameSamples = frameSamples.slice(firstValid);
    }
  }

  onMount(() => {
    lastFrameTime = performance.now();
    lastTextUpdate = lastFrameTime;

    const loop = (time: number) => {
      const deltaMs = time - lastFrameTime;
      lastFrameTime = time;

      if (deltaMs > 0 && deltaMs < 1000) {
        frameSamples.push({ time, deltaMs });
        pruneSamples(time);
      }

      // Update displayed text at roughly 4 Hz (every 250 ms)
      if (time - lastTextUpdate >= 250) {
        lastTextUpdate = time;

        if (frameSamples.length >= 2) {
          let maxDt = 0;
          let sumDt = 0;
          for (const sample of frameSamples) {
            sumDt += sample.deltaMs;
            if (sample.deltaMs > maxDt) maxDt = sample.deltaMs;
          }
          const avgDt = sumDt / frameSamples.length;
          const timeSpanSec =
            (frameSamples[frameSamples.length - 1].time - frameSamples[0].time) / 1000;
          fps = timeSpanSec > 0 ? Math.round((frameSamples.length - 1) / timeSpanSec) : 0;
          avgFrameTimeMs = Math.round(avgDt * 10) / 10;
          maxFrameTimeMs = Math.round(maxDt * 10) / 10;
        } else {
          fps = 0;
          avgFrameTimeMs = 0;
          maxFrameTimeMs = 0;
        }

        sampleRateHz = Math.round(stream.sampleRateHz);
        const batchStats = stream.batchIntervalStats;
        batchAvgMs = Math.round(batchStats.avgMs * 10) / 10;
        batchMaxMs = Math.round(batchStats.maxMs * 10) / 10;
      }

      rafId = requestAnimationFrame(loop);
    };

    rafId = requestAnimationFrame(loop);
  });

  onDestroy(() => {
    if (rafId !== null && typeof cancelAnimationFrame !== "undefined") {
      cancelAnimationFrame(rafId);
      rafId = null;
    }
  });
</script>

<div class="frame-time-hud" data-testid="frame-time-hud">
  <div class="metric-row">
    <span class="label">Render:</span>
    <span class="val font-mono">{fps} FPS</span>
    <span class="sub font-mono">({avgFrameTimeMs} ms avg / {maxFrameTimeMs} ms max)</span>
  </div>
  <div class="metric-row">
    <span class="label">Stream:</span>
    <span class="val font-mono">{sampleRateHz} Hz</span>
    <span class="sub font-mono">({batchAvgMs} ms avg / {batchMaxMs} ms max batch)</span>
  </div>
</div>

<style>
  .frame-time-hud {
    display: inline-flex;
    flex-direction: column;
    gap: 0.25rem;
    padding: 0.5rem 0.75rem;
    background: color-mix(in srgb, var(--surface-raised) 85%, transparent);
    backdrop-filter: blur(8px);
    border: 1px solid var(--border);
    border-radius: 0.5rem;
    font-size: 0.75rem;
    line-height: 1.3;
    color: var(--text-muted);
    user-select: none;
    pointer-events: none;
    z-index: 10;
  }

  .metric-row {
    display: flex;
    align-items: baseline;
    gap: 0.375rem;
    white-space: nowrap;
  }

  .label {
    color: var(--text-muted);
    font-weight: 600;
  }

  .val {
    color: var(--text);
    font-weight: 700;
  }

  .sub {
    color: var(--text-muted);
    font-size: 0.6875rem;
  }

  .font-mono {
    font-family:
      ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, "Liberation Mono", monospace;
    font-variant-numeric: tabular-nums;
  }
</style>
