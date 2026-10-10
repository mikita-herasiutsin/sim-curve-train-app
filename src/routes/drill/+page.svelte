<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { resolve } from "$app/paths";
  import { page } from "$app/state";
  import { isTauri } from "@tauri-apps/api/core";
  import {
    loadLastPreset,
    saveLastPreset,
    loadTraceView,
    saveTraceView,
    type TraceViewMode,
  } from "$lib/settings";
  import {
    listPresets,
    startDrillRun,
    abortDrillRun,
    applyDrillEvent,
    playableDrills,
    toleranceOf,
    IDLE_VIEW,
    type Preset,
    type Drill,
    type DrillEvent,
    type RunView,
    type SetSummary,
    type Overlap,
  } from "$lib/drill";
  import { buildAttempt, saveAttempt, type ScoredRep } from "$lib/attempts";
  import { pedalStream } from "$lib/pedals/stream";
  import { startRealSource, type SourceStatus } from "$lib/pedals/realSource";
  import AudioControls from "$lib/components/AudioControls.svelte";
  import PedalBars from "$lib/components/PedalBars.svelte";
  import PedalGraph from "$lib/components/PedalGraph.svelte";
  import TraceView from "$lib/components/TraceView.svelte";
  import { traceDurationMs, TraceCurve, GO_LEAD_MS } from "$lib/pedals/traceDraw";
  import { formatPercentValue } from "$lib/pedals/geometry";
  import type { TargetBand } from "$lib/pedals/graphDraw";

  let presets = $state<Preset[]>([]);
  let selectedPreset = $state<Preset | null>(null);
  let selectedDrill = $state<Drill | null>(null);
  let traceView = $state<TraceViewMode>(loadTraceView());

  function setTraceView(mode: TraceViewMode): void {
    traceView = mode;
    saveTraceView(mode);
  }
  let presetsError = $state<string | null>(null);

  const leadIn = $derived(selectedDrill?.throttleLeadIn);

  const traceCurve = $derived<TraceCurve | null>(
    selectedDrill?.type === "trace" ? new TraceCurve(selectedDrill.points) : null,
  );

  let sourceStatus = $state<SourceStatus>({ kind: "connecting" });
  let stopSource: (() => void) | null = null;

  let view = $state<RunView>({ ...IDLE_VIEW });
  let countdownMs = $state(0);
  let throttleRemainingSec = $state(0);
  let showLiftCue = $state(false);
  let isGo = $state(false);
  // Time left to hold in the active rep, in ms (0..holdMs), refreshed every frame.
  let holdRemainingMs = $state(0);
  // Time left in the active trace rep, in ms (0..durationMs), refreshed every frame.
  let remainingMs = $state(0);
  let currentTargetFrac = $state<number | null>(null);
  let currentRangeFrac = $state<[number, number] | null>(null);
  let currentFrac = $state(0);
  // While remaining > GO_LEAD_MS show the number; while remaining <= GO_LEAD_MS show "GO".
  const countdownText = $derived(
    countdownMs > GO_LEAD_MS
      ? String(Math.max(1, Math.ceil((countdownMs - GO_LEAD_MS) / 1000)))
      : leadIn
        ? "THROTTLE"
        : "GO",
  );
  let errorMessage = $state<string | null>(null);
  // Set once the user aborts; the engine still ends the set with a final `setFinished`.
  let aborting = $state(false);

  // The target band on the graph, while a hold rep is active or during the GO second.
  const graphBand = $derived<TargetBand | null>(
    (view.runState === "active" ||
      (!leadIn && view.runState === "countdown" && countdownMs <= GO_LEAD_MS)) &&
      selectedDrill?.type === "hold"
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

  // What a run saves when its `setFinished` arrives. Not reactive: only the save reads it.
  interface RunRecord {
    presetId: string;
    drill: Drill;
    startedAt: string;
    scored: ScoredRep[];
    /** Reps that ended, failed ones included. */
    ended: number;
    saved: boolean;
  }

  let rafId: number | null = null;

  const METRIC_HELP: Record<string, string> = {
    accuracy: "Time inside the band and distance from target after you first reach it.",
    timing: "How fast you got into the band.",
    smoothness: "Overshoot beyond the band and shakiness.",
    avgError: "Average distance from the target, as a share of full pedal travel.",
    shakiness: "Small quick wobbles around your own average pedal position.",
  };

  onMount(() => {
    if (isTauri()) {
      listPresets()
        .then((p) => {
          // Only presets with something this screen can run are offered.
          presets = p.filter((preset) => playableDrills(preset).length > 0);
          if (presets.length > 0) {
            // The home picker opens this screen with ?preset=<id>&drill=<id>.
            const params = page.url.searchParams;
            const preset =
              presets.find((pr) => pr.id === (params.get("preset") ?? loadLastPreset())) ??
              presets[0];
            const playable = playableDrills(preset);
            selectedPreset = preset;
            selectedDrill =
              playable.find((d) => d.id === params.get("drill")) ?? playable[0] ?? null;
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
      const nowUs = pedalStream.dataNowUs();
      if (view.runState === "countdown" && view.countdownEndsUs > 0) {
        const remainingUs = view.countdownEndsUs - nowUs;
        const ms = Math.max(0, Math.ceil(remainingUs / 1000));
        countdownMs = Math.ceil(ms / 100) * 100;
      } else if (view.runState === "active" && selectedDrill?.type === "hold") {
        const endUs = view.repStartUs + selectedDrill.holdMs * 1000;
        const ms = Math.min(selectedDrill.holdMs, Math.max(0, Math.ceil((endUs - nowUs) / 1000)));
        holdRemainingMs = Math.ceil(ms / 100) * 100;
      }

      if (view.runState === "throttle") {
        if (view.throttleHoldEndsUs > 0) {
          throttleRemainingSec = Math.max(0, (view.throttleHoldEndsUs - nowUs) / 1e6);
        } else {
          throttleRemainingSec = 0;
        }
      }

      showLiftCue = Boolean(
        leadIn &&
        view.runState === "active" &&
        view.repStartUs > 0 &&
        nowUs - view.repStartUs < 800_000,
      );

      const goCountingDown = leadIn
        ? view.runState === "throttle" && view.throttleHoldEndsUs > 0
        : view.runState === "countdown";
      const goEndsUs = leadIn ? view.throttleHoldEndsUs : view.countdownEndsUs;
      isGo = goCountingDown && goEndsUs > 0 && goEndsUs - nowUs <= GO_LEAD_MS * 1000;

      if (selectedDrill?.type === "trace" && traceCurve) {
        const durationMs = traceCurve.durationMs;
        if (view.runState === "active") {
          const repMs = Math.max(0, Math.min(durationMs, (nowUs - view.repStartUs) / 1000));
          currentTargetFrac = traceCurve.valueAt(repMs);
          currentRangeFrac = traceCurve.envelopeAt(repMs);
          const latest = pedalStream.history.latest();
          currentFrac =
            latest && selectedDrill.pedal !== "clutch" ? latest[selectedDrill.pedal] : 0;
          const rem = durationMs - repMs;
          remainingMs = Math.ceil(rem / 100) * 100;
        } else if (isGo) {
          const playheadMs = goEndsUs > 0 ? (nowUs - goEndsUs) / 1000 : -GO_LEAD_MS;
          currentTargetFrac = traceCurve.valueAt(playheadMs);
          currentRangeFrac = traceCurve.envelopeAt(playheadMs);
          const latest = pedalStream.history.latest();
          currentFrac =
            latest && selectedDrill.pedal !== "clutch" ? latest[selectedDrill.pedal] : 0;
          remainingMs = durationMs;
        } else {
          currentTargetFrac = null;
          currentRangeFrac = null;
          currentFrac = 0;
          remainingMs = durationMs;
        }
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
    saveLastPreset(selectedPreset.id);
    if (sourceStatus.kind !== "live") {
      errorMessage = "Connect your pedals before starting.";
      return;
    }

    errorMessage = null;
    aborting = false;
    const id = ++runId;
    const record: RunRecord = {
      presetId: selectedPreset.id,
      drill: selectedDrill,
      startedAt: new Date().toISOString(),
      scored: [],
      ended: 0,
      saved: false,
    };
    // The engine ends every run with `setFinished`: after the last rep, on abort, and when the
    // stream stops. The record is saved then, even for a run the screen has given up on.
    const onEvent = (e: DrillEvent) => {
      if (e.event === "repScored") {
        record.scored.push({ rep: e.rep, score: e.score });
        record.ended++;
      } else if (e.event === "repFailed") {
        record.ended++;
      }
      if (id === runId) view = applyDrillEvent(view, e);
      if (e.event === "setFinished") persist(record, e.summary);
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
      // No stream to ask for a final event, so give the run up here. The engine's own
      // `setFinished` still saves it.
      runId++;
      view = { ...IDLE_VIEW };
      return;
    }
    // The run stays current: its final `setFinished` shows the summary and saves the set.
    aborting = true;
    abortDrillRun(sourceStatus.token).catch((e) => {
      console.error(e);
      aborting = false;
      errorMessage = `Failed to abort drill: ${e}`;
    });
  }

  /**
   * Saves a set once. A set that ended before all its reps (Abort, leaving the page, pedals
   * unplugged) is saved as aborted; one that ended before any rep is not saved. A failed save
   * is logged and shown, and the drill screen keeps working.
   */
  function persist(record: RunRecord, summary: SetSummary | null) {
    if (record.saved || record.ended === 0) return;
    record.saved = true;
    const attempt = buildAttempt({
      drillId: record.drill.id,
      presetId: record.presetId,
      pedal: record.drill.pedal,
      startedAt: record.startedAt,
      aborted: record.ended < record.drill.reps,
      summary,
      scored: record.scored,
    });
    saveAttempt(attempt).catch((e) => {
      console.error("Failed to save attempt", e);
      errorMessage = `This set was not saved: ${e}`;
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
              onchange={() => {
                if (selectedPreset) {
                  saveLastPreset(selectedPreset.id);
                  selectedDrill = playableDrills(selectedPreset)[0] ?? null;
                } else {
                  selectedDrill = null;
                }
              }}
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

        {#if selectedDrill}
          <div class="drill-info">
            <p><strong>Type:</strong> {selectedDrill.type}</p>
            <p><strong>Target Pedal:</strong> {selectedDrill.pedal}</p>
            {#if selectedDrill.type === "hold"}
              <p>
                <strong>Target:</strong>
                {selectedDrill.target.toFixed(selectedDrill.decimals ?? 0)}%
              </p>
              <p><strong>Tolerance:</strong> &plusmn;{toleranceOf(selectedDrill)}%</p>
              <p><strong>Hold Time:</strong> {selectedDrill.holdMs} ms</p>
            {:else if selectedDrill.type === "trace"}
              {@const peak = selectedDrill.points.reduce((max, p) => Math.max(max, p[1]), 0)}
              <p>
                <strong>Duration:</strong>
                {(traceDurationMs(selectedDrill.points) / 1000).toFixed(1)} s
              </p>
              <p>
                <strong>Peak:</strong>
                {formatPercentValue(peak / 100, selectedDrill.decimals ?? 0)}%
              </p>
              <p><strong>Tolerance:</strong> &plusmn;{toleranceOf(selectedDrill)}%</p>
              <p class="view-row"><strong>View:</strong> {@render traceViewToggle()}</p>
            {/if}
            {#if leadIn}
              <p data-testid="lead-in-info">
                <strong>Starts from throttle:</strong>
                {leadIn.level}% for {leadIn.holdMs / 1000}s, then lift and brake
              </p>
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
                {#key countdownText}
                  <p class="countdown-number">{countdownText}</p>
                {/key}
              </div>
            {:else if view.runState === "throttle"}
              <div class="overlay" aria-live="assertive" data-testid="throttle-cue">
                <p class="countdown-label">Throttle to {leadIn?.level}%</p>
                <p class="countdown-number" class:text-cue={view.throttleHoldEndsUs <= 0}>
                  {view.throttleHoldEndsUs > 0
                    ? throttleRemainingSec.toFixed(1)
                    : "Press the throttle"}
                </p>
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

            {#if showLiftCue}
              <p class="lift-cue" data-testid="lift-cue">LIFT</p>
            {/if}

            {#if selectedDrill?.type === "hold"}
              <PedalBars
                stream={pedalStream}
                targetPedal={view.runState === "throttle" && leadIn
                  ? "throttle"
                  : selectedDrill.pedal}
                targetVal={view.runState === "throttle" && leadIn
                  ? leadIn.level / 100
                  : selectedDrill.target / 100}
                targetTolerance={view.runState === "throttle" && leadIn
                  ? 0.1
                  : toleranceOf(selectedDrill) / 100}
                targetRange={null}
                decimals={selectedDrill.decimals ?? 0}
              />
            {:else if selectedDrill?.type === "trace"}
              <PedalBars
                stream={pedalStream}
                targetPedal={view.runState === "throttle" && leadIn
                  ? "throttle"
                  : selectedDrill.pedal}
                targetVal={view.runState === "throttle" && leadIn
                  ? leadIn.level / 100
                  : currentTargetFrac}
                targetRange={view.runState === "throttle" && leadIn ? null : currentRangeFrac}
                targetTolerance={view.runState === "throttle" && leadIn
                  ? 0.1
                  : toleranceOf(selectedDrill) / 100}
                decimals={selectedDrill.decimals ?? 0}
              />
            {/if}
          </div>

          <div class="graph-container">
            {#if selectedDrill?.type === "trace" && traceCurve}
              <TraceView
                drill={selectedDrill}
                curve={traceCurve}
                repStartUs={view.repStartUs}
                active={view.runState === "active"}
                countdownEndsUs={leadIn ? view.throttleHoldEndsUs : view.countdownEndsUs}
                countingDown={leadIn
                  ? view.runState === "throttle" && view.throttleHoldEndsUs > 0
                  : view.runState === "countdown"}
                mode={traceView}
              />
            {:else}
              <PedalGraph stream={pedalStream} band={graphBand} />
            {/if}
          </div>
        </div>

        <div class="side-panel">
          {#if view.runState === "active" && selectedDrill?.type === "hold"}
            <div class="hold-hud-card panel" data-testid="hold-hud">
              <p class="hold-time">Hold <span>{(holdRemainingMs / 1000).toFixed(1)}s</span></p>
              <p class="hold-target">
                Target {selectedDrill.target.toFixed(selectedDrill.decimals ?? 0)}% &plusmn;{toleranceOf(
                  selectedDrill,
                )}%
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
          {:else if (view.runState === "active" || isGo) && selectedDrill?.type === "trace"}
            {@const durationMs = traceCurve ? traceCurve.durationMs : 0}
            <div class="trace-hud-card panel" data-testid="trace-hud">
              <div class="trace-numbers">
                <p class="trace-target">
                  Target <span
                    >{formatPercentValue(
                      currentTargetFrac ?? 0,
                      selectedDrill.decimals ?? 0,
                    )}%</span
                  >
                </p>
                <p class="trace-current">
                  You <span>{formatPercentValue(currentFrac, selectedDrill.decimals ?? 0)}%</span>
                </p>
              </div>
              <p class="hold-time"><span>{(remainingMs / 1000).toFixed(1)}s</span></p>
              <div
                class="hold-progress"
                role="progressbar"
                aria-label="Trace time left"
                aria-valuemin={0}
                aria-valuemax={durationMs}
                aria-valuenow={remainingMs}
              >
                <span style:width="{durationMs > 0 ? (remainingMs / durationMs) * 100 : 0}%"></span>
              </div>
            </div>
          {/if}

          <div class="rep-info panel">
            <h3>Rep {view.currentRep + 1} / {selectedDrill?.reps}</h3>
            <p class="status-badge {view.runState}">{view.runState.toUpperCase()}</p>
            {#if selectedDrill?.type === "trace"}
              <p class="view-row rep-view-row">
                <strong>View:</strong>
                {@render traceViewToggle()}
              </p>
            {/if}
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
              <ul class="subscores">
                <li>Accuracy: {Math.round(view.lastScore.accuracy)}</li>
                <li>Timing: {Math.round(view.lastScore.timing)}</li>
                <li>Smoothness: {Math.round(view.lastScore.smoothness)}</li>
              </ul>
              <div class="metrics">
                <small
                  title="How late (positive) or early (negative) you followed the curve, in ms."
                  >Lag {Math.round(view.lastScore.lagMs)} ms</small
                >
                <small title="Share of the rep your pedal was inside the band."
                  >In band {Math.round(view.lastScore.timeInBand * 100)}%</small
                >
                <small title="Average distance outside the band, as a share of full pedal travel."
                  >Off band ±{(view.lastScore.rmse * 100).toFixed(1)}%</small
                >
              </div>
              {#if view.lastOverlap}
                {@render overlapBlock(view.lastOverlap)}
              {/if}
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
              {#if view.lastOverlap}
                {@render overlapBlock(view.lastOverlap)}
              {/if}
            </div>
          {:else if view.lastOverlap}
            <div class="panel">
              {@render overlapBlock(view.lastOverlap)}
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

{#snippet overlapBlock(overlap: Overlap)}
  <div class="overlap" class:clean={Math.round(overlap.overlapMs) === 0} data-testid="overlap">
    <small title="Time both pedals were pressed at once, during the throttle hold and the rep."
      >Overlap {Math.round(overlap.overlapMs)} ms</small
    >
    <small title="Highest throttle while the brake was pressed."
      >Peak throttle while braking {Math.round(overlap.peakThrottle * 100)}%</small
    >
  </div>
{/snippet}

{#snippet traceViewToggle()}
  <span class="view-toggle" role="group" aria-label="Trace view">
    {#each [["playhead", "Playhead"], ["ghost", "Ghost"]] as const as [mode, label] (mode)}
      <button
        type="button"
        class:selected={traceView === mode}
        aria-pressed={traceView === mode}
        onclick={() => setTraceView(mode)}>{label}</button
      >
    {/each}
  </span>
{/snippet}

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
  .countdown-number.text-cue {
    font-size: clamp(2rem, 5vw, 4rem);
  }

  .lift-cue {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    pointer-events: none;
    font-size: 5rem;
    font-weight: 800;
    color: var(--accent);
    margin: 0;
    z-index: 10;
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

  .hold-hud-card,
  .trace-hud-card {
    text-align: center;
  }

  .trace-numbers {
    display: flex;
    justify-content: space-around;
    align-items: baseline;
    margin: 0.5rem 0;
  }

  .trace-target,
  .trace-current {
    margin: 0;
    font-size: 0.875rem;
    color: var(--text-muted);
    font-weight: 600;
  }

  .trace-target span,
  .trace-current span {
    font-size: 1.75rem;
    font-weight: 700;
    color: var(--text);
    font-variant-numeric: tabular-nums;
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
  .status-badge.countdown,
  .status-badge.throttle {
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

  .overlap {
    display: flex;
    flex-wrap: wrap;
    gap: 0.25rem 1rem;
    margin-top: 0.75rem;
    color: var(--text-muted);
  }
  .overlap small {
    white-space: nowrap;
  }
  .overlap.clean {
    color: var(--throttle);
  }
  .panel > .overlap:only-child {
    margin-top: 0;
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

  .view-row {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }

  .view-toggle {
    display: inline-flex;
    border: 1px solid var(--border);
    border-radius: 0.5rem;
    overflow: hidden;
    background: var(--surface);
  }

  .view-toggle button {
    border: none;
    border-radius: 0;
    padding: 0.25rem 0.625rem;
    font-size: 0.8125rem;
    background: transparent;
    color: var(--text-muted);
    cursor: pointer;
  }

  .view-toggle button.selected {
    background: var(--accent);
    color: var(--surface);
  }

  /* Let the canvases shrink with their panel instead of keeping their own 18rem floor. */
  .bars-container :global(.pedal-bars-container),
  .graph-container :global(.pedal-graph-container),
  .graph-container :global(.trace-view-container) {
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
