<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { scrollIntoViewSoon } from "$lib/scroll";
  import type { DeviceStream } from "$lib/stream";
  import { normaliseRaw } from "$lib/stream";
  import {
    WIZARD_STEPS,
    assignedAxes,
    detectAxis,
    isBackAtRest,
    pedalLabel,
    type Assignments,
    type PedalName,
    unassign,
  } from "$lib/wizard";

  let {
    stream,
    axisCount,
    assignments = $bindable({}),
    onchange,
  }: {
    stream: DeviceStream;
    axisCount: number;
    assignments?: Assignments;
    /** Called with the final assignments when the wizard finishes or an axis is overridden. */
    onchange?: (assignments: Assignments) => void;
  } = $props();

  /** How often the wizard asks Rust for a detection while a step is active. */
  const POLL_MS = 200;
  /**
   * With no movement for this long, the detection window restarts. Rust keeps only 10 s of
   * samples, and the window must still start at a sample taken with the pedal released.
   */
  const IDLE_RESTART_US = 5_000_000;

  let stepIndex = $state<number | null>(null);
  let message = $state("");
  let timer: ReturnType<typeof setInterval> | undefined;
  let sinceUs = 0;
  let polling = false;
  // Bumped whenever the step changes, so a poll that was in flight can tell its result is stale.
  let stepToken = 0;

  const step = $derived(stepIndex === null ? null : WIZARD_STEPS[stepIndex]);

  // Assignments from before "Detect pedals", restored on Cancel (the saved profile stays as is).
  let beforeWizard: Assignments = {};
  let sectionEl = $state<HTMLElement | null>(null);

  function start() {
    beforeWizard = assignments;
    assignments = {};
    beginStep(0);
    // Bring the instructions and the axis bars below them into view.
    void scrollIntoViewSoon(() => sectionEl?.parentElement);
  }

  function beginStep(index: number) {
    stopTimer();
    stepToken += 1;
    if (index >= WIZARD_STEPS.length) {
      stepIndex = null;
      message = "Done. Check the assignments below.";
      onchange?.(assignments);
      return;
    }
    stepIndex = index;
    message = "";
    // The detector's baseline is the first sample after this point, so the pedals must be at
    // rest now. Samples share Rust's clock, so take the start time from the stream.
    sinceUs = stream.latest?.tUs ?? 0;
    const token = stepToken;
    timer = setInterval(() => void poll(index, token), POLL_MS);
  }

  async function poll(index: number, token: number) {
    if (polling) return;
    polling = true;
    const { pedal } = WIZARD_STEPS[index];
    try {
      const result = await detectAxis(sinceUs, assignedAxes(assignments, pedal));
      // Skipped, restarted or cancelled while Rust was answering.
      if (token !== stepToken) return;
      const latestUs = stream.latest?.tUs ?? sinceUs;
      if (result.kind === "noMovement" && latestUs - sinceUs > IDLE_RESTART_US) {
        // Nothing moved past the threshold, so the pedal is still (near) released.
        sinceUs = latestUs;
      } else if (result.kind === "ambiguous") {
        message = `Several axes moved (${result.candidates.join(", ")}). Press only the ${pedal}.`;
        sinceUs = stream.latest?.tUs ?? sinceUs;
      } else if (result.kind === "axis") {
        const raw = stream.latest?.axes[result.index];
        if (raw !== undefined && isBackAtRest(result, raw)) {
          const { index: axis, rest, min, max } = result;
          assignments = { ...assignments, [pedal]: { axis, rest, min, max } };
          beginStep(index + 1);
        } else {
          message = `Axis ${result.index} moving. Now release the ${pedal}.`;
        }
      }
    } catch (e: unknown) {
      message = `Detection failed: ${String(e)}`;
      stopTimer();
    } finally {
      polling = false;
    }
  }

  function skip() {
    if (stepIndex === null) return;
    assignments = unassign(assignments, WIZARD_STEPS[stepIndex].pedal);
    beginStep(stepIndex + 1);
  }

  function override(pedal: PedalName, value: string) {
    if (value === "") {
      assignments = unassign(assignments, pedal);
      onchange?.(assignments);
      return;
    }
    // Manual choice: no sweep data yet, so assume the usual 'rest at minimum' range.
    // Calibration (SCT-014) captures the real range.
    const axis = Number(value);
    const raw = stream.latest?.axes[axis] ?? -32768;
    assignments = { ...assignments, [pedal]: { axis, rest: raw, min: -32768, max: 32767 } };
    onchange?.(assignments);
  }

  function cancel() {
    stopTimer();
    stepToken += 1;
    stepIndex = null;
    assignments = beforeWizard;
    message = "Cancelled. Nothing was changed.";
  }

  function stopTimer() {
    clearInterval(timer);
    timer = undefined;
  }

  // Live input bars: one rAF loop writes straight to the DOM, so no reactive churn per sample.
  const fills: Partial<Record<PedalName, HTMLElement>> = {};
  const labels: Partial<Record<PedalName, HTMLElement>> = {};
  const lastRenderedPct: Partial<Record<PedalName, number>> = {};
  let frame = 0;

  function paint() {
    frame = requestAnimationFrame(paint);
    const latest = stream.latest;
    for (const { pedal } of WIZARD_STEPS) {
      const a = assignments[pedal];
      const fill = fills[pedal];
      const label = labels[pedal];
      if (!a || !fill || !label) continue;
      const raw = latest?.axes[a.axis];
      if (raw === undefined) continue;
      const pct = normaliseRaw(raw) * 100;
      const pctInt = Math.round(pct);
      // Only update DOM if the integer percentage changed
      if (lastRenderedPct[pedal] !== pctInt) {
        lastRenderedPct[pedal] = pctInt;
        fill.style.width = `${pct}%`;
        label.textContent = `${pctInt}%`;
        fill.parentElement?.setAttribute("aria-valuenow", pctInt.toString());
      }
    }
  }

  onMount(() => {
    frame = requestAnimationFrame(paint);
  });

  onDestroy(() => {
    stopTimer();
    cancelAnimationFrame(frame);
  });
