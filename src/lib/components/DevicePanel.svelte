<script lang="ts">
  import { onMount } from "svelte";
  import type { DeviceInfo } from "$lib/devices";
  import {
    assignmentsFromProfile,
    loadProfile,
    profileFromAssignments,
    resetProfile,
    saveProfile,
    type DeviceProfile,
  } from "$lib/profile";
  import { DeviceStream } from "$lib/stream";
  import type { Assignments } from "$lib/wizard";
  import CalibrationPanel from "./CalibrationPanel.svelte";
  import PedalWizard from "./PedalWizard.svelte";
  import RawAxisMonitor from "./RawAxisMonitor.svelte";

  let { device }: { device: DeviceInfo } = $props();

  // The parent re-creates this panel per device (`{#key}`), so the id never changes here.
  // svelte-ignore state_referenced_locally
  const stream = new DeviceStream(device.id);
  let error = $state<string | null>(null);
  let assignments = $state<Assignments>({});
  let profile = $state<DeviceProfile | null>(null);

  onMount(() => {
    // start_stream activates the saved profile, so load it first only for display.
    loadProfile(device.id)
      .then((saved) => {
        profile = saved;
        if (saved) assignments = assignmentsFromProfile(saved);
      })
      .catch((e: unknown) => (error = String(e)));
    stream.start().catch((e: unknown) => (error = String(e)));
    return () => void stream.stop();
  });

  async function save(next: DeviceProfile) {
    await saveProfile(device.id, next);
    profile = next;
  }

  async function applyAssignments(next: Assignments) {
    try {
      error = null;
      if (Object.keys(next).length === 0) return;
      await save(await profileFromAssignments(next, profile));
    } catch (e: unknown) {
      error = `Saving the profile failed: ${String(e)}`;
    }
  }

  async function reset() {
    try {
      await resetProfile(device.id);
      profile = null;
      assignments = {};
    } catch (e: unknown) {
      error = `Reset failed: ${String(e)}`;
    }
  }
</script>

<div class="panel">
  <div class="title">
    <h3>{device.name}</h3>
    {#if profile}
      <span class="saved">Profile saved</span>
      <button type="button" onclick={reset}>Reset profile</button>
    {/if}
  </div>
  {#if error}
    <p class="error" role="alert">{error}</p>
  {/if}
  <PedalWizard
    {stream}
    axisCount={Math.min(device.axisCount, 8)}
    bind:assignments
    onchange={applyAssignments}
  />
  {#if profile}
    <CalibrationPanel {stream} {profile} onsave={save} />
  {/if}
  <p class="muted">Raw axes. Move one pedal at a time: exactly one bar should move.</p>
  <RawAxisMonitor {stream} />
</div>

<style>
  .panel {
    display: grid;
    gap: 1rem;
    margin-top: 2rem;
  }

  .title {
    display: flex;
    align-items: center;
    gap: 1rem;
  }

  h3 {
    margin: 0;
  }

  .saved {
    color: var(--throttle);
    font-size: 0.875rem;
  }

  button {
    padding: 0.25rem 0.625rem;
    border: 1px solid var(--border);
    border-radius: 0.5rem;
    background: var(--surface-raised);
    color: var(--text);
    font: inherit;
    font-size: 0.8125rem;
    cursor: pointer;
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
