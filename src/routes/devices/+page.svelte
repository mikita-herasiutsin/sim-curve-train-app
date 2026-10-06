<script lang="ts">
  import { onMount } from "svelte";
  import AppHeader from "$lib/components/AppHeader.svelte";
  import DevicePanel from "$lib/components/DevicePanel.svelte";
  import { formatUsbIds, listDevices, onDevicesChanged, type DevicesSnapshot } from "$lib/devices";

  let snapshot = $state<DevicesSnapshot | null>(null);
  let loadError = $state<string | null>(null);
  let selectedId = $state<number | null>(null);
  // Unplugging the selected device hides its monitor.
  const selected = $derived(snapshot?.devices.find((d) => d.id === selectedId) ?? null);

  onMount(() => {
    let unlisten: (() => void) | undefined;
    let destroyed = false;

    // Subscribe before the first fetch so a hot-plug in between isn't missed.
    onDevicesChanged((next) => {
      snapshot = next;
      loadError = null;
    })
      .then((fn) => (destroyed ? fn() : (unlisten = fn)))
      .catch((error: unknown) => console.error("Failed to watch devices", error));

    listDevices()
      .then((result) => (snapshot ??= result))
      .catch((error: unknown) => {
        console.error("Failed to list devices", error);
        loadError = String(error);
      });

    return () => {
      destroyed = true;
      unlisten?.();
    };
  });
</script>

<AppHeader />

<main>
  <h2>Devices</h2>
  <p class="lead">
    Every connected game controller. Plug in or unplug a device and the list updates by itself.
  </p>

  {#if loadError || snapshot?.error}
    <p class="error" role="alert">Input unavailable: {snapshot?.error ?? loadError}</p>
  {:else if snapshot === null}
    <p class="muted">Looking for controllers…</p>
  {:else if snapshot.devices.length === 0}
    <p class="muted">No controllers found. Plug in your pedals.</p>
  {:else}
    <table>
      <thead>
        <tr>
          <th scope="col">Name</th>
          <th scope="col">USB VID:PID</th>
          <th scope="col" class="num">Axes</th>
          <th scope="col" class="num">Buttons</th>
          <th scope="col" class="num">Hats</th>
          <th scope="col">GUID</th>
          <th scope="col"><span class="visually-hidden">Monitor</span></th>
        </tr>
      </thead>
      <tbody>
        {#each snapshot.devices as device (device.id)}
          <tr class:selected={selectedId === device.id}>
            <td>{device.name}</td>
            <td class="mono">{formatUsbIds(device)}</td>
            <td class="num">{device.axisCount}</td>
            <td class="num">{device.buttonCount}</td>
            <td class="num">{device.hatCount}</td>
            <td class="mono guid">{device.guid}</td>
            <td>
              <button
                type="button"
                aria-pressed={selectedId === device.id}
                onclick={() => (selectedId = selectedId === device.id ? null : device.id)}
              >
                {selectedId === device.id ? "Hide axes" : "Show axes"}
              </button>
            </td>
          </tr>
        {/each}
      </tbody>
    </table>

    {#if selected}
      {#key selected.id}
        <DevicePanel device={selected} />
      {/key}
    {/if}
  {/if}
</main>

<style>
  main {
    max-width: 72rem;
    margin: 0 auto;
    padding: 2.5rem 1.5rem;
  }

  h2 {
    margin: 0 0 0.5rem;
    font-size: 1.75rem;
  }

  .lead,
  .muted {
    color: var(--text-muted);
  }

  .lead {
    margin: 0 0 1.5rem;
  }

  .error {
    color: var(--brake);
  }

  table {
    width: 100%;
    border-collapse: collapse;
    border: 1px solid var(--border);
    border-radius: 0.75rem;
    background: var(--surface);
    font-size: 0.9375rem;
  }

  th,
  td {
    padding: 0.625rem 0.875rem;
    border-bottom: 1px solid var(--border);
    text-align: left;
  }

  th {
    color: var(--text-muted);
    font-size: 0.8125rem;
    font-weight: 600;
  }

  tbody tr:last-child td {
    border-bottom: none;
  }

  .num {
    text-align: right;
    font-variant-numeric: tabular-nums;
  }

  .mono {
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
    font-size: 0.8125rem;
  }

  tr.selected td {
    background: var(--surface-raised);
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
    white-space: nowrap;
  }

  .visually-hidden {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
  }

  .guid {
    color: var(--text-muted);
  }
</style>
