<script lang="ts">
  import { onMount } from "svelte";
  import { resolve } from "$app/paths";
  import { page } from "$app/state";
  import { formatVersion, getAppInfo, type AppInfo } from "$lib/appInfo";
  import AudioControls from "$lib/components/AudioControls.svelte";
  import {
    loadTheme,
    onThemeChange,
    resolveTheme,
    toggleTheme,
    type ResolvedTheme,
  } from "$lib/settings";

  let info = $state<AppInfo | null>(null);
  let failed = $state(false);
  let currentTheme = $state<ResolvedTheme>(resolveTheme(loadTheme()));

  const current = (path: "/" | "/devices" | "/live" | "/drill") =>
    page.url.pathname === resolve(path) ? ("page" as const) : undefined;

  onMount(() => {
    currentTheme = resolveTheme(loadTheme());
    const unsubTheme = onThemeChange((resolved) => {
      currentTheme = resolved;
    });

    getAppInfo()
      .then((result) => (info = result))
      .catch((error: unknown) => {
        console.error("Failed to load app info", error);
        failed = true;
      });

    return () => {
      unsubTheme();
    };
  });

  function handleToggleTheme(): void {
    currentTheme = toggleTheme();
  }
</script>

<header class="app-header">
  <div class="brand">
    <span class="logo" aria-hidden="true"></span>
    <h1>{info?.name ?? "SimCurveTrainApp"}</h1>
  </div>
  <nav aria-label="Main">
    <a href={resolve("/")} aria-current={current("/")}>Home</a>
    <a href={resolve("/devices")} aria-current={current("/devices")}>Devices</a>
    <a href={resolve("/live")} aria-current={current("/live")}>Live</a>
    <a href={resolve("/drill")} aria-current={current("/drill")}>Drills</a>
  </nav>
  <div class="header-actions">
    <button
      type="button"
      class="theme-toggle"
      data-testid="theme-toggle"
      onclick={handleToggleTheme}
      aria-label={currentTheme === "dark" ? "Switch to light theme" : "Switch to dark theme"}
      title={currentTheme === "dark" ? "Switch to light theme" : "Switch to dark theme"}
    >
      {#if currentTheme === "dark"}
        <svg
          class="theme-icon"
          aria-hidden="true"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="2"
          stroke-linecap="round"
          stroke-linejoin="round"
        >
          <circle cx="12" cy="12" r="4" />
          <path d="M12 2v2" />
          <path d="M12 20v2" />
          <path d="m4.93 4.93 1.41 1.41" />
          <path d="m17.66 17.66 1.41 1.41" />
          <path d="M2 12h2" />
          <path d="M20 12h2" />
          <path d="m6.34 17.66-1.41 1.41" />
          <path d="m19.07 4.93-1.41 1.41" />
        </svg>
      {:else}
        <svg
          class="theme-icon"
          aria-hidden="true"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="2"
          stroke-linecap="round"
          stroke-linejoin="round"
        >
          <path d="M12 3a6 6 0 0 0 9 9 9 9 0 1 1-9-9Z" />
        </svg>
      {/if}
    </button>
    <AudioControls />
    {#if info}
      <span class="version" data-testid="app-version">{formatVersion(info.version)}</span>
    {:else if failed}
      <span class="version version--error" data-testid="app-version">version unavailable</span>
    {/if}
  </div>
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

  nav {
    display: flex;
    gap: 0.25rem;
    margin-right: auto;
  }

  nav a {
    padding: 0.375rem 0.75rem;
    border-radius: 0.5rem;
    color: var(--text-muted);
    font-size: 0.9375rem;
    text-decoration: none;
  }

  nav a:hover,
  nav a[aria-current="page"] {
    color: var(--text);
    background: var(--surface-raised);
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

  .header-actions {
    display: flex;
    align-items: center;
    gap: 0.75rem;
  }

  .theme-toggle {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 2rem;
    height: 2rem;
    padding: 0;
    border: 1px solid var(--border);
    border-radius: 0.5rem;
    background: var(--surface-raised);
    color: var(--text-muted);
    cursor: pointer;
    transition:
      color 0.15s ease,
      background 0.15s ease,
      border-color 0.15s ease;
  }

  .theme-toggle:hover {
    color: var(--text);
    border-color: var(--accent);
  }

  .theme-icon {
    width: 1.125rem;
    height: 1.125rem;
  }
</style>