</script>

<section class="wizard" aria-label="Pedal setup" bind:this={sectionEl}>
  {#if step}
    <p class="prompt" aria-live="polite">
      Press <strong>{step.pedal}</strong> fully and release.
    </p>
    <p class="muted" aria-live="polite">{message || "Waiting for movement…"}</p>
    <div class="actions">
      {#if step.optional}
        <button type="button" onclick={skip}>Skip {step.pedal}</button>
      {/if}
      <button type="button" onclick={() => beginStep(stepIndex ?? 0)}>Restart step</button>
      <button type="button" onclick={cancel}>Cancel</button>
    </div>
  {:else}
    <div class="actions">
      <button type="button" class="primary" onclick={start}>Detect pedals</button>
    </div>
    {#if message}<p class="muted" aria-live="polite">{message}</p>{/if}
  {/if}

  <table>
    <tbody>
      {#each WIZARD_STEPS as { pedal } (pedal)}
        <tr>
          <th scope="row">{pedalLabel(pedal)}</th>
          <td>
            <select
              aria-label="{pedalLabel(pedal)} axis"
              value={assignments[pedal]?.axis.toString() ?? ""}
              onchange={(e) => override(pedal, e.currentTarget.value)}
              disabled={step !== null}
            >
              <option value="">Not assigned</option>
              {#each { length: axisCount }, axis (axis)}
                <option value={axis.toString()}>Axis {axis}</option>
              {/each}
            </select>
          </td>
          <td>
            {#if assignments[pedal]}
              <div class="live">
                <span
                  class="bar"
                  role="meter"
                  aria-label="{pedalLabel(pedal)} live input"
                  aria-valuemin={0}
                  aria-valuemax={100}
                  aria-valuenow={0}
                >
                  <span class="fill" bind:this={fills[pedal]}></span>
                </span>
                <span class="pct" bind:this={labels[pedal]}>0%</span>
              </div>
            {/if}
          </td>
        </tr>
      {/each}
    </tbody>
  </table>
</section>

<style>
  .wizard {
    display: grid;
    gap: 0.75rem;
    padding: 1.25rem;
    border: 1px solid var(--border);
    border-radius: 0.75rem;
    background: var(--surface);
  }

  .prompt {
    margin: 0;
    font-size: 1.25rem;
  }

  .prompt strong {
    text-transform: uppercase;
    color: var(--accent);
  }

  .muted {
    margin: 0;
    color: var(--text-muted);
  }

  .actions {
    display: flex;
    gap: 0.5rem;
  }

  button,
  select {
    padding: 0.375rem 0.75rem;
    border: 1px solid var(--border);
    border-radius: 0.5rem;
    background: var(--surface-raised);
    color: var(--text);
    font: inherit;
    font-size: 0.875rem;
  }

  button {
    cursor: pointer;
  }

  button.primary {
    border-color: var(--accent);
    background: color-mix(in srgb, var(--accent) 25%, transparent);
  }

  table {
    border-collapse: collapse;
  }

  td .live {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding-left: 0.75rem;
  }

  .bar {
    display: inline-block;
    width: 10rem;
    height: 0.75rem;
    border-radius: 999px;
    background: var(--surface-raised);
    border: 1px solid var(--border);
    overflow: hidden;
  }

  .fill {
    display: block;
    height: 100%;
    width: 0;
    background: var(--accent);
  }

  .pct {
    min-width: 4.5ch;
    text-align: right;
    font-variant-numeric: tabular-nums;
    color: var(--text-muted);
    font-size: 0.875rem;
  }

  th {
    padding: 0.25rem 1rem 0.25rem 0;
    color: var(--text-muted);
    font-weight: 600;
    text-align: left;
  }
</style>
