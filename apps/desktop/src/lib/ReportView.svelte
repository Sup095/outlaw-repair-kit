<script lang="ts">
  import { api, type Incident, type ProblemReport } from "./api";

  let report = $state<ProblemReport | null>(null);
  let incidents = $state<Incident[]>([]);
  let error = $state<string | null>(null);
  let notice = $state<string | null>(null);
  let loaded = $state(false);

  // What the person will actually post. Seeded from the generated report and
  // then theirs to change — the backend sends whatever is in these, not what
  // it generated, so an edit is never silently discarded.
  let title = $state("");
  let body = $state("");

  const edited = $derived(
    report !== null && (title !== report.title || body !== report.body),
  );

  async function load() {
    try {
      const [built, recorded] = await Promise.all([
        api.reportBuild(),
        api.reportIncidents(40),
      ]);
      report = built;
      incidents = recorded;
      title = built.title;
      body = built.body;
      error = null;
    } catch (problem) {
      error = String(problem);
    } finally {
      loaded = true;
    }
  }

  async function openIssue() {
    notice = null;
    try {
      await api.reportOpenIssue(title, body);
      notice = "The issue form is open in your browser. Nothing is sent until you press the button there.";
    } catch (problem) {
      // Usually "too long for a link". Saving is the way through, so say so
      // rather than leaving a dead end.
      error = String(problem);
    }
  }

  async function save() {
    notice = null;
    try {
      const path = await api.reportSave(body);
      notice = `Saved to ${path}. Attach that file to the issue.`;
    } catch (problem) {
      error = String(problem);
    }
  }

  async function openForm() {
    notice = null;
    try {
      await api.reportOpenForm();
      notice = "The issue form is open in your browser.";
    } catch (problem) {
      error = String(problem);
    }
  }

  async function clear() {
    notice = null;
    try {
      await api.reportClear();
      await load();
      notice = "Cleared. Nothing recorded is kept.";
    } catch (problem) {
      error = String(problem);
    }
  }

  function restore() {
    if (!report) return;
    title = report.title;
    body = report.body;
  }

  load();
</script>

<div class="head">
  <h2>Report a problem</h2>
  <button onclick={load}>Refresh</button>
  <button onclick={clear} disabled={incidents.length === 0}>Forget what was recorded</button>
</div>

<p class="dim intro">
  Errors and crashes are recorded on this machine so they can be reported afterwards.
  What is below is exactly what would be posted, with personal details already taken
  out — home directory paths, account and machine names, email and network addresses,
  and anything shaped like a key. <strong>Read it through anyway</strong>, and edit
  whatever you like. Nothing is ever sent for you: the button opens GitHub's form with
  this already filled in, and you press Submit there.
</p>

{#if error}
  <div class="panel bad">{error}</div>
{/if}
{#if notice}
  <div class="panel good">{notice}</div>
{/if}

{#if loaded && report}
  {#if report.incident_count === 0}
    <div class="panel dim">
      Nothing has gone wrong on this machine yet. You can still describe something by
      hand below.
    </div>
  {:else if report.includes_crash}
    <div class="panel warn">
      This includes a crash. That is the most useful kind of report there is — it says
      exactly where the tool fell over.
    </div>
  {/if}

  <label class="field">
    <span class="dim">Title</span>
    <input bind:value={title} spellcheck="false" />
  </label>

  <label class="field">
    <span class="dim">What would be posted</span>
    <textarea bind:value={body} rows="20" spellcheck="false"></textarea>
  </label>

  <div class="actions">
    <button class="go" onclick={openIssue}>Open the issue form</button>
    <button onclick={save}>Save to a file</button>
    <button onclick={openForm}>Open a blank issue</button>
    {#if edited}
      <button class="quiet" onclick={restore}>Undo my edits</button>
    {/if}
  </div>

  {#if incidents.length}
    <details class="raw">
      <summary>What was recorded, unedited ({incidents.length})</summary>
      <p class="dim small">
        This is the record as it sits on this machine, before anything was taken out. It
        is shown so you can see what the report was built from — it is not what gets
        posted.
      </p>
      <ul>
        {#each incidents as incident, index (index)}
          <li class:crash={incident.kind === "panic"}>
            <span class="kind">{incident.kind === "panic" ? "crash" : "error"}</span>
            <span class="when dim">{incident.at}</span>
            <span class="where dim">{incident.source}</span>
            <div class="what">{incident.message}</div>
          </li>
        {/each}
      </ul>
    </details>
  {/if}
{/if}

<style>
  .head { display: flex; align-items: center; gap: 0.5rem; margin-bottom: 0.5rem; }
  .head button:first-of-type { margin-left: auto; }
  .intro { max-width: 68ch; margin: 0 0 1rem; font-size: 12.5px; }
  .bad { border-color: var(--red); color: var(--red); }
  .warn { border-color: var(--yellow); color: var(--yellow); }
  .good { border-color: var(--cyan); color: var(--cyan); }
  .field { display: block; margin-bottom: 0.8rem; }
  .field span { display: block; font-size: 11.5px; text-transform: uppercase; letter-spacing: 0.12em; margin-bottom: 0.25rem; }
  .field input, .field textarea {
    width: 100%;
    background: var(--bg);
    color: var(--text);
    border: 1px solid var(--border);
    padding: 0.5rem;
    font: inherit;
    font-size: 12.5px;
  }
  .field textarea { resize: vertical; line-height: 1.5; }
  .field input:focus, .field textarea:focus { outline: none; border-color: var(--cyan); }
  .actions { display: flex; gap: 0.5rem; flex-wrap: wrap; margin-bottom: 1rem; }
  .go { border-color: var(--cyan); color: var(--cyan); }
  .quiet { color: var(--text-dim); }
  .raw { margin-top: 0.5rem; font-size: 12.5px; }
  .raw summary { cursor: pointer; }
  .raw ul { list-style: none; padding: 0; margin: 0.6rem 0 0; }
  .raw li { border-left: 2px solid var(--border); padding: 0.35rem 0 0.35rem 0.6rem; margin-bottom: 0.4rem; }
  .raw li.crash { border-left-color: var(--red); }
  .kind { text-transform: uppercase; font-size: 10.5px; letter-spacing: 0.14em; margin-right: 0.5rem; }
  .raw li.crash .kind { color: var(--red); }
  .when, .where { font-size: 11.5px; margin-right: 0.5rem; }
  .what { margin-top: 0.2rem; word-break: break-word; }
  .small { font-size: 11.5px; }
</style>
