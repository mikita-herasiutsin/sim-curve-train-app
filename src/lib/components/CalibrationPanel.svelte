<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";
  import { calibrate, type AxisCalibration, type DeviceProfile } from "$lib/profile";
  import type { DeviceStream } from "$lib/stream";
  import { WIZARD_STEPS, pedalLabel, type PedalName } from "$lib/wizard";

  let {
    stream,
    profile,
    onsave,
  }: {
    stream: DeviceStream;
    profile: DeviceProfile;
    onsave: (profile: DeviceProfile) => Promise<void>;
  } = $props();

  let values = $state<Record<PedalName, number>>({ throttle: 0, brake: 0, clutch: 0 });
  /** Pedal whose range is being re-swept, with the sample time the sweep started at. */
  let sweeping = $state<{ pedal: PedalName; sinceUs: number } | null>(null);
  let error = $state<string | null>(null);

  const pedals = $derived(WIZARD_STEPS.map((s) => s.pedal).filter((p) => profile[p] !== null));

  onMount(() => {
    let frame = 0;
    const render = () => {
      frame = requestAnimationFrame(render);
      const f = stream.latestFrame;
      if (f) values = { throttle: f.throttle, brake: f.brake, clutch: f.clutch };
    };
    frame = requestAnimationFrame(render);
    return () => cancelAnimationFrame(frame);
  });

  async function update(pedal: PedalName, change: Partial<AxisCalibration>) {
    const current = profile[pedal];
    if (!current) return;
    await save({
      ...profile,
      [pedal]: { ...current, calibration: { ...current.calibration, ...change } },
    });
  }

  async function save(next: DeviceProfile) {
    error = null;
    try {
      await onsave(next);
    } catch (e: unknown) {
      error = String(e);
    }
  }

  function startSweep(pedal: PedalName) {
    sweeping = { pedal, sinceUs: stream.latest?.tUs ?? 0 };
  }

  async function finishSweep() {
    if (!sweeping) return;
    const { pedal, sinceUs } = sweeping;
    sweeping = null;
    const current = profile[pedal];
    if (!current) return;
    try {
      const range = await invoke<[number, number] | null>("capture_range", {
        axis: current.axis,
        sinceUs,
      });
      if (!range) throw new Error("no samples recorded");
      const old = current.calibration;
      // The sweep started with the pedal released, so its first value is the rest position.
      const rest = old.invert ? range[1] : range[0];
      const calibration = await calibrate(
        { min: range[0], max: range[1], rest },
        old.deadzoneLow,
        old.deadzoneHigh,
      );
      await save({ ...profile, [pedal]: { ...current, calibration } });
    } catch (e: unknown) {
      error = `Recalibration failed: ${String(e)}`;
    }
  }

  const percent = (fraction: number) => Math.round(fraction * 1000) / 10;
</script>

<section class="calibration" aria-label="Calibration">
  <h4>Calibration</h4>
  {#if error}<p class="error" role="alert">{error}</p>{/if}
  {#each pedals as pedal (pedal)}
    {@const p = profile[pedal]!}
    {@const value = values[pedal]}
    <div class="row" data-testid="calibration-{pedal}">
      <span class="name">{pedalLabel(pedal)} <small>axis {p.axis}</small></span>
      <span
        class="bar"
        role="meter"
        aria-label="{pedalLabel(pedal)} output"
        aria-valuenow={percent(value)}
        aria-valuemin={0}
        aria-valuemax={100}
      >
        <span class="fill fill--{pedal}" style:width="{value * 100}%"></span>
      </span>
      <strong class="value">{percent(value).toFixed(1)}%</strong>
      <label>
        <input
          type="checkbox"
          checked={p.calibration.invert}
          onchange={(e) => update(pedal, { invert: e.currentTarget.checked })}
        />
        Invert
      </label>
      <label>
        Low deadzone
        <input
          type="number"
          min="0"
          max="20"
          step="0.5"
          value={percent(p.calibration.deadzoneLow)}
          onchange={(e) => update(pedal, { deadzoneLow: Number(e.currentTarget.value) / 100 })}
        />%
      </label>
      <label>
        High deadzone
        <input
          type="number"
          min="0"
          max="20"
          step="0.5"
          value={percent(p.calibration.deadzoneHigh)}
          onchange={(e) => update(pedal, { deadzoneHigh: Number(e.currentTarget.value) / 100 })}
        />%
      </label>
      {#if sweeping?.pedal === pedal}
        <button type="button" class="primary" onclick={finishSweep}>Done sweeping</button>
      {:else}
        <button type="button" onclick={() => startSweep(pedal)} disabled={sweeping !== null}>
          Recalibrate range
        </button>
      {/if}
    </div>
  {/each}
  {#if sweeping}
    <p class="hint" aria-live="polite">
      Start with the {sweeping.pedal} released, press it fully a few times, then click "Done sweeping".
    </p>
  {/if}
</section>

<style>
  .calibration {
    display: grid;
    gap: 0.75rem;
    padding: 1.25rem;
    border: 1px solid var(--border);
    border-radius: 0.75rem;
    background: var(--surface);
  }

  h4 {
    margin: 0;
  }

  .row {
    display: grid;
    grid-template-columns: 8rem minmax(6rem, 1fr) 4.5rem auto auto auto auto;
    align-items: center;
    gap: 0.75rem;
    font-size: 0.875rem;
  }

  small {
    color: var(--text-muted);
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

  .fill--brake {
    background: var(--brake);
  }

  .fill--throttle {
    background: var(--throttle);
  }

  .value {
    text-align: right;
    font-variant-numeric: tabular-nums;
  }

  label {
    display: flex;
    align-items: center;
    gap: 0.375rem;
    color: var(--text-muted);
    white-space: nowrap;
  }

  input[type="number"] {
    width: 4rem;
    padding: 0.25rem;
    border: 1px solid var(--border);
    border-radius: 0.375rem;
    background: var(--surface-raised);
    color: var(--text);
    font: inherit;
  }

  button {
    padding: 0.375rem 0.75rem;
    border: 1px solid var(--border);
    border-radius: 0.5rem;
    background: var(--surface-raised);
    color: var(--text);
    font: inherit;
    cursor: pointer;
    white-space: nowrap;
  }

  button.primary {
    border-color: var(--accent);
  }

  .hint {
    margin: 0;
    color: var(--text-muted);
  }

  .error {
    margin: 0;
    color: var(--brake);
  }
</style>
