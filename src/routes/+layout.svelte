<script lang="ts">
  import "../app.css";
  import { onMount, type Snippet } from "svelte";
  import { isTauri } from "@tauri-apps/api/core";
  import SimPedalsPanel from "$lib/components/SimPedalsPanel.svelte";
  import { initTheme } from "$lib/settings";

  let { children }: { children: Snippet } = $props();

  onMount(() => {
    return initTheme();
  });
</script>

{@render children()}
{#if import.meta.env.DEV && isTauri()}
  <SimPedalsPanel />
{/if}
