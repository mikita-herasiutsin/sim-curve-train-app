<script lang="ts">
  import { onMount } from "svelte";
  import { formatVersion, getAppInfo, type AppInfo } from "$lib/appInfo";

  let info = $state<AppInfo | null>(null);
  let failed = $state(false);

  onMount(() => {
    getAppInfo()
      .then((result) => (info = result))
      .catch((error: unknown) => {
        console.error("Failed to load app info", error);
        failed = true;
      });
  });
</script>

<header class="app-header">
  <div class="brand">
    <span class="logo" aria-hidden="true"></span>
    <h1>{info?.name ?? "SimCurveTrainApp"}</h1>
  </div>
  {#if info}
    <span class="version" data-testid="app-version">{formatVersion(info.version)}</span>
  {:else if failed}
    <span class="version version--error" data-testid="app-version">version unavailable</span>
  {/if}
</header>

<style>
  .app-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 1rem;
    padding: 1rem 1.5rem;
    border-bottom: 1px solid var(--border);
    background: var(--surface);
  }

  .brand {
    display: flex;
    align-items: center;
    gap: 0.75rem;
  }

  .logo {
    width: 1.75rem;
    height: 1.75rem;
    border-radius: 0.5rem;
    background: linear-gradient(135deg, var(--brake) 0%, var(--throttle) 100%);
  }

  h1 {
    margin: 0;
    font-size: 1.125rem;
    font-weight: 700;
    letter-spacing: 0.01em;
  }

  .version {
    padding: 0.25rem 0.625rem;
    border: 1px solid var(--border);
    border-radius: 999px;
    color: var(--text-muted);
    font-size: 0.8125rem;
    font-variant-numeric: tabular-nums;
  }

  .version--error {
    color: var(--brake);
  }
</style>
