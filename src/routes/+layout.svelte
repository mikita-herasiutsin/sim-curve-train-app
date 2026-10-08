<script lang="ts">
  import "../app.css";
  import { onMount, type Snippet } from "svelte";
  import { initTheme } from "$lib/settings";

  let { children }: { children: Snippet } = $props();

  onMount(() => {
    return initTheme();
  });
</script>

{@render children()}
