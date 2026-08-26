<script lang="ts">
  import { api } from "./api";

  let rows = $state<{ at: string; readable: string; kind: string; message: string }[]>([]);
  let error = $state<string | null>(null);
  let limit = $state(80);

  async function load() {
    try {
      rows = await api.audit(limit);
      error = null;
    } catch (problem) {
      error = String(problem);
    }
  }

  load();
</script>

<div class="head">
  <h2>Audit log</h2>
  <button onclick={load}>Refresh</button>
</div>

<p class="dim intro">
  Everything the tool has checked, found, attempted, and changed. This is written
  whether or not anyone is watching, and it is never rewritten.
</p>

{#if error}
  <div class="panel bad">{error}</div>
{:else if rows.length === 0}
  <div class="panel dim">Nothing recorded yet.</div>
{:else}
  <div class="panel log">
    {#each rows as row, index (row.at + index)}
      <div class="line">
        <!-- `readable` rather than `at`: the raw one is RFC 3339 to seven decimal
             places, which is the right thing to store and the wrong thing to read. -->
        <span class="dim at" title={row.at}>{row.readable}</span>
        <span class="kind">{row.kind}</span>
        <span>{row.message}</span>
      </div>
    {/each}
  </div>
{/if}

<style>
  .head { display: flex; align-items: center; gap: 1rem; margin-bottom: 0.5rem; }
  .head button { margin-left: auto; }
  .intro { max-width: 62ch; margin: 0 0 1rem; font-size: 12.5px; }
  .log { display: grid; gap: 0.25rem; font-size: 12.5px; }
  .line { display: flex; gap: 0.9rem; }
  .at { min-width: 11rem; }
  .kind { color: var(--cyan); min-width: 8rem; }
  .bad { border-color: var(--red); color: var(--red); }
</style>
