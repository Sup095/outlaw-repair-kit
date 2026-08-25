<script lang="ts">
  import { api } from "./api";

  let items = $state<any[]>([]);
  let error = $state<string | null>(null);
  let loaded = $state(false);

  async function load() {
    try {
      items = await api.queue();
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
  <h2>Triage queue</h2>
  <button onclick={load}>Refresh</button>
</div>

<p class="dim intro">
  Problems that are not safe to fix inline wait here, worst first. Working through the
  queue applies one change at a time, with a snapshot taken first and a rollback if the
  test that follows fails — run <code>outlaw fix</code> to do that.
</p>

{#if error}
  <div class="panel bad">{error}</div>
{:else if loaded && items.length === 0}
  <div class="panel dim">Nothing is waiting. Run a scan to fill this.</div>
{:else}
  {#each items as item (item.occurrence_key)}
    <article class="panel row">
      <span class="sev {item.severity}">{item.severity}</span>
      <div class="body">
        <strong>{item.title}</strong>
        <div class="dim">{item.subject ?? item.finding_id}</div>
        <p>{item.finding?.summary ?? ""}</p>
      </div>
      <div class="side dim">
        <div>{item.state}</div>
        <div>{item.attempts} attempt{item.attempts === 1 ? "" : "s"}</div>
      </div>
    </article>
  {/each}
{/if}

<style>
  .head { display: flex; align-items: center; gap: 1rem; margin-bottom: 0.5rem; }
  .head button { margin-left: auto; }
  .intro { max-width: 62ch; margin: 0 0 1rem; font-size: 12.5px; }
  .row { display: flex; gap: 1rem; align-items: start; margin-bottom: 0.6rem; }
  .body { flex: 1; }
  .body p { margin: 0.35rem 0 0; font-size: 12.5px; }
  .side { text-align: right; font-size: 12px; }
  .bad { border-color: var(--red); color: var(--red); }
  .sev { text-transform: uppercase; font-size: 10.5px; letter-spacing: 0.14em; padding: 0.1rem 0.45rem; border: 1px solid currentColor; }
  .sev.critical { color: #fff; background: var(--red); border-color: var(--red); }
  .sev.high { color: var(--red); }
  .sev.medium { color: var(--yellow); }
  .sev.low { color: var(--cyan); }
  .sev.info { color: var(--text-dim); }
</style>
