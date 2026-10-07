<script lang="ts">
  import { onMount } from "svelte";
  import { normaliseRaw, type DeviceStream, type SampleBatch } from "$lib/stream";

  let { stream, error = null }: { stream: DeviceStream; error?: string | null } = $props();

  let axes = $state<number[]>([]);
  let sampleRateHz = $state(0);
  let batchAgeMs = $state(0);
  let batchIntervalMs = $state(0);

  onMount(() => {
    // Batches arrive at ~125 Hz; only the newest one is rendered, once per animation frame.
    let latest: SampleBatch | null = null;
    let lastBatchAt = 0;
    let intervalSum = 0;
    let intervalCount = 0;
    let frame = 0;

    const render = () => {
      frame = requestAnimationFrame(render);
      if (!latest) return;
      const last = latest.samples.at(-1);
      if (last) axes = last.axes.slice(0, last.axisCount);
      sampleRateHz = latest.stats.sampleRateHz;
      batchAgeMs = latest.stats.batchAgeMs;
      if (intervalCount > 0) batchIntervalMs = intervalSum / intervalCount;
      intervalSum = 0;
      intervalCount = 0;
      latest = null;
    };
    frame = requestAnimationFrame(render);

    const unsubscribe = stream.subscribe((batch) => {
      const now = performance.now();
      if (lastBatchAt > 0) {
        intervalSum += now - lastBatchAt;
        intervalCount += 1;
      }
      lastBatchAt = now;
      latest = batch;
    });

    return () => {
      cancelAnimationFrame(frame);
      unsubscribe();
    };
  });
</script>

<section class="monitor" aria-label="Raw axis monitor">
  <div class="hud" data-testid="stream-hud">
    <span><strong>{sampleRateHz.toFixed(0)}</strong> Hz</span>
    <span>batch age <strong>{batchAgeMs.toFixed(1)}</strong> ms</span>
    <span>batch interval <strong>{batchIntervalMs.toFixed(1)}</strong> ms</span>
  </div>

  {#if error}
    <p class="error" role="alert">Stream failed: {error}</p>
  {:else if axes.length === 0}
    <p class="muted">Waiting for samples…</p>
  {:else}
    <ol class="axes">
      {#each axes as raw, i (i)}
        {@const value = normaliseRaw(raw)}
        <li>
          <span class="label">Axis {i}</span>
          <span
            class="bar"
            role="meter"
            aria-valuenow={raw}
            aria-valuemin={-32768}
            aria-valuemax={32767}
          >
            <span class="fill" style:width="{value * 100}%"></span>
          </span>
          <span class="value">{raw}</span>
          <span class="value">{(value * 100).toFixed(1)}%</span>
        </li>
      {/each}
    </ol>
  {/if}
</section>

<style>
  .monitor {
    display: grid;
    gap: 1rem;
    padding: 1.25rem;
    border: 1px solid var(--border);
    border-radius: 0.75rem;
    background: var(--surface);
  }

  .hud {
    display: flex;
    gap: 1.5rem;
    color: var(--text-muted);
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
    font-size: 0.8125rem;
  }

  .hud strong {
    color: var(--text);
    font-variant-numeric: tabular-nums;
  }

  .axes {
    display: grid;
    gap: 0.5rem;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  li {
    display: grid;
    grid-template-columns: 4.5rem 1fr 4.5rem 4.5rem;
    align-items: center;
    gap: 0.75rem;
  }

  .label {
    color: var(--text-muted);
    font-size: 0.875rem;
  }

  .bar {
    height: 1rem;
    border-radius: 0.375rem;
    background: var(--surface-raised);
    overflow: hidden;
  }

  .fill {
    display: block;
    height: 100%;
    background: var(--accent);
  }

  .value {
    text-align: right;
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
    font-size: 0.8125rem;
    font-variant-numeric: tabular-nums;
  }

  .muted {
    margin: 0;
    color: var(--text-muted);
  }

  .error {
    margin: 0;
    color: var(--brake);
  }
</style>
