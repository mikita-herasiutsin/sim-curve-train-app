<script lang="ts">
  import { onDestroy } from "svelte";
  import type { DeviceStream } from "$lib/stream";
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
  }: { stream: DeviceStream; axisCount: number; assignments?: Assignments } = $props();

  /** How often the wizard asks Rust for a detection while a step is active. */
  const POLL_MS = 200;

  let stepIndex = $state<number | null>(null);
  let message = $state("");
  let timer: ReturnType<typeof setInterval> | undefined;
  let sinceUs = 0;
  let polling = false;

  const step = $derived(stepIndex === null ? null : WIZARD_STEPS[stepIndex]);

  function start() {
    assignments = {};
    beginStep(0);
  }

  function beginStep(index: number) {
    stopTimer();
    if (index >= WIZARD_STEPS.length) {
      stepIndex = null;
      message = "Done. Check the assignments below.";
      return;
    }
    stepIndex = index;
    message = "";
    // The detector's baseline is the first sample after this point, so the pedals must be at
    // rest now. Samples share Rust's clock, so take the start time from the stream.
    sinceUs = stream.latest?.tUs ?? 0;
    timer = setInterval(() => void poll(WIZARD_STEPS[index].pedal), POLL_MS);
  }

  async function poll(pedal: PedalName) {
    if (polling) return;
    polling = true;
    try {
      const result = await detectAxis(sinceUs, assignedAxes(assignments, pedal));
      if (result.kind === "ambiguous") {
        message = `Several axes moved (${result.candidates.join(", ")}). Press only the ${pedal}.`;
        sinceUs = stream.latest?.tUs ?? sinceUs;
      } else if (result.kind === "axis") {
        const raw = stream.latest?.axes[result.index];
        if (raw !== undefined && isBackAtRest(result, raw)) {
          const { index: axis, rest, min, max } = result;
          assignments = { ...assignments, [pedal]: { axis, rest, min, max } };
          beginStep((stepIndex ?? 0) + 1);
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
      return;
    }
    // Manual choice: no sweep data yet, so assume the usual 'rest at minimum' range.
    // Calibration (SCT-014) captures the real range.
    const axis = Number(value);
    const raw = stream.latest?.axes[axis] ?? -32768;
    assignments = { ...assignments, [pedal]: { axis, rest: raw, min: -32768, max: 32767 } };
  }

  function stopTimer() {
    clearInterval(timer);
    timer = undefined;
  }

  onDestroy(stopTimer);
</script>

<section class="wizard" aria-label="Pedal setup">
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
      <button type="button" onclick={() => beginStep(WIZARD_STEPS.length)}>Cancel</button>
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

  th {
    padding: 0.25rem 1rem 0.25rem 0;
    color: var(--text-muted);
    font-weight: 600;
    text-align: left;
  }
</style>
