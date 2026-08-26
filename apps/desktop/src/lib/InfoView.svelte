<script lang="ts">
  import { api, type BootReport, type ManualEntry, type ManualPage } from "./api";

  const { booted }: { booted: BootReport } = $props();

  let contents = $state<ManualEntry[]>([]);
  let page = $state<ManualPage | null>(null);
  let licence = $state<string | null>(null);
  let error = $state<string | null>(null);
  let host = $state<Record<string, unknown> | null>(null);

  async function load() {
    try {
      contents = await api.manualContents();
      if (contents.length) await open(contents[0].id);
      host = await api.hostInfo();
      error = null;
    } catch (problem) {
      error = String(problem);
    }
  }

  async function open(id: string) {
    licence = null;
    try {
      page = await api.manualPage(id);
      error = null;
    } catch (problem) {
      error = String(problem);
    }
  }

  async function showLicence() {
    page = null;
    try {
      licence = await api.manualLicence();
    } catch (problem) {
      error = String(problem);
    }
  }

  load();
</script>

<div class="head">
  <h2>Information</h2>
  <span class="dim version">v{booted.version}</span>
</div>

<p class="dim intro">
  The whole manual, carried inside this program rather than linked to. A computer that
  has gone wrong is often one that cannot reach the internet, and the pages most likely
  to be needed are the ones least likely to be reachable when they are needed. What is
  below describes <em>this</em> build, not whatever has been written since.
</p>

{#if error}
  <div class="panel bad">{error}</div>
{/if}

<div class="layout">
  <nav class="panel contents">
    {#each contents as entry (entry.id)}
      <button class:active={page?.id === entry.id} onclick={() => open(entry.id)}>
        <strong>{entry.title}</strong>
        <span class="dim">{entry.summary}</span>
      </button>
    {/each}

    <div class="about">
      <h3>This build</h3>
      <dl>
        <div><dt class="dim">Version</dt><dd>{booted.version}</dd></div>
        {#if host}
          <div><dt class="dim">Platform</dt><dd>{host.os_name ?? "unknown"}</dd></div>
          <div><dt class="dim">Architecture</dt><dd>{host.arch ?? "unknown"}</dd></div>
        {/if}
        <div>
          <dt class="dim">Updates</dt>
          <dd>
            {#if booted.update.state === "available"}
              {booted.update.latest} is available
            {:else if booted.update.state === "up_to_date"}
              up to date
            {:else}
              not known
            {/if}
          </dd>
        </div>
      </dl>
      <button class:active={licence !== null} onclick={showLicence}>
        <strong>Licence</strong>
        <span class="dim">MIT.</span>
      </button>
      <p class="dim made">
        Made by Outlaw Systems, in collaboration with AI. Design decisions are made
        jointly and reviewed by a person; a substantial part of the code is written by
        Claude.
      </p>
    </div>
  </nav>

  <article class="panel reading">
    {#if licence !== null}
      <pre class="licence">{licence}</pre>
    {:else if page}
      <!-- Rendered from Markdown in the back end, from files compiled into this
           binary. Not fetched, not user-supplied, and not reachable by anything
           a scan found — which is why this is the only screen here that renders
           HTML at all. See src-tauri/src/manual.rs. -->
      <div class="doc">{@html page.html}</div>
    {:else}
      <p class="dim">Choose a page.</p>
    {/if}
  </article>
</div>

<style>
  .head { display: flex; align-items: baseline; gap: 1rem; margin-bottom: 0.5rem; }
  .version { font-size: 12.5px; }
  .intro { max-width: 78ch; margin: 0 0 1rem; font-size: 12.5px; }
  .bad { border-color: var(--red); color: var(--red); margin-bottom: 0.8rem; }

  .layout { display: grid; grid-template-columns: minmax(200px, 260px) 1fr; gap: 1rem; align-items: start; }

  .contents { display: grid; gap: 0.15rem; padding: 0.5rem; position: sticky; top: 0; }
  .contents button {
    display: grid;
    gap: 0.1rem;
    text-align: left;
    background: transparent;
    border-color: transparent;
    padding: 0.4rem 0.5rem;
    width: 100%;
  }
  .contents button strong { font-size: 12.5px; font-weight: 600; }
  /* The summary is a sentence, and buttons in this application are set in
     capitals. A sentence in capitals is a sentence being shouted at somebody
     who only wanted to know what the page is about. */
  .contents button span { font-size: 11.5px; text-transform: none; letter-spacing: 0; }
  .contents button.active { border-color: var(--amber); color: var(--amber); }
  .contents button.active span { color: var(--amber-dim); }

  .about { border-top: 1px solid var(--line); margin-top: 0.5rem; padding-top: 0.6rem; }
  .about h3 { margin: 0 0 0.4rem 0.5rem; font-size: 11px; text-transform: uppercase; letter-spacing: 0.14em; color: var(--text-dim); }
  .about dl { margin: 0 0 0.5rem; padding: 0 0.5rem; display: grid; gap: 0.15rem; font-size: 11.5px; }
  .about dl div { display: flex; gap: 0.5rem; }
  .about dt { min-width: 8.5ch; }
  .about dd { margin: 0; }
  .made { margin: 0.6rem 0.5rem 0.2rem; font-size: 11px; }

  .reading { padding: 1.1rem 1.4rem; min-height: 60vh; }
  .licence { margin: 0; white-space: pre-wrap; font-size: 12px; color: var(--text-dim); }

  /* The manual's own typography. Deliberately narrower than the panel: a line
     of prose 200 characters wide is one nobody finishes reading. */
  .doc { max-width: 84ch; font-size: 13px; line-height: 1.62; }
  .doc :global(h1) { font-size: 19px; color: var(--amber); margin: 0 0 0.8rem; }
  .doc :global(h2) { font-size: 15px; color: var(--cyan); margin: 1.6rem 0 0.5rem; }
  .doc :global(h3) { font-size: 13px; color: var(--text); margin: 1.2rem 0 0.4rem; text-transform: none; letter-spacing: 0; }
  .doc :global(p) { margin: 0 0 0.85rem; }
  .doc :global(ul), .doc :global(ol) { margin: 0 0 0.85rem; padding-left: 1.3rem; }
  .doc :global(li) { margin-bottom: 0.25rem; }
  .doc :global(code) {
    background: #171c25;
    border: 1px solid var(--line);
    padding: 0.05rem 0.3rem;
    font-size: 12px;
  }
  .doc :global(pre) {
    background: #10141b;
    border: 1px solid var(--line);
    padding: 0.7rem 0.9rem;
    overflow-x: auto;
    margin: 0 0 0.9rem;
  }
  .doc :global(pre code) { background: none; border: none; padding: 0; font-size: 12px; }
  .doc :global(blockquote) {
    margin: 0 0 0.9rem;
    padding: 0.1rem 0 0.1rem 0.9rem;
    border-left: 2px solid var(--amber-dim);
    color: var(--text-dim);
  }
  .doc :global(table) { border-collapse: collapse; margin: 0 0 0.9rem; font-size: 12px; display: block; overflow-x: auto; }
  .doc :global(th), .doc :global(td) { border: 1px solid var(--line); padding: 0.35rem 0.6rem; text-align: left; vertical-align: top; }
  .doc :global(th) { color: var(--cyan); font-weight: 600; }
  .doc :global(a) { color: var(--cyan); }
  .doc :global(hr) { border: none; border-top: 1px solid var(--line); margin: 1.4rem 0; }
  .doc :global(strong) { color: var(--text); font-weight: 600; }
</style>
