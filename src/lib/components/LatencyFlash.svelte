<script lang="ts">
  import { onDestroy } from "svelte";
  import { pedalStream, type PedalStream } from "$lib/pedals/stream";
  import { createCrossingDetector } from "$lib/pedals/latency";

  interface Props {
    enabled?: boolean;
    stream?: PedalStream;
    threshold?: number;
    rearm?: number;
  }

  let { enabled = true, stream = pedalStream, threshold = 0.5, rearm = 0.4 }: Props = $props();

  let flashing = $state(false);
  let flashTimer: ReturnType<typeof setTimeout> | null = null;
  let rafId: number | null = null;

  const detector = $derived(createCrossingDetector(threshold, rearm));

  function triggerFlash(): void {
    flashing = true;
    if (flashTimer !== null) clearTimeout(flashTimer);
    flashTimer = setTimeout(() => {
      flashing = false;
      flashTimer = null;
    }, 100);
  }

  $effect(() => {
    if (!enabled) {
      flashing = false;
      if (flashTimer !== null) {
        clearTimeout(flashTimer);
        flashTimer = null;
      }
      if (rafId !== null) {
        cancelAnimationFrame(rafId);
        rafId = null;
      }
      return;
    }

    const loop = () => {
      const latest = stream.history.latest();
      const brake = latest?.brake ?? 0;
      if (detector(brake)) {
        triggerFlash();
      }
      rafId = requestAnimationFrame(loop);
    };

    rafId = requestAnimationFrame(loop);

    return () => {
      if (rafId !== null) {
        cancelAnimationFrame(rafId);
        rafId = null;
      }
      if (flashTimer !== null) {
        clearTimeout(flashTimer);
        flashTimer = null;
      }
    };
  });

  onDestroy(() => {
    if (rafId !== null && typeof cancelAnimationFrame !== "undefined") {
      cancelAnimationFrame(rafId);
      rafId = null;
    }
    if (flashTimer !== null) {
      clearTimeout(flashTimer);
      flashTimer = null;
    }
  });
</script>

{#if enabled}
  {#if flashing}
    <div class="flash-overlay" data-testid="flash-overlay" aria-hidden="true"></div>
  {/if}
  <div class="latency-hint" data-testid="latency-hint">
    <span>Latency check active: tap brake past {Math.round(threshold * 100)}% to flash</span>
  </div>
{/if}

<style>
  .flash-overlay {
    position: fixed;
    inset: 0;
    background-color: #ffffff;
    z-index: 9999;
    pointer-events: none;
  }

  .latency-hint {
    position: fixed;
    bottom: 1.5rem;
    left: 50%;
    transform: translateX(-50%);
    z-index: 100;
    padding: 0.5rem 1rem;
    background: var(--surface-raised);
    border: 1px solid var(--accent);
    color: var(--text);
    border-radius: 999px;
    font-size: 0.8125rem;
    font-weight: 500;
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.4);
    pointer-events: none;
    white-space: nowrap;
  }
</style>
