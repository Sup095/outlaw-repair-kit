<script lang="ts">
  /**
   * What is running, and what a sweep would do to each.
   *
   * Stage two of `docs/proposals/process-control.md`, in the window. It stops
   * nothing, and says so on the screen rather than in the manual, because a
   * list of running programs with no visible way to act on it is a screen
   * somebody will otherwise spend a minute hunting for the button on.
   *
   * The judgement is not made here. It comes from the same `Survey` the
   * terminal prints, so the two cannot come to different conclusions about
   * what "held back" means.
   */
  import { api, type ProcessSurvey } from "./api";
  import { formatBytes } from "./bytes";
  import { compactDuration } from "./time";

  let survey = $state<ProcessSurvey | null>(null);
  let error = $state<string | null>(null);
  let loading = $state(false);
  let showAll = $state(false);

  const ENOUGH = 20;

  const candidates = $derived(
    survey ? survey.rows.filter((row) => row.standing.standing === "candidate") : [],
  );
  const held = $derived(
    survey ? survey.rows.filter((row) => row.standing.standing === "held-back") : [],
  );
  const shown = $derived(showAll ? candidates : candidates.slice(0, ENOUGH));

  async function load() {
    loading = true;
    try {
      survey = await api.processSurvey();
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
  <h2>Processes</h2>
  <button onclick={load} disabled={loading}>{loading ? "Looking" : "Refresh"}</button>
</div>

<p class="dim intro">
  What is running, and what would happen to each if there were a button to stop
  things. <strong>There is not one yet.</strong> This screen only looks: nothing
  here stops, suspends, or changes anything. The list exists on its own first so
  that it can be read on real machines before anything is able to act on it.
</p>

{#if error}
  <div class="panel bad">{error}</div>
{:else if !survey}
  <div class="panel dim">Looking at what is running…</div>
{:else}
  <div class="panel counts">
    <div><strong>{survey.running}</strong> <span class="dim">running</span></div>
    <div><strong>{candidates.length}</strong> <span class="dim">could be stopped</span></div>
    <div><strong>{held.length}</strong> <span class="dim">held back</span></div>
    <div>
      <strong>{survey.rows.length - candidates.length - held.length}</strong>
      <span class="dim">never touched</span>
    </div>
  </div>

  {#if survey.in_front_unchecked}
    <!-- Called out, and not quietly. Every other unknown in this tool makes it
         more careful; this one makes it less, because not knowing what is in
         front of you holds nothing back. A screen that hid it would be showing
         a list that looked complete and was not. -->
    <div class="panel unchecked">
      <h3>One rule did not run</h3>
      <p>
        Nothing with a window in front of you is offered for stopping, and on
        this machine that could not be checked: {survey.in_front_unchecked}.
        Everything else below still applies. It means the list may include what
        you are looking at.
      </p>
    </div>
  {/if}

  <section>
    <div class="section-head">
      <h3>Could be stopped</h3>
      <span class="dim">
        holding {formatBytes(survey.memory_held_by_candidates)} between them
      </span>
    </div>
    {#if candidates.length === 0}
      <div class="panel dim">Nothing.</div>
    {:else}
      <div class="panel rows">
        {#each shown as row (row.pid)}
          <div class="row">
            <span class="name" title={row.name}>{row.name}</span>
            <span class="mem">{formatBytes(row.memory_bytes)}</span>
            <span class="dim when">running {compactDuration(row.run_time_secs)}</span>
          </div>
        {/each}
        {#if candidates.length > shown.length}
          <button class="more" onclick={() => (showAll = true)}>
            Show the other {candidates.length - shown.length}
          </button>
        {/if}
      </div>
      <p class="dim note">
        <strong>Holding</strong> is what they have now, not what stopping them
        would give back. The second number is always smaller, because memory
        shared between programs is counted against every one of them, and it is
        only knowable by measuring afterwards.
      </p>
    {/if}
  </section>

  <section>
    <h3>Held back, and why</h3>
    <p class="dim note">
      Not offered by default. You could still choose them one at a time, once
      there is anything to choose them for.
    </p>
    {#if survey.why_held_back.length === 0}
      <div class="panel dim">Nothing.</div>
    {:else}
      <div class="panel reasons">
        {#each survey.why_held_back as reason (reason.reason)}
          <div class="reason">
            <span>{reason.reason}</span>
            <span class="dim count">
              {reason.count} {reason.count === 1 ? "process" : "processes"}
            </span>
          </div>
        {/each}
      </div>
    {/if}
  </section>

  <section>
    <h3>Never touched</h3>
    <p class="dim note">
      There is no setting that changes this. A list of what a tool considers
      untouchable is worthless if nobody can read it, so here it is.
    </p>
    <div class="panel reasons">
      {#each survey.why_protected as reason (reason.reason)}
        <div class="reason">
          <span>{reason.reason}</span>
          <span class="dim count">
            {reason.count} {reason.count === 1 ? "process" : "processes"}
          </span>
        </div>
      {/each}
    </div>
  </section>
{/if}

<style>
  .head { display: flex; align-items: center; gap: 1rem; margin-bottom: 0.5rem; }
  .head button { margin-left: auto; }
  .intro { max-width: 68ch; margin: 0 0 1rem; font-size: 12.5px; }
  .note { max-width: 68ch; margin: 0.5rem 0 0; font-size: 12px; }

  .counts {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem 2rem;
    font-size: 13px;
    margin-bottom: 1rem;
  }
  .counts strong { color: var(--cyan); font-size: 15px; }

  .unchecked { border-color: var(--amber); margin-bottom: 1rem; }
  .unchecked h3 { margin: 0 0 0.35rem; color: var(--amber); }
  .unchecked p { margin: 0; font-size: 12.5px; max-width: 68ch; }

  section { margin-bottom: 1.5rem; }
  section h3 { margin: 0 0 0.5rem; }
  .section-head {
    display: flex;
    align-items: baseline;
    gap: 0.9rem;
    flex-wrap: wrap;
    margin-bottom: 0.5rem;
  }
  .section-head h3 { margin: 0; }

  .rows { display: grid; gap: 0.2rem; font-size: 12.5px; }
  .row { display: flex; gap: 0.9rem; align-items: baseline; }
  .name {
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .mem { flex: none; min-width: 5.5rem; text-align: right; color: var(--cyan); }
  .when { flex: none; min-width: 8rem; }
  .more { margin-top: 0.5rem; justify-self: start; font-size: 12px; padding: 0.3rem 0.7rem; }

  .reasons { display: grid; gap: 0.25rem; font-size: 12.5px; }
  .reason { display: flex; gap: 1rem; align-items: baseline; }
  .reason span:first-child { flex: 1 1 auto; min-width: 0; }
  .count { flex: none; }

  .bad { border-color: var(--red); color: var(--red); }
</style>
