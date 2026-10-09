<script lang="ts">
  import { onMount } from "svelte";
  import { listDevices, onDevicesChanged, setSimPedals, type DevicesSnapshot } from "$lib/devices";

  let hasSimDevice = $state(false);
  let collapsed = $state(false);
  let throttle = $state(0);
  let brake = $state(0);
  let clutch = $state(0);
  let auto = $state(false);
  let error = $state<string | null>(null);

  let rafId: number | null = null;
  let inFlight = false;
  let pending = false;
  let destroyed = false;

  function scheduleSend() {
    if (destroyed) return;
    if (typeof requestAnimationFrame === "undefined") {
      void doSend();
      return;
    }
    if (rafId !== null) {
      return;
    }
    rafId = requestAnimationFrame(() => {
      rafId = null;
      void doSend();
    });
  }

  async function doSend() {
    if (inFlight) {
      pending = true;
      return;
    }
    inFlight = true;
    try {
      error = null;
      const t = Math.max(0, Math.min(100, Number(throttle))) / 100;
      const b = Math.max(0, Math.min(100, Number(brake))) / 100;
      const c = Math.max(0, Math.min(100, Number(clutch))) / 100;
      await setSimPedals([t, b, c], auto);
    } catch (e: unknown) {
      error = String(e);
    } finally {
      inFlight = false;
      if (pending) {
        pending = false;
        scheduleSend();
      }
    }
  }

  function releaseAll() {
    throttle = 0;
    brake = 0;
    clutch = 0;
    scheduleSend();
  }

  /** Shows the panel for a snapshot. When the device appears, the backend may still hold
   * targets from before a webview reload, so it gets the panel state. */
  function showFor(snapshot: DevicesSnapshot) {
    const present = snapshot.devices.some((d) => d.simulated);
    if (present && !hasSimDevice) scheduleSend();
    hasSimDevice = present;
  }

  onMount(() => {
    let unlisten: (() => void) | undefined;

    let gotEvent = false;
    onDevicesChanged((snapshot) => {
      gotEvent = true;
      showFor(snapshot);
    })
      .then((fn) => {
        if (destroyed) {
          fn();
        } else {
          unlisten = fn;
        }
      })
      .catch((e: unknown) => {
        error = String(e);
      });

    listDevices()
      .then((snapshot) => {
        // An event that arrived first is newer than this snapshot.
        if (!gotEvent) showFor(snapshot);
      })
      .catch((e: unknown) => {
        error = String(e);
      });

    return () => {
      destroyed = true;
      unlisten?.();
      if (rafId !== null) {
        cancelAnimationFrame(rafId);
        rafId = null;
      }
    };
  });
</script>

{#if hasSimDevice}
  <aside class="sim-panel" class:collapsed aria-label="Simulated pedals (dev)">
    <div class="panel-header">
      <span class="panel-title">Simulated pedals (dev)</span>
      <button
        type="button"
        class="toggle-btn"
        aria-expanded={!collapsed}
        onclick={() => (collapsed = !collapsed)}
      >
        {collapsed ? "Expand" : "Collapse"}
      </button>
    </div>
    {#if !collapsed}
      <div class="panel-body">
        {#if error}
          <p class="error" role="alert">{error}</p>
        {/if}
        <div class="sliders">
          <div class="control-row">
            <div class="label-row">
              <label for="sim-throttle">Throttle</label>
              <span class="val">{throttle}%</span>
            </div>
            <input
              id="sim-throttle"
              type="range"
              min="0"
              max="100"
              bind:value={throttle}
              oninput={scheduleSend}
              onchange={scheduleSend}
              disabled={auto}
              aria-label="Throttle"
              class="slider-throttle"
            />
          </div>
          <div class="control-row">
            <div class="label-row">
              <label for="sim-brake">Brake</label>
              <span class="val">{brake}%</span>
            </div>
            <input
              id="sim-brake"
              type="range"
              min="0"
              max="100"
              bind:value={brake}
              oninput={scheduleSend}
              onchange={scheduleSend}
              disabled={auto}
              aria-label="Brake"
              class="slider-brake"
            />
          </div>
          <div class="control-row">
            <div class="label-row">
              <label for="sim-clutch">Clutch</label>
              <span class="val">{clutch}%</span>
            </div>
            <input
              id="sim-clutch"
              type="range"
              min="0"
              max="100"
              bind:value={clutch}
              oninput={scheduleSend}
              onchange={scheduleSend}
              disabled={auto}
              aria-label="Clutch"
              class="slider-clutch"
            />
          </div>
        </div>
        <div class="actions-row">
          <label class="auto-label" for="sim-auto">
            <input
              id="sim-auto"
              type="checkbox"
              bind:checked={auto}
              onchange={scheduleSend}
              aria-label="Auto"
            />
            Auto
          </label>
          <button type="button" class="release-btn" disabled={auto} onclick={releaseAll}>
            Release all
          </button>
        </div>
      </div>
    {/if}
  </aside>
{/if}

<style>
  .sim-panel {
    position: fixed;
    bottom: 1rem;
    right: 1rem;
    z-index: 1000;
    width: 280px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 0.75rem;
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.4);
    font-family: inherit;
    font-size: 0.8125rem;
    color: var(--text);
    overflow: hidden;
  }

  .panel-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0.625rem 0.875rem;
    background: var(--surface-raised);
    border-bottom: 1px solid var(--border);
  }

  .sim-panel.collapsed .panel-header {
    border-bottom: none;
  }

  .panel-title {
    font-weight: 600;
    font-size: 0.8125rem;
  }

  .toggle-btn {
    padding: 0.15rem 0.5rem;
    border: 1px solid var(--border);
    border-radius: 0.375rem;
    background: var(--surface);
    color: var(--text-muted);
    font: inherit;
    font-size: 0.75rem;
    cursor: pointer;
  }

  .toggle-btn:hover {
    color: var(--text);
  }

  .panel-body {
    padding: 0.875rem;
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
  }

  .sliders {
    display: flex;
    flex-direction: column;
    gap: 0.625rem;
  }

  .control-row {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
  }

  .label-row {
    display: flex;
    justify-content: space-between;
    align-items: center;
    color: var(--text-muted);
    font-size: 0.75rem;
  }

  .val {
    font-variant-numeric: tabular-nums;
    color: var(--text);
  }

  input[type="range"] {
    width: 100%;
    cursor: pointer;
  }

  input[type="range"]:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .slider-throttle {
    accent-color: var(--throttle);
  }

  .slider-brake {
    accent-color: var(--brake);
  }

  .slider-clutch {
    accent-color: var(--accent);
  }

  .actions-row {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-top: 0.25rem;
    padding-top: 0.5rem;
    border-top: 1px solid var(--border);
  }

  .auto-label {
    display: flex;
    align-items: center;
    gap: 0.375rem;
    cursor: pointer;
    user-select: none;
  }

  .auto-label input[type="checkbox"] {
    accent-color: var(--accent);
    cursor: pointer;
  }

  .release-btn {
    padding: 0.25rem 0.625rem;
    border: 1px solid var(--border);
    border-radius: 0.5rem;
    background: var(--surface-raised);
    color: var(--text);
    font: inherit;
    font-size: 0.8125rem;
    cursor: pointer;
  }

  .release-btn:hover:not(:disabled) {
    background: color-mix(in srgb, var(--accent) 15%, var(--surface-raised));
  }

  .release-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .error {
    margin: 0;
    padding: 0.375rem 0.5rem;
    border-radius: 0.375rem;
    background: color-mix(in srgb, var(--brake) 15%, transparent);
    color: var(--brake);
    font-size: 0.75rem;
  }
</style>
