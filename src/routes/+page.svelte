<script lang="ts">
  import { onMount } from "svelte";
  import { goto } from "$app/navigation";
  import { resolve } from "$app/paths";
  import { isTauri } from "@tauri-apps/api/core";
  import AppHeader from "$lib/components/AppHeader.svelte";
  import { listPresets, playableDrills, type Preset, type Drill } from "$lib/drill";
  import { hasWarmUp } from "$lib/warmup";
  import { bestTotals } from "$lib/attempts";
  import { loadLastPreset, saveLastPreset } from "$lib/settings";

  let inTauri = $state(true);
  let loaded = $state(false);
  let loadError = $state<unknown>(null);
  let presets = $state<Preset[]>([]);
  let openPresetId = $state<string | null>(null);
  let scores = $state<Record<string, number>>({});

  function fetchScores(presetId: string) {
    scores = {};
    bestTotals(presetId)
      .then((result) => {
        if (openPresetId !== presetId) return;
        scores = result;
      })
      // Without scores every drill shows a dash.
      .catch((err: unknown) => console.error("Failed to load best scores", err));
  }

  function handlePresetClick(id: string) {
    if (openPresetId === id) {
      openPresetId = null;
      return;
    }
    openPresetId = id;
    saveLastPreset(id);
    fetchScores(id);
  }

  function handleDrillClick(preset: Preset, drill: Drill) {
    saveLastPreset(preset.id);
    goto(
      resolve(
        `/drill?preset=${encodeURIComponent(preset.id)}&drill=${encodeURIComponent(drill.id)}`,
      ),
    );
  }

  function handleWarmUpClick(preset: Preset) {
    saveLastPreset(preset.id);
    goto(resolve(`/drill?preset=${encodeURIComponent(preset.id)}&warmup=1`));
  }

  onMount(() => {
    if (!isTauri()) {
      inTauri = false;
      return;
    }
    inTauri = true;
    listPresets()
      .then((p) => {
        presets = p.filter((preset) => playableDrills(preset).length > 0);
        loaded = true;
        if (presets.length > 0) {
          const lastId = loadLastPreset();
          const initial = presets.find((pr) => pr.id === lastId) ?? presets[0];
          openPresetId = initial.id;
          fetchScores(initial.id);
        }
      })
      .catch((e: unknown) => {
        console.error("Failed to load presets", e);
        loadError = e;
        loaded = true;
      });
  });
</script>

<AppHeader />

