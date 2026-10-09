<script lang="ts">
  import "../app.css";
  import { onMount, type Snippet } from "svelte";
  import { invoke, isTauri } from "@tauri-apps/api/core";
  import { initTheme, loadAudioEnabled, loadAudioVolume } from "$lib/settings";
  import SimPedalsPanel from "$lib/components/SimPedalsPanel.svelte";

  let { children }: { children: Snippet } = $props();

  onMount(() => {
    // Push the persisted audio settings to the backend once per app start.
    if (isTauri()) {
      invoke("audio_set_enabled", { enabled: loadAudioEnabled() }).catch(console.error);
      invoke("audio_set_volume", { volume: loadAudioVolume() }).catch(console.error);
    }
    return initTheme();
  });
</script>

{@render children()}
{#if import.meta.env.DEV && isTauri()}
  <SimPedalsPanel />
{/if}
