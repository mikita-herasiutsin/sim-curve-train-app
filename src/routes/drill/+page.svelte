<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { resolve } from "$app/paths";
  import { isTauri } from "@tauri-apps/api/core";
  import {
    listPresets,
    startDrillRun,
    abortDrillRun,
    applyDrillEvent,
    playableDrills,
    toleranceOf,
    IDLE_VIEW,
    type Preset,
    type HoldDrill,
    type DrillEvent,
    type RunView,
  } from "$lib/drill";
  import { pedalStream } from "$lib/pedals/stream";
  import { startRealSource, type SourceStatus } from "$lib/pedals/realSource";
  import AudioControls from "$lib/components/AudioControls.svelte";
  import PedalBars from "$lib/components/PedalBars.svelte";
  import PedalGraph from "$lib/components/PedalGraph.svelte";
  import type { TargetBand } from "$lib/pedals/graphDraw";

  let presets = $state<Preset[]>([]);
  let selectedPreset = $state<Preset | null>(null);
  let selectedDrill = $state<HoldDrill | null>(null);
  let presetsError = $state<string | null>(null);

  let sourceStatus = $state<SourceStatus>({ kind: "connecting" });
  let stopSource: (() => void) | null = null;

  let view = $state<RunView>({ ...IDLE_VIEW });
  let countdownMs = $state(0);
  // Time left to hold in the active rep, in ms (0..holdMs), refreshed every frame.
  let holdRemainingMs = $state(0);
  // "GO!" is shown briefly when a rep becomes active.
  let showGo = $state(false);
  let goTimer: ReturnType<typeof setTimeout> | undefined;
  const GO_MS = 600;
  // 3, 2, 1 for a 3000 ms lead-in; clamped so a late frame never flashes 0.
  const countdownSeconds = $derived(Math.max(1, Math.ceil(countdownMs / 1000)));
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
          tolerance: toleranceOf(selectedDrill) / 100,
        }
      : null,
  );
  // Bumped on every start and whenever a run is given up, so late events of a replaced or
  // dead run are ignored. An aborted run keeps its id: its final `setFinished` ends the set.
  let runId = 0;

  let rafId: number | null = null;

  const METRIC_HELP: Record<string, string> = {
    accuracy: "Time inside the band and distance from target after you first reach it.",
    timing: "How fast you got into the band.",
    smoothness: "Overshoot beyond the band and shakiness.",
    avgError: "Average distance from the target, as a share of full pedal travel.",
    shakiness: "Small quick wobbles around your own average pedal position.",
  };

  // Show "GO!" for a moment each time a rep becomes active.
  let lastRunState = "idle";
  $effect(() => {
    const state = view.runState;
    if (state === "active" && lastRunState !== "active") {
      showGo = true;
      holdRemainingMs = selectedDrill?.type === "hold" ? selectedDrill.holdMs : 0;
      clearTimeout(goTimer);
      goTimer = setTimeout(() => (showGo = false), GO_MS);
    } else if (state !== "active") {
      showGo = false;
      clearTimeout(goTimer);
    }
    lastRunState = state;
  });

  onMount(() => {
    if (isTauri()) {
      listPresets()
        .then((p) => {
          // Only presets with something this screen can run are offered.
          presets = p.filter((preset) => playableDrills(preset).length > 0);
          if (presets.length > 0) {
            selectedPreset = presets[0];
            selectedDrill = playableDrills(presets[0])[0] ?? null;
          } else {
            presetsError = "No playable drills found.";
          }
        })
        .catch((e) => {
          console.error("Failed to load presets", e);
          presetsError = `Failed to load drills: ${e}`;
        });

      stopSource = startRealSource(pedalStream, (s) => {
        sourceStatus = s;
      });
    }

    const loop = () => {
      if (view.runState === "countdown" && view.countdownEndsUs > 0) {
        const remainingUs = view.countdownEndsUs - pedalStream.dataNowUs();
        const ms = Math.max(0, Math.ceil(remainingUs / 1000));
        countdownMs = Math.ceil(ms / 100) * 100;
      } else if (view.runState === "active" && selectedDrill?.type === "hold") {
        const endUs = view.repStartUs + selectedDrill.holdMs * 1000;
        const ms = Math.min(
          selectedDrill.holdMs,
          Math.max(0, Math.ceil((endUs - pedalStream.dataNowUs()) / 1000)),
        );
        holdRemainingMs = Math.ceil(ms / 100) * 100;
      }
      rafId = requestAnimationFrame(loop);
    };
    rafId = requestAnimationFrame(loop);
  });

  onDestroy(() => {
    clearTimeout(goTimer);
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
      runId++;
      view = { ...IDLE_VIEW };
      errorMessage = `Failed to start drill: ${e}`;
    });
    view = { ...IDLE_VIEW, runState: "countdown" };
    countdownMs = selectedDrill.leadInMs;
    // countdownEndsUs is set by the first event
  }

  function abort() {
    if (sourceStatus.kind !== "live") {
      // No stream to ask for a final event, so give the run up here.
      runId++;
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
    <div class="header-right">
      <AudioControls />
      {#if sourceStatus.kind === "live"}
        <span class="source-status">Pedals Active: {sourceStatus.device.name}</span>
      {:else}
        <span class="source-status error">Pedals disconnected</span>
      {/if}
    </div>
  </header>

  <main class="content">
    {#if view.runState === "idle"}
      <div class="picker-panel panel">
        <h2>Select a Drill</h2>

        {#if errorMessage}
          <p class="error-message" role="alert">{errorMessage}</p>
        {/if}

        {#if presetsError}
          <p class="error-message" role="alert">{presetsError}</p>
        {/if}

        <div class="picker-controls">
          <label>
            Preset:
            <select
              bind:value={selectedPreset}
              onchange={() =>
                (selectedDrill = selectedPreset
                  ? (playableDrills(selectedPreset)[0] ?? null)
                  : null)}
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
                {#each playableDrills(selectedPreset) as d (d.id)}
                  <option value={d}>{d.name} ({d.reps} reps)</option>
                {/each}
              </select>
            </label>
          {/if}
        </div>
        <p class="note">Trace drills arrive with SCT-034.</p>

        {#if selectedDrill}
          <div class="drill-info">
            <p><strong>Type:</strong> {selectedDrill.type}</p>
            <p><strong>Target Pedal:</strong> {selectedDrill.pedal}</p>
            {#if selectedDrill.type === "hold"}
              <p><strong>Target:</strong> {selectedDrill.target}%</p>
              <p><strong>Tolerance:</strong> &plusmn;{toleranceOf(selectedDrill)}%</p>
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
              <div class="overlay" aria-live="assertive">
                <p class="countdown-label">Get ready</p>
                {#key countdownSeconds}
                  <p class="countdown-number">{countdownSeconds}</p>
                {/key}
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
                targetTolerance={toleranceOf(selectedDrill) / 100}
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
          {#if view.runState === "active" && selectedDrill?.type === "hold"}
            <div class="hold-hud-card panel" data-testid="hold-hud">
              {#if showGo}
                <p class="go-label">GO!</p>
              {/if}
              <p class="hold-time">Hold <span>{(holdRemainingMs / 1000).toFixed(1)}s</span></p>
              <p class="hold-target">
                Target {selectedDrill.target}% &plusmn;{toleranceOf(selectedDrill)}%
              </p>
              <div
                class="hold-progress"
                role="progressbar"
                aria-label="Hold time left"
                aria-valuemin={0}
                aria-valuemax={selectedDrill.holdMs}
                aria-valuenow={holdRemainingMs}
              >
                <span style:width="{(holdRemainingMs / selectedDrill.holdMs) * 100}%"></span>
              </div>
            </div>
          {/if}

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
                <li title={METRIC_HELP.accuracy} aria-describedby="help-accuracy">
                  Accuracy: {Math.round(view.lastScore.accuracy)}
                </li>
                <li title={METRIC_HELP.timing} aria-describedby="help-timing">
                  Timing: {Math.round(view.lastScore.timing)}
                </li>
                <li title={METRIC_HELP.smoothness} aria-describedby="help-smoothness">
                  Smoothness: {Math.round(view.lastScore.smoothness)}
                </li>
              </ul>
              <div class="metrics">
                <small title={METRIC_HELP.avgError} aria-describedby="help-avgError"
                  >Avg error ±{(view.lastScore.rmse * 100).toFixed(1)}%</small
                >
                <small title={METRIC_HELP.shakiness} aria-describedby="help-shakiness"
                  >Shakiness {(view.lastScore.jitter * 100).toFixed(1)}%</small
                >
              </div>
              <details class="metric-help">
                <summary>What do these mean?</summary>
                <ul>
                  {#each [["accuracy", "Accuracy"], ["timing", "Timing"], ["smoothness", "Smoothness"], ["avgError", "Avg error"], ["shakiness", "Shakiness"]] as [key, label] (key)}
                    <li id="help-{key}"><strong>{label}:</strong> {METRIC_HELP[key]}</li>
                  {/each}
                </ul>
              </details>
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
                {#each view.reps as r (r.rep)}
                  <span class="rep-pill"
                    >#{r.rep + 1}: {r.total === null ? "failed" : Math.round(r.total)}</span
                  >
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
    height: 100vh;
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

  .header-right {
    display: flex;
    align-items: center;
    gap: 1rem;
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
    min-height: 0;
    overflow-y: auto;
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
    min-height: 0;
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
    flex: 1.2 1 0;
    min-height: 0;
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

  .countdown-label {
    margin: 0;
    font-size: 1.25rem;
    color: var(--text-muted);
    text-transform: uppercase;
    letter-spacing: 0.1em;
  }

  .countdown-number {
    font-size: 9rem;
    line-height: 1;
    font-weight: 800;
    margin: 0;
    color: var(--accent);
    font-variant-numeric: tabular-nums;
    animation: pop-in 0.45s ease-out;
  }

  @keyframes pop-in {
    from {
      transform: scale(1.8);
      opacity: 0;
    }
    to {
      transform: scale(1);
      opacity: 1;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .countdown-number {
      animation: none;
    }
  }

  .hold-hud-card {
    text-align: center;
  }

  .go-label {
    margin: 0;
    font-size: 3rem;
    font-weight: 800;
    color: #22c55e;
  }

  .hold-time {
    margin: 0;
    font-size: 1.25rem;
    font-weight: 700;
  }
  .hold-time span {
    font-size: 1.75rem;
    color: var(--accent);
    font-variant-numeric: tabular-nums;
  }
  .hold-target {
    margin: 0 0 0.375rem;
    color: var(--text-muted);
    font-size: 0.875rem;
  }
  .hold-progress {
    height: 0.375rem;
    border-radius: 999px;
    background: var(--surface-raised);
    overflow: hidden;
  }
  .hold-progress span {
    display: block;
    height: 100%;
    background: var(--accent);
  }

  .metric-help {
    margin-top: 0.75rem;
    font-size: 0.8rem;
    color: var(--text-muted);
  }
  .metric-help ul {
    padding-left: 1rem;
    margin: 0.5rem 0 0;
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
    min-height: 0;
    overflow-y: auto;
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
    gap: 1rem;
    min-width: 0;
    min-height: 0;
  }

  /* The graph takes the height the bars leave over, but never less than 160px. */
  .graph-container {
    flex: 1 1 0;
    min-height: 160px;
  }

  /* Let the canvases shrink with their panel instead of keeping their own 18rem floor. */
  .bars-container :global(.pedal-bars-container),
  .graph-container :global(.pedal-graph-container) {
    min-height: 0;
  }

  .note {
    margin: 0 0 1rem 0;
    color: var(--text-muted);
    font-size: 0.8125rem;
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
