<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { resolve } from "$app/paths";
  import { isTauri } from "@tauri-apps/api/core";
  import {
    listPresets,
    startDrillRun,
    abortDrillRun,
    applyDrillEvent,
    IDLE_VIEW,
    type Preset,
    type Drill,
    type DrillEvent,
    type RunView,
  } from "$lib/drill";
  import { pedalStream } from "$lib/pedals/stream";
  import { startRealSource, type SourceStatus } from "$lib/pedals/realSource";
  import PedalBars from "$lib/components/PedalBars.svelte";
  import PedalGraph from "$lib/components/PedalGraph.svelte";
  import type { TargetBand } from "$lib/pedals/graphDraw";

  let presets = $state<Preset[]>([]);
  let selectedPreset = $state<Preset | null>(null);
  let selectedDrill = $state<Drill | null>(null);

  let sourceStatus = $state<SourceStatus>({ kind: "connecting" });
  let stopSource: (() => void) | null = null;

  let view = $state<RunView>({ ...IDLE_VIEW });
  let countdownMs = $state(0);
  let errorMessage = $state<string | null>(null);
  // Set once the user aborts; the engine still ends the set with a final `setFinished`.
  let aborting = $state(false);

  // The target band on the graph, while a hold rep is active.
  // TODO(SCT-034): trace drills draw their target curve instead.
  const graphBand = $derived<TargetBand | null>(
    view.runState === "active" && selectedDrill?.type === "hold"
      ? {
          pedal: selectedDrill.pedal,
          target: selectedDrill.target / 100,
          tolerance: selectedDrill.tolerance / 100,
        }
      : null,
  );
  // Bumped on every start and abort, so events of a run the user left are ignored.
  let runId = 0;

  let rafId: number | null = null;

  onMount(() => {
    if (isTauri()) {
      listPresets()
        .then((p) => {
          presets = p;
          if (p.length > 0) {
            selectedPreset = p[0];
            selectedDrill = p[0].drills[0] ?? null;
          }
        })
        .catch((e) => console.error("Failed to load presets", e));

      stopSource = startRealSource(pedalStream, (s) => {
        sourceStatus = s;
      });
    }

    const loop = () => {
      if (view.runState === "countdown" && view.countdownEndsUs > 0) {
        const remainingUs = view.countdownEndsUs - pedalStream.dataNowUs();
        countdownMs = Math.max(0, Math.ceil(remainingUs / 1000));
      }
      rafId = requestAnimationFrame(loop);
    };
    rafId = requestAnimationFrame(loop);
  });

  onDestroy(() => {
    if (stopSource) stopSource();
    if (rafId !== null) cancelAnimationFrame(rafId);
    if (sourceStatus.kind === "live" && view.runState !== "idle" && view.runState !== "finished") {
      abortDrillRun(sourceStatus.token).catch(console.error);
    }
  });

  function start() {
    if (!selectedPreset || !selectedDrill) return;
    if (sourceStatus.kind !== "live") {
      errorMessage = "Connect your pedals before starting.";
      return;
    }

    errorMessage = null;
    aborting = false;
    const id = ++runId;
    const onEvent = (e: DrillEvent) => {
      if (id === runId) view = applyDrillEvent(view, e);
    };
    startDrillRun(sourceStatus.token, selectedPreset.id, selectedDrill.id, onEvent).catch((e) => {
      if (id !== runId) return;
      console.error(e);
      view = { ...IDLE_VIEW };
      errorMessage = `Failed to start drill: ${e}`;
    });
    view = { ...IDLE_VIEW, runState: "countdown" };
    countdownMs = selectedDrill.leadInMs;
    // countdownEndsUs is set by the first event
  }

  function abort() {
    if (sourceStatus.kind !== "live") {
      view = { ...IDLE_VIEW };
      return;
    }
    // The run stays current: its final `setFinished` shows the summary of what was scored.
    aborting = true;
    abortDrillRun(sourceStatus.token).catch((e) => {
      console.error(e);
      aborting = false;
      errorMessage = `Failed to abort drill: ${e}`;
    });
  }

  function restart() {
    view = { ...IDLE_VIEW };
    start();
  }
</script>

