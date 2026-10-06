<script lang="ts">
  import AppHeader from "$lib/components/AppHeader.svelte";

  const milestones = [
    {
      id: "M1",
      title: "See my pedals",
      text: "Detect & calibrate pedals, live bars and scrolling graph.",
    },
    { id: "M2", title: "First drill", text: "Hold drills with reps, scoring and audio feedback." },
    { id: "M3", title: "Traces", text: "Follow real brake and throttle traces, two view modes." },
    { id: "M4", title: "Content", text: "GT3, NASCAR and MX-5 presets plus a pre-race warm-up." },
    {
      id: "M5",
      title: "Progress",
      text: "Personal bests, leaderboard, onboarding, v0.1.0 release.",
    },
  ];
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

  <section class="roadmap" aria-labelledby="roadmap-title">
    <h3 id="roadmap-title">Roadmap</h3>
    <ol>
      {#each milestones as m (m.id)}
        <li>
          <span class="badge">{m.id}</span>
          <div>
            <strong>{m.title}</strong>
            <p>{m.text}</p>
          </div>
        </li>
      {/each}
    </ol>
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

  .roadmap {
    padding: 1.5rem;
    border: 1px solid var(--border);
    border-radius: 1rem;
    background: var(--surface);
  }

  h3 {
    margin: 0 0 1rem;
    font-size: 1rem;
  }

  ol {
    display: grid;
    gap: 0.75rem;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  li {
    display: flex;
    gap: 0.875rem;
    padding: 0.75rem;
    border-radius: 0.75rem;
    background: var(--surface-raised);
  }

  li p {
    margin: 0.125rem 0 0;
    color: var(--text-muted);
    font-size: 0.875rem;
  }

  .badge {
    flex: none;
    display: grid;
    place-items: center;
    width: 2.25rem;
    height: 2.25rem;
    border-radius: 0.5rem;
    background: color-mix(in srgb, var(--accent) 18%, transparent);
    color: var(--accent);
    font-size: 0.8125rem;
    font-weight: 700;
  }
</style>
