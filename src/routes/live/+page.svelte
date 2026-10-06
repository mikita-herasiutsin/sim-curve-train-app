<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { resolve } from "$app/paths";
  import { isTauri } from "@tauri-apps/api/core";
  import { pedalStream, type PedalStream } from "$lib/pedals/stream";
  import { startMockSource } from "$lib/pedals/mockSource";
  import { startRealSource, type SourceStatus } from "$lib/pedals/realSource";
  import { loadGraphWindow, saveGraphWindow } from "$lib/settings";
  import PedalBars from "$lib/components/PedalBars.svelte";
  import PedalGraph from "$lib/components/PedalGraph.svelte";
  import FrameTimeHud from "$lib/components/FrameTimeHud.svelte";
  import LatencyFlash from "$lib/components/LatencyFlash.svelte";

  let windowSeconds = $state(5);
  let latencyFlashEnabled = $state(false);
  let isDemoSource = $state(false);
  let status = $state<SourceStatus>({ kind: "connecting" });
  let stopSource: (() => void) | null = null;

  /**
   * Connects the pedal stream to an input source.
   * In the app: the calibrated frames of the first device with a saved profile.
   * In a plain browser: the mock source, with the Demo data badge.
   */
  function connectSource(stream: PedalStream): () => void {
    // Outside the app (plain `npm run dev` in a browser) there is no Rust backend.
    if (typeof window === "undefined" || !isTauri()) {
      isDemoSource = true;
      return startMockSource(stream);
    }
    isDemoSource = false;
    return startRealSource(stream, (next) => (status = next));
  }

  function handleWindowChange(event: Event): void {
    const target = event.currentTarget as HTMLInputElement;
    const val = Number(target.value);
    windowSeconds = val;
    saveGraphWindow(val);
  }

  onMount(() => {
    windowSeconds = loadGraphWindow();
    stopSource = connectSource(pedalStream);
  });

  onDestroy(() => {
    if (stopSource) {
      stopSource();
      stopSource = null;
    }
  });
</script>