<div class="drill-page">
  <header class="app-header">
    <div class="brand">
      <a href={resolve("/")} class="home-link">← Home</a>
      <h1>Practice Drills</h1>
    </div>
    {#if sourceStatus.kind === "live"}
      <span class="source-status">Pedals Active: {sourceStatus.device.name}</span>
    {:else}
      <span class="source-status error">Pedals disconnected</span>
    {/if}
  </header>

  <main class="content">
    {#if view.runState === "idle"}
      <div class="picker-panel panel">
        <h2>Select a Drill</h2>

        {#if errorMessage}
          <p class="error-message" role="alert">{errorMessage}</p>
        {/if}

        <div class="picker-controls">
          <label>
            Preset:
            <select
              bind:value={selectedPreset}
              onchange={() => (selectedDrill = selectedPreset?.drills[0] ?? null)}
            >
              {#each presets as p (p.id)}
                <option value={p}>{p.name}</option>
              {/each}
            </select>
          </label>

          {#if selectedPreset}
            <label>
              Drill:
              <select bind:value={selectedDrill}>
                {#each selectedPreset.drills as d (d.id)}
                  <option value={d}>{d.name} ({d.reps} reps)</option>
                {/each}
              </select>
            </label>
          {/if}
        </div>

        {#if selectedDrill}
          <div class="drill-info">
            <p><strong>Type:</strong> {selectedDrill.type}</p>
            <p><strong>Target Pedal:</strong> {selectedDrill.pedal}</p>
            {#if selectedDrill.type === "hold"}
              <p><strong>Target:</strong> {selectedDrill.target}%</p>
              <p><strong>Tolerance:</strong> &plusmn;{selectedDrill.tolerance}%</p>
              <p><strong>Hold Time:</strong> {selectedDrill.holdMs} ms</p>
            {/if}
            <p><strong>Reps:</strong> {selectedDrill.reps}</p>
          </div>

          <button class="btn-primary" onclick={start} disabled={sourceStatus.kind !== "live"}
            >Start Drill</button
          >
        {/if}
      </div>
    {:else}
      <div class="active-workspace">
        <div class="left-col">
          <div class="bars-container panel">
            {#if view.runState === "countdown"}
              <div class="overlay">
                <h2 class="countdown-text">Get Ready!</h2>
                <p class="countdown-timer">{(countdownMs / 1000).toFixed(1)}s</p>
              </div>
            {:else if view.runState === "finished"}
              <div class="overlay">
                <h2 class="finished-text">Set Finished!</h2>
                <button class="btn-primary mt" onclick={restart}>Play Again</button>
                <button class="btn-secondary mt" onclick={() => (view.runState = "idle")}
                  >Pick Another Drill</button
                >
              </div>
            {/if}

            {#if selectedDrill?.type === "hold"}
              <PedalBars
                stream={pedalStream}
                targetPedal={selectedDrill.pedal}
                targetVal={selectedDrill.target / 100}
                targetTolerance={selectedDrill.tolerance / 100}
              />
            {:else if selectedDrill?.type === "trace"}
              <!-- Trace not fully supported in PedalBars target yet -->
              <PedalBars stream={pedalStream} />
            {/if}
          </div>

          <div class="graph-container">
            <PedalGraph stream={pedalStream} band={graphBand} />
          </div>
        </div>

        <div class="side-panel">
          <div class="rep-info panel">
            <h3>Rep {view.currentRep + 1} / {selectedDrill?.reps}</h3>
            <p class="status-badge {view.runState}">{view.runState.toUpperCase()}</p>
            {#if errorMessage}
              <p class="error-message" role="alert">{errorMessage}</p>
            {/if}

            {#if view.runState !== "finished" && !aborting}
              <button class="btn-abort" onclick={abort}>Abort Set</button>
            {/if}
          </div>

          {#if view.lastScore?.kind === "trace"}
            <div class="score-card panel">
              <h3>Rep Result</h3>
              <div class="score-grade">
                <span class="total">{Math.round(view.lastScore.total)}</span>
                <span class="grade grade-{view.lastScore.grade}">{view.lastScore.grade}</span>
              </div>
            </div>
          {:else if view.lastScore}
            <div class="score-card panel">
              <h3>Rep Result</h3>
              <div class="score-grade">
                <span class="total">{Math.round(view.lastScore.total)}</span>
                <span class="grade grade-{view.lastScore.grade}">{view.lastScore.grade}</span>
              </div>
              <ul class="subscores">
                <li>Accuracy: {Math.round(view.lastScore.accuracy)}</li>
                <li>Timing: {Math.round(view.lastScore.timing)}</li>
                <li>Smoothness: {Math.round(view.lastScore.smoothness)}</li>
              </ul>
              <div class="metrics">
                <small>RMSE: {view.lastScore.rmse.toFixed(3)}</small>
                <small>Jitter: {view.lastScore.jitter.toFixed(3)}</small>
              </div>
            </div>
          {/if}

          {#if view.summary}
            <div class="summary-card panel">
              <h3>Set Summary</h3>
              <div class="summary-stats">
                <div><strong>Best:</strong> {Math.round(view.summary.best)}</div>
                <div>
                  <strong>Average:</strong>
                  {Math.round(view.summary.average)} ({view.summary.grade})
                </div>
                <div>
                  <strong>Consistency:</strong>
                  {view.summary.consistency === null
                    ? "n/a (one rep)"
                    : `${Math.round(view.summary.consistency)}%`}
                </div>
              </div>
              <h4>All Reps:</h4>
              <div class="rep-totals">
                {#each view.summary.repTotals as t, i (i)}
                  <span class="rep-pill">#{i + 1}: {Math.round(t)}</span>
                {/each}
              </div>
            </div>
          {:else if view.runState === "finished"}
            <div class="summary-card panel">
              <h3>Set Summary</h3>
              <p>No scored reps.</p>
            </div>
          {/if}
        </div>
      </div>
    {/if}
  </main>
</div>

<style>
  .drill-page {
    display: flex;
    flex-direction: column;
    min-height: 100vh;
    background: var(--bg);
    color: var(--text);
  }

  .app-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0.75rem 1.5rem;
    background: var(--surface);
    border-bottom: 1px solid var(--border);
  }

  .brand {
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
  }

  h1 {
    margin: 0;
    font-size: 1.125rem;
  }

  .source-status {
    font-size: 0.875rem;
    color: var(--text-muted);
  }
  .source-status.error {
    color: var(--brake);
  }

  .content {
    flex: 1;
    padding: 1.5rem;
    display: flex;
    gap: 1.5rem;
  }

  .panel {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 0.75rem;
    padding: 1.5rem;
  }

  .picker-panel {
    max-width: 32rem;
    margin: 0 auto;
    width: 100%;
  }

  .picker-controls {
    display: flex;
    flex-direction: column;
    gap: 1rem;
    margin-bottom: 1.5rem;
  }

  .picker-controls label {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    font-weight: 500;
  }

  .picker-controls select {
    padding: 0.5rem;
    border-radius: 0.375rem;
    border: 1px solid var(--border);
    background: var(--surface-raised);
    color: var(--text);
    font-size: 1rem;
  }

  .drill-info {
    background: var(--surface-raised);
    padding: 1rem;
    border-radius: 0.5rem;
    margin-bottom: 1.5rem;
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 0.5rem;
  }
  .drill-info p {
    margin: 0;
    font-size: 0.875rem;
  }

  .active-workspace {
    flex: 1;
    display: grid;
    grid-template-columns: 1fr 300px;
    gap: 1.5rem;
    max-width: 1200px;
    margin: 0 auto;
    width: 100%;
  }

  .bars-container {
    position: relative;
    padding: 0;
    overflow: hidden;
    display: flex;
    min-height: 400px;
  }

  .overlay {
    position: absolute;
    inset: 0;
    background: rgba(0, 0, 0, 0.6);
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    z-index: 10;
  }

  .countdown-text {
    font-size: 2rem;
    margin: 0;
    color: var(--text);
  }

  .countdown-timer {
    font-size: 4rem;
    font-weight: 700;
    margin: 0;
    color: var(--accent);
    font-variant-numeric: tabular-nums;
  }

  .finished-text {
    font-size: 2.5rem;
    color: var(--accent);
    margin: 0;
  }

  .side-panel {
    display: flex;
    flex-direction: column;
    gap: 1rem;
  }

  .rep-info h3 {
    margin: 0 0 0.5rem 0;
  }
  .status-badge {
    display: inline-block;
    padding: 0.25rem 0.5rem;
    border-radius: 999px;
    font-size: 0.75rem;
    font-weight: 700;
    background: var(--surface-raised);
  }
  .status-badge.active {
    background: var(--accent);
    color: #fff;
  }
  .status-badge.countdown {
    background: var(--throttle);
    color: #fff;
  }

  .score-card .score-grade {
    display: flex;
    align-items: baseline;
    gap: 1rem;
    margin-bottom: 1rem;
  }
  .score-card .total {
    font-size: 3rem;
    font-weight: 800;
  }
  .score-card .grade {
    font-size: 2rem;
    font-weight: 800;
  }
  .grade-S {
    color: #a855f7;
  }
  .grade-A {
    color: #22c55e;
  }
  .grade-B {
    color: #3b82f6;
  }
  .grade-C {
    color: #eab308;
  }
  .grade-D {
    color: #ef4444;
  }

  .subscores {
    list-style: none;
    padding: 0;
    margin: 0 0 1rem 0;
    font-size: 0.9rem;
  }

  .metrics {
    display: flex;
    justify-content: space-between;
    color: var(--text-muted);
  }

  .summary-stats {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    margin-bottom: 1rem;
  }

  .rep-totals {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem;
  }

  .rep-pill {
    background: var(--surface-raised);
    padding: 0.25rem 0.5rem;
    border-radius: 0.25rem;
    font-size: 0.8rem;
  }

  .btn-primary,
  .btn-secondary,
  .btn-abort {
    border: none;
    padding: 0.75rem 1.5rem;
    border-radius: 0.5rem;
    font-size: 1rem;
    font-weight: 600;
    cursor: pointer;
    width: 100%;
  }

  .btn-primary {
    background: var(--accent);
    color: #fff;
  }
  .btn-primary:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .btn-secondary {
    background: var(--surface-raised);
    color: var(--text);
  }

  .btn-abort {
    background: transparent;
    border: 1px solid var(--brake);
    color: var(--brake);
    margin-top: 1rem;
  }

  .left-col {
    display: flex;
    flex-direction: column;
    gap: 1.5rem;
    min-width: 0;
  }

  .graph-container {
    height: 18rem;
  }

  .error-message {
    margin: 0 0 1rem 0;
    color: var(--brake);
    font-size: 0.875rem;
  }

  .mt {
    margin-top: 1rem;
  }
</style>