<main>
  <section class="hero">
    <p class="eyebrow">Pre-alpha</p>
    <h2>Train your pedal muscle memory</h2>
    <p class="lead">
      Follow target pressures and real telemetry traces with your own pedals, and get an instant
      score. Native, low-latency, offline.
    </p>
    <div class="pedals" aria-hidden="true">
      <div class="pedal pedal--brake"><span style="height: 72%"></span></div>
      <div class="pedal pedal--throttle"><span style="height: 38%"></span></div>
    </div>
  </section>

  <section class="presets" aria-labelledby="presets-title">
    <h3 id="presets-title">Presets</h3>
    {#if !inTauri}
      <p>Presets load in the desktop app.</p>
    {:else if loadError}
      <p role="alert">Failed to load presets: {loadError}</p>
    {:else if loaded && presets.length === 0}
      <p>No playable drills found.</p>
    {:else if !loaded}
      <p>Loading presets…</p>
    {:else}
      <ul class="preset-list">
        {#each presets as preset (preset.id)}
          <li class="preset-card">
            <button
              type="button"
              class="preset-header"
              aria-expanded={openPresetId === preset.id}
              onclick={() => handlePresetClick(preset.id)}
            >
              <span class="preset-info">
                <strong class="preset-name">{preset.name}</strong>
                {#if preset.description}
                  <span class="preset-description">{preset.description}</span>
                {/if}
              </span>
              <span class="drill-count"
                >{playableDrills(preset).length}
                {playableDrills(preset).length === 1 ? "drill" : "drills"}</span
              >
            </button>
            {#if openPresetId === preset.id}
              {#if hasWarmUp(preset)}
                <div class="warm-up-container">
                  <button
                    type="button"
                    class="btn-warmup"
                    data-testid="warm-up-start"
                    onclick={() => handleWarmUpClick(preset)}
                  >
                    Warm-up
                  </button>
                </div>
              {/if}
              <ul class="drill-list">
                {#each playableDrills(preset) as drill (drill.id)}
                  <li>
                    <button
                      type="button"
                      class="drill"
                      onclick={() => handleDrillClick(preset, drill)}
                    >
                      <span class="drill-name">{drill.name}</span>
                      <span class="drill-type">{drill.type}</span>
                      <span class="drill-pedal pedal--{drill.pedal}">{drill.pedal}</span>
                      <span class="drill-score">
                        {#if Object.hasOwn(scores, drill.id)}
                          Best {Math.round(scores[drill.id])}
                        {:else}
                          Best —
                        {/if}
                      </span>
                    </button>
                  </li>
                {/each}
              </ul>
            {/if}
          </li>
        {/each}
      </ul>
    {/if}
  </section>
</main>

<style>
  main {
    display: grid;
    grid-template-columns: minmax(0, 1.2fr) minmax(0, 1fr);
    gap: 2rem;
    max-width: 72rem;
    margin: 0 auto;
    padding: 2.5rem 1.5rem;
  }

  @media (max-width: 60rem) {
    main {
      grid-template-columns: 1fr;
    }
  }

  .eyebrow {
    margin: 0 0 0.5rem;
    color: var(--accent);
    font-size: 0.8125rem;
    font-weight: 600;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }

  h2 {
    margin: 0 0 1rem;
    font-size: 2.25rem;
    line-height: 1.15;
  }

  .lead {
    margin: 0 0 2rem;
    max-width: 34rem;
    color: var(--text-muted);
  }

  .pedals {
    display: flex;
    gap: 1.25rem;
    height: 14rem;
  }

  .pedal {
    position: relative;
    width: 4rem;
    border: 1px solid var(--border);
    border-radius: 0.75rem;
    background: var(--surface);
    overflow: hidden;
  }

  .pedal span {
    position: absolute;
    inset: auto 0 0;
    border-radius: 0 0 0.75rem 0.75rem;
  }

  .pedal--brake span {
    background: var(--brake);
  }

  .pedal--throttle span {
    background: var(--throttle);
  }

  .presets {
    padding: 1.5rem;
    border: 1px solid var(--border);
    border-radius: 1rem;
    background: var(--surface);
  }

  h3 {
    margin: 0 0 1rem;
    font-size: 1rem;
  }

  .preset-list {
    display: grid;
    gap: 0.75rem;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .preset-card {
    border-radius: 0.75rem;
    background: var(--surface-raised);
    overflow: hidden;
    border: 1px solid var(--border);
  }

  .preset-header {
    width: 100%;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 1rem;
    padding: 1rem;
    background: transparent;
    border: none;
    color: inherit;
    text-align: left;
    cursor: pointer;
  }

  .preset-header:hover {
    background: color-mix(in srgb, var(--accent) 8%, transparent);
  }

  .preset-info {
    display: flex;
    flex-direction: column;
  }

  .preset-info strong {
    display: block;
    font-size: 1rem;
  }

  .preset-description {
    display: block;
    margin: 0.25rem 0 0;
    color: var(--text-muted);
    font-size: 0.875rem;
  }

  .drill-count {
    flex: none;
    padding: 0.25rem 0.625rem;
    border-radius: 999px;
    background: color-mix(in srgb, var(--accent) 18%, transparent);
    color: var(--accent);
    font-size: 0.8125rem;
    font-weight: 600;
  }

  .warm-up-container {
    padding: 0 1rem 0.5rem;
  }

  .btn-warmup {
    width: 100%;
    padding: 0.625rem 1rem;
    border: none;
    border-radius: 0.5rem;
    background: var(--accent);
    color: #fff;
    font-size: 0.875rem;
    font-weight: 600;
    cursor: pointer;
    text-align: center;
    transition: opacity 0.15s ease;
  }

  .btn-warmup:hover {
    opacity: 0.9;
  }

  .drill-list {
    display: grid;
    gap: 0.5rem;
    margin: 0;
    padding: 0 1rem 1rem;
    list-style: none;
  }

  .drill {
    width: 100%;
    display: flex;
    align-items: center;
    gap: 0.75rem;
    padding: 0.625rem 0.875rem;
    border: 1px solid var(--border);
    border-radius: 0.5rem;
    background: var(--surface);
    color: inherit;
    cursor: pointer;
    text-align: left;
    transition: border-color 0.15s ease;
  }

  .drill:hover {
    border-color: var(--accent);
  }

  .drill-name {
    font-weight: 500;
    margin-right: auto;
  }

  .drill-type {
    color: var(--text-muted);
    font-size: 0.8125rem;
    text-transform: capitalize;
  }

  .drill-pedal {
    font-size: 0.8125rem;
    font-weight: 600;
    text-transform: capitalize;
  }

  .pedal--brake {
    color: var(--brake);
  }

  .pedal--throttle {
    color: var(--throttle);
  }

  .drill-score {
    font-size: 0.875rem;
    font-variant-numeric: tabular-nums;
    font-weight: 600;
    color: var(--text-muted);
  }
</style>