<div class="live-page">
  <header class="live-header">
    <div class="header-left">
      <a href={resolve("/")} class="home-link" aria-label="Back to home">← Home</a>
      <h1 class="page-title">Live Pedal View</h1>
      {#if isDemoSource}
        <span class="badge-demo" data-testid="demo-badge">Demo data</span>
      {/if}
      {#if !isDemoSource}
        <span class="source-status" data-testid="source-status" aria-live="polite">
          {#if status.kind === "live"}
            {status.device.name}
          {:else if status.kind === "disconnected"}
            <span class="source-error">{status.device.name} disconnected</span>
          {:else if status.kind === "noProfile"}
            No pedal profile yet. <a href={resolve("/devices")}>Set up your pedals</a>
          {:else if status.kind === "error"}
            <span class="source-error">Input error: {status.message}</span>
          {:else}
            Connecting…
          {/if}
        </span>
      {/if}
    </div>

    <div class="header-right">
      <FrameTimeHud stream={pedalStream} />
    </div>
  </header>

  <main class="live-workspace">
    <section class="bars-panel" aria-label="Live pedal bars">
      <PedalBars stream={pedalStream} />
    </section>

    <section class="graph-panel" aria-label="Live scrolling graph">
      <div class="graph-toolbar">
        <div class="toolbar-control">
          <label for="window-range" class="control-label">
            Window: <strong>{windowSeconds} s</strong>
          </label>
          <input
            id="window-range"
            type="range"
            min="3"
            max="10"
            step="1"
            value={windowSeconds}
            oninput={handleWindowChange}
            aria-label="Graph time window in seconds"
          />
        </div>

        <button
          type="button"
          class="btn-toggle"
          class:active={latencyFlashEnabled}
          onclick={() => (latencyFlashEnabled = !latencyFlashEnabled)}
          aria-pressed={latencyFlashEnabled}
          data-testid="latency-toggle"
        >
          <span class="indicator" aria-hidden="true"></span>
          Latency check
        </button>
      </div>

      <div class="graph-canvas-wrapper">
        <PedalGraph stream={pedalStream} {windowSeconds} />
        {#if !isDemoSource && status.kind !== "live"}
          <div class="graph-overlay" data-testid="graph-overlay" role="status">
            {#if status.kind === "disconnected"}
              <strong>{status.device.name} disconnected</strong>
              <span>Plug it back in. The view reconnects by itself.</span>
            {:else if status.kind === "noProfile"}
              <strong>No pedals set up</strong>
              <a href={resolve("/devices")}>Set up your pedals on the Devices page</a>
            {:else if status.kind === "error"}
              <strong>Input error</strong>
              <span>{status.message}</span>
            {:else}
              <strong>Connecting…</strong>
            {/if}
          </div>
        {/if}
      </div>
    </section>
  </main>

  <LatencyFlash enabled={latencyFlashEnabled} stream={pedalStream} />
</div>

<style>
  .source-status {
    color: var(--text-muted);
    font-size: 0.875rem;
  }

  .source-status a {
    color: var(--accent);
  }

  .source-error {
    color: var(--brake);
  }

  .live-page {
    display: flex;
    flex-direction: column;
    min-height: 100vh;
    background: var(--bg);
    color: var(--text);
  }

  .live-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 1rem;
    padding: 0.75rem 1.5rem;
    background: var(--surface);
    border-bottom: 1px solid var(--border);
  }

  .header-left {
    display: flex;
    align-items: center;
    gap: 1rem;
  }

  .home-link {
    color: var(--accent);
    text-decoration: none;
    font-size: 0.875rem;
    font-weight: 500;
    padding: 0.375rem 0.625rem;
    border-radius: 0.375rem;
    border: 1px solid var(--border);
    background: var(--surface-raised);
    transition: background 0.15s ease;
  }

  .home-link:hover {
    background: color-mix(in srgb, var(--accent) 15%, var(--surface-raised));
  }

  .page-title {
    margin: 0;
    font-size: 1.125rem;
    font-weight: 700;
  }

  .badge-demo {
    padding: 0.2rem 0.5rem;
    background: color-mix(in srgb, var(--accent) 18%, transparent);
    color: var(--accent);
    border: 1px solid color-mix(in srgb, var(--accent) 30%, transparent);
    border-radius: 999px;
    font-size: 0.75rem;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  .header-right {
    display: flex;
    align-items: center;
  }

  .live-workspace {
    flex: 1;
    display: grid;
    grid-template-columns: 220px 1fr;
    gap: 1.25rem;
    padding: 1.25rem 1.5rem;
    box-sizing: border-box;
    min-height: 0;
  }

  @media (max-width: 48rem) {
    .live-workspace {
      grid-template-columns: 1fr;
    }
  }

  .bars-panel {
    display: flex;
    flex-direction: column;
    min-height: 20rem;
  }

  .graph-panel {
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
    min-height: 20rem;
  }

  .graph-toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 1rem;
    padding: 0.5rem 1rem;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 0.5rem;
  }

  .toolbar-control {
    display: flex;
    align-items: center;
    gap: 0.75rem;
  }

  .control-label {
    font-size: 0.8125rem;
    color: var(--text-muted);
    white-space: nowrap;
  }

  .control-label strong {
    color: var(--text);
  }

  input[type="range"] {
    accent-color: var(--accent);
    cursor: pointer;
  }

  .btn-toggle {
    display: inline-flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.375rem 0.75rem;
    border: 1px solid var(--border);
    border-radius: 0.375rem;
    background: var(--surface-raised);
    color: var(--text-muted);
    font-size: 0.8125rem;
    font-weight: 500;
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .btn-toggle:hover {
    color: var(--text);
    border-color: var(--text-muted);
  }

  .btn-toggle.active {
    background: color-mix(in srgb, var(--accent) 20%, var(--surface-raised));
    color: var(--text);
    border-color: var(--accent);
  }

  .indicator {
    width: 0.5rem;
    height: 0.5rem;
    border-radius: 50%;
    background: var(--text-muted);
  }

  .btn-toggle.active .indicator {
    background: var(--accent);
    box-shadow: 0 0 8px var(--accent);
  }

  .graph-canvas-wrapper {
    position: relative;
    flex: 1;
    display: flex;
    min-height: 0;
  }

  .graph-overlay {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 0.5rem;
    background: color-mix(in srgb, var(--bg) 70%, transparent);
    color: var(--text-muted);
    text-align: center;
  }

  .graph-overlay strong {
    color: var(--text);
    font-size: 1.25rem;
  }

  .graph-overlay a {
    color: var(--accent);
  }
</style>
