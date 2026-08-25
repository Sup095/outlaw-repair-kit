<script lang="ts">
  import { api } from "./api";

  let routing = $state<any | null>(null);
  let error = $state<string | null>(null);
  let loading = $state(false);

  async function load() {
    loading = true;
    try {
      routing = await api.routing();
      error = null;
    } catch (problem) {
      error = String(problem);
    } finally {
      loading = false;
    }
  }

  load();
</script>

<div class="head">
  <h2>Model routing</h2>
  <button onclick={load} disabled={loading}>{loading ? "Checking…" : "Re-check"}</button>
</div>

{#if error}
  <div class="panel bad">{error}</div>
{:else if routing}
  <div class="panel">
    <strong>{routing.summary}</strong>
  </div>

  <section class="panel">
    <h3>How that was decided</h3>
    <!-- Shown in order so it is obvious why a tier was passed over, rather
         than leaving the choice a mystery. -->
    {#each routing.attempts as attempt (attempt.tier)}
      <div class="attempt" class:chosen={attempt.selected}>
        <span class="marker">{attempt.selected ? "→" : ""}</span>
        <span class="tier">{attempt.tier}</span>
        <span class="dim">{attempt.outcome}</span>
      </div>
    {/each}
  </section>

  <section class="panel">
    <h3>Graphics hardware</h3>
    {#if routing.gpus.length === 0}
      <p class="dim">None detected.</p>
    {/if}
    {#each routing.gpus as gpu (gpu.name)}
      <div>{gpu.name} <span class="dim">{gpu.vram_total_bytes ? `${(gpu.vram_total_bytes / 1024 ** 3).toFixed(1)} GB of video memory` : "video memory unknown"}</span></div>
    {/each}
    <p class="dim">{routing.vram_recommendation}</p>
  </section>

  <section class="panel">
    <h3>Without a model</h3>
    <p class="dim">
      {routing.runbook_entries} known problems can be matched and explained with no model
      involved at all. Every deterministic check works the same either way.
    </p>
  </section>
{/if}

<style>
  .head { display: flex; align-items: center; gap: 1rem; margin-bottom: 1rem; }
  .head button { margin-left: auto; }
  section { margin-top: 0.75rem; display: grid; gap: 0.4rem; }
  section h3 { font-size: 12.5px; color: var(--amber); }
  section p { margin: 0; font-size: 12.5px; max-width: 66ch; }
  .attempt { display: flex; gap: 0.8rem; font-size: 12.5px; }
  .attempt.chosen { color: var(--amber); }
  .marker { width: 1rem; }
  .tier { min-width: 6rem; text-transform: uppercase; letter-spacing: 0.1em; font-size: 11.5px; }
  .bad { border-color: var(--red); color: var(--red); }
</style>
