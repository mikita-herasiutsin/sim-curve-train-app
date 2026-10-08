<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import {
    loadAudioEnabled,
    loadAudioVolume,
    saveAudioEnabled,
    saveAudioVolume,
  } from "$lib/settings";

  // The loaders are safe without `localStorage`, so the first render already shows the saved
  // settings.
  let audioEnabled = $state(loadAudioEnabled());
  let audioVolume = $state(loadAudioVolume());

  function toggleAudio(): void {
    audioEnabled = !audioEnabled;
    saveAudioEnabled(audioEnabled);
    invoke("audio_set_enabled", { enabled: audioEnabled }).catch(console.error);
  }

  /** Live while dragging: the backend follows every step. */
  function handleVolume(e: Event): void {
    const val = Number.parseFloat((e.target as HTMLInputElement).value);
    audioVolume = val;
    invoke("audio_set_volume", { volume: val }).catch(console.error);
  }

  /** Saved once the drag ends, not on every step. */
  function commitVolume(): void {
    saveAudioVolume(audioVolume);
  }

  function testTone(): void {
    invoke("audio_test_tone").catch(console.error);
  }
</script>

<div class="audio-controls">
  <button
    type="button"
    class="audio-btn"
    data-testid="audio-toggle"
    onclick={toggleAudio}
    aria-pressed={!audioEnabled}
    aria-label="Mute audio feedback"
    title={audioEnabled ? "Mute audio feedback" : "Unmute audio feedback"}
  >
    <svg
      class="audio-icon"
      aria-hidden="true"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      stroke-width="2"
      stroke-linecap="round"
      stroke-linejoin="round"
    >
      <path d="M11 5 6 9H2v6h4l5 4V5Z" />
      {#if audioEnabled}
        <path d="M15.5 8.5a5 5 0 0 1 0 7" />
        <path d="M19 5a10 10 0 0 1 0 14" />
      {:else}
        <path d="m22 9-6 6" />
        <path d="m16 9 6 6" />
      {/if}
    </svg>
  </button>
  <input
    type="range"
    min="0"
    max="1"
    step="0.01"
    value={audioVolume}
    oninput={handleVolume}
    onchange={commitVolume}
    disabled={!audioEnabled}
    aria-label="Audio volume"
    data-testid="audio-volume"
  />
  {#if import.meta.env.DEV}
    <button
      type="button"
      class="audio-btn"
      data-testid="audio-test-tone"
      onclick={testTone}
      aria-label="Play test sounds"
      title="Play test sounds"
    >
      ♪
    </button>
  {/if}
</div>

<style>
  .audio-controls {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }

  .audio-controls input[type="range"] {
    width: 4rem;
  }

  .audio-btn {
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

  .audio-btn:hover {
    color: var(--text);
    border-color: var(--accent);
  }

  .audio-icon {
    width: 1.125rem;
    height: 1.125rem;
  }
</style>
