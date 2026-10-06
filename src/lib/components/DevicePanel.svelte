<script lang="ts">
  import { onMount } from "svelte";
  import type { DeviceInfo } from "$lib/devices";
  import { DeviceStream } from "$lib/stream";
  import type { Assignments } from "$lib/wizard";
  import PedalWizard from "./PedalWizard.svelte";
  import RawAxisMonitor from "./RawAxisMonitor.svelte";

  let { device }: { device: DeviceInfo } = $props();

  // The parent re-creates this panel per device (`{#key}`), so the id never changes here.
  // svelte-ignore state_referenced_locally
  const stream = new DeviceStream(device.id);
  let error = $state<string | null>(null);
  let assignments = $state<Assignments>({});

  onMount(() => {
    stream.start().catch((e: unknown) => (error = String(e)));
    return () => void stream.stop();
  });
</script>

<div class="panel">
  <h3>{device.name}</h3>
  {#if error}
    <p class="error" role="alert">Stream failed: {error}</p>
  {:else}
    <PedalWizard {stream} axisCount={Math.min(device.axisCount, 8)} bind:assignments />
    <p class="muted">Move one pedal at a time: exactly one bar should move.</p>
    <RawAxisMonitor {stream} />
  {/if}
</div>

<style>
  .panel {
    display: grid;
    gap: 1rem;
    margin-top: 2rem;
  }

  h3 {
    margin: 0;
  }

  .muted {
    margin: 0;
    color: var(--text-muted);
  }

  .error {
    color: var(--brake);
  }
</style>
