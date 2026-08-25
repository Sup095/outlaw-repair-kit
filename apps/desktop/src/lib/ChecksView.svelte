<script lang="ts">
  import { api, type CheckCatalogue } from "./api";

  let catalogue = $state<CheckCatalogue | null>(null);
  let error = $state<string | null>(null);
  let loaded = $state(false);

  const blocked = $derived(catalogue?.checks.filter((check) => !check.available) ?? []);

  // Grouped by tier rather than listed flat, because the question people
  // actually have is "what do I get if I ask for a deeper scan".
  const tiers = [
    { id: "quick", label: "Quick", note: "Runs in a quick scan, and in every deeper one." },
    { id: "full", label: "Full", note: "Adds checks that start programs or talk to hardware." },
    { id: "deep", label: "Deep", note: "Nothing declares this tier yet." },
  ] as const;

  async function load() {
    try {
      catalogue = await api.probes();
      error = null;
    } catch (problem) {
      error = String(problem);
    } finally {
      loaded = true;
    }
  }

  load();
</script>

<div class="head">
  <h2>Checks</h2>
  <button onclick={load}>Refresh</button>
</div>

<p class="dim intro">
  Every check this build knows how to run, and whether it can run on this machine. A
  check that cannot run is never quietly dropped — it is skipped with the reason shown,
  here and in the scan itself, because a scan that could not look at something must not
  read like it looked and found nothing.
</p>

{#if error}
  <div class="panel bad">{error}</div>
{:else if loaded && catalogue}
  <div class="panel summary">
    <span>{catalogue.checks.length} checks</span>
    <span class="dim">·</span>
    <span>{catalogue.checks.length - blocked.length} can run here</span>
    <span class="dim">·</span>
    <span class="dim">{catalogue.platform}</span>
    <span class="dim">·</span>
    <span class="dim">
      {catalogue.elevated ? "running with administrator rights" : "not elevated"}
    </span>
  </div>

  {#if blocked.length}
    <div class="panel warn">
      {blocked.length} check{blocked.length === 1 ? "" : "s"} cannot run here. Each one says
      why below — usually a missing tool, or rights this process does not have.
    </div>
  {/if}

  {#each tiers as tier (tier.id)}
    {@const mine = catalogue.checks.filter((check) => check.tier === tier.id)}
    <section>
      <h3>{tier.label}</h3>
      <p class="dim tier-note">{tier.note}</p>
      {#if mine.length === 0}
        <div class="panel dim">No checks run only at this tier.</div>
      {:else}
        {#each mine as check (check.id)}
          <article class="panel row" class:off={!check.available}>
            <div class="body">
              <strong>{check.name}</strong>
              <code class="dim">{check.id}</code>
              <p>{check.description}</p>
              {#if !check.available}
                <p class="reason">Will not run here — {check.unavailable_reason}</p>
              {/if}
            </div>
            <div class="side dim">
              <div>{check.platforms.join(", ")}</div>
              {#if check.requires_elevation}<div>needs administrator</div>{/if}
              {#if check.required_tools.length}
                <div>needs {check.required_tools.join(", ")}</div>
              {/if}
            </div>
          </article>
        {/each}
      {/if}
    </section>
  {/each}
{/if}

<style>
  .head { display: flex; align-items: center; gap: 1rem; margin-bottom: 0.5rem; }
  .head button { margin-left: auto; }
  .intro { max-width: 66ch; margin: 0 0 1rem; font-size: 12.5px; }
  .summary { display: flex; gap: 0.5rem; flex-wrap: wrap; font-size: 12.5px; margin-bottom: 0.6rem; }
  .bad { border-color: var(--red); color: var(--red); }
  .warn { border-color: var(--yellow); color: var(--yellow); margin-bottom: 0.8rem; }
  section { margin-bottom: 1.4rem; }
  h3 { margin: 0 0 0.15rem; font-size: 12px; text-transform: uppercase; letter-spacing: 0.14em; }
  .tier-note { margin: 0 0 0.5rem; font-size: 12px; }
  .row { display: flex; gap: 1rem; align-items: start; margin-bottom: 0.5rem; }
  .row.off { opacity: 0.72; border-style: dashed; }
  .body { flex: 1; }
  .body code { margin-left: 0.5rem; font-size: 11.5px; }
  .body p { margin: 0.3rem 0 0; font-size: 12.5px; }
  .reason { color: var(--yellow); }
  .side { text-align: right; font-size: 11.5px; min-width: 14ch; }
</style>
