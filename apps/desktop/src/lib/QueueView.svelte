<script lang="ts">
  import { onDestroy } from "svelte";
  import {
    api,
    onFixAsk,
    onFixEvent,
    type FixAsk,
    type FixEvent,
    type ItemOutcome,
    type QueueItem,
  } from "./api";
  import type { UnlistenFn } from "@tauri-apps/api/event";

  let items = $state<QueueItem[]>([]);
  let error = $state<string | null>(null);
  let loaded = $state(false);

  const pending = $derived(items.filter((item) => item.state === "pending").length);

  let running = $state(false);
  let applying = $state(false);
  let coverage = $state<{ total: number; testable: number } | null>(null);
  let snapshotWarning = $state<string | null>(null);
  let current = $state<string | null>(null);
  let outcomes = $state<Record<string, ItemOutcome>>({});
  let summary = $state<string | null>(null);

  // The open question. While this is set, a change is waiting on an answer and
  // nothing is happening to the machine.
  let ask = $state<FixAsk | null>(null);

  const listeners: UnlistenFn[] = [];

  onFixEvent(handle).then((off) => listeners.push(off));
  onFixAsk((question) => (ask = question)).then((off) => listeners.push(off));
  onDestroy(() => listeners.forEach((off) => off()));

  function handle(event: FixEvent) {
    if (event.event === "started") {
      coverage = { total: event.total, testable: event.testable };
      snapshotWarning = event.snapshot_warning;
    } else if (event.event === "item") {
      current = event.occurrence_key;
    } else if (event.event === "outcome") {
      outcomes = { ...outcomes, [event.occurrence_key]: event.outcome };
      current = null;
    } else if (event.event === "finished") {
      current = null;
      summary = event.stopped
        ? "Stopped. Nothing further was attempted."
        : applying
          ? `${event.resolved} fixed. Everything attempted is in the audit log.`
          : "Dry run finished. Nothing was changed.";
    }
  }

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

  async function work(apply: boolean) {
    if (running) return;
    running = true;
    applying = apply;
    outcomes = {};
    summary = null;
    coverage = null;
    try {
      await api.fixRun(apply);
      error = null;
    } catch (problem) {
      error = String(problem);
    } finally {
      running = false;
      ask = null;
      current = null;
      await load();
    }
  }

  async function answer(reply: "approve" | "decline" | "stop") {
    const question = ask;
    if (!question) return;
    // Cleared first: the question is answered once, and a second click must
    // not be able to send a second answer to the same prompt.
    ask = null;
    try {
      await api.fixAnswer(question.id, reply);
    } catch (problem) {
      error = String(problem);
    }
  }

  async function stop() {
    try {
      await api.fixCancel();
    } catch (problem) {
      error = String(problem);
    }
  }

  function describe(outcome: ItemOutcome): string {
    switch (outcome.outcome) {
      case "resolved":
        return outcome.action;
      case "exhausted":
        return `Tried ${outcome.tried} candidate${outcome.tried === 1 ? "" : "s"}; none worked.`;
      case "stopped":
        return "Stopped before this one was attempted.";
      case "no-candidates":
        return "No known fix. Explaining a scan may be able to reason about it.";
      default:
        return "";
    }
  }

  load();
</script>

<div class="head">
  <h2>Triage queue</h2>
  <button onclick={load} disabled={running}>Refresh</button>
  <button onclick={() => work(false)} disabled={running || pending === 0}>
    Preview
  </button>
  <button class="apply" onclick={() => work(true)} disabled={running || pending === 0}>
    Work the queue
  </button>
  {#if running}
    <button class="stop" onclick={stop}>Stop</button>
  {/if}
</div>

<p class="dim intro">
  Problems that are not safe to fix inline wait here, worst first. Ones that have been
  worked already stay in the list with what happened to them. Working through the
  queue applies one change at a time, with a snapshot taken first and a rollback if the
  test that follows fails. <strong>Preview</strong> takes exactly the same path but is
  never given permission, so it shows what would happen without touching anything.
</p>

{#if coverage}
  <p class="dim intro">
    {coverage.testable} of {coverage.total} can be tested after a change, so only those can
    be fixed automatically. The rest are explained instead.
  </p>
{/if}

{#if snapshotWarning}
  <div class="panel warn">{snapshotWarning}</div>
{/if}

{#if summary}
  <div class="panel dim">{summary}</div>
{/if}

{#if error}
  <div class="panel bad">{error}</div>
{:else if loaded && items.length === 0}
  <div class="panel dim">Nothing is waiting. Run a scan to fill this.</div>
{:else if loaded && pending === 0}
  <div class="panel dim">
    Nothing is still waiting. The items below have been worked already.
  </div>
{:else}
  {#each items as item (item.occurrence_key)}
    <article class="panel row" class:active={current === item.occurrence_key}>
      <span class="sev {item.severity}">{item.severity}</span>
      <div class="body">
        <strong>{item.title}</strong>
        <div class="dim">{item.subject ?? item.finding_id}</div>
        <p>{item.finding?.detail ?? ""}</p>
        {#if outcomes[item.occurrence_key]}
          {@const outcome = outcomes[item.occurrence_key]}
          {#if outcome.outcome === "needs-a-person"}
            <div class="outcome">
              <div class="dim">This one needs you. Least disruptive first:</div>
              <ol>
                {#each outcome.instructions as instruction}
                  <li>{instruction}</li>
                {/each}
              </ol>
            </div>
          {:else}
            <div class="outcome" class:good={outcome.outcome === "resolved"}>
              {outcome.outcome === "resolved" ? "Fixed — " : ""}{describe(outcome)}
            </div>
          {/if}
        {/if}
      </div>
      <div class="side dim">
        <div>{current === item.occurrence_key ? "working…" : item.state}</div>
        <div>{item.attempts} attempt{item.attempts === 1 ? "" : "s"}</div>
        <div>{item.seen}</div>
      </div>
    </article>
  {/each}
{/if}

{#if ask}
  <div class="scrim">
    <div class="dialog">
      <h3>This would change your system</h3>
      <p class="action">{ask.action}</p>
      <p class="dim">to address: {ask.title}</p>
      <p class="dim small">
        A copy of anything touched is taken first, and the change is undone if the test
        that follows does not pass.
      </p>
      <div class="buttons">
        <button class="apply" onclick={() => answer("approve")}>Allow it</button>
        <button onclick={() => answer("decline")}>Skip this one</button>
        <button class="stop" onclick={() => answer("stop")}>Stop everything</button>
      </div>
    </div>
  </div>
{/if}

<style>
  .head { display: flex; align-items: center; gap: 0.5rem; margin-bottom: 0.5rem; }
  .head button:first-of-type { margin-left: auto; }
  .intro { max-width: 62ch; margin: 0 0 1rem; font-size: 12.5px; }
  .row { display: flex; gap: 1rem; align-items: start; margin-bottom: 0.6rem; }
  .row.active { border-color: var(--cyan); }
  .body { flex: 1; }
  .body p { margin: 0.35rem 0 0; font-size: 12.5px; }
  .side { text-align: right; font-size: 12px; }
  .bad { border-color: var(--red); color: var(--red); }
  .warn { border-color: var(--yellow); color: var(--yellow); }
  .outcome { margin-top: 0.5rem; font-size: 12.5px; border-left: 2px solid var(--text-dim); padding-left: 0.6rem; }
  .outcome.good { border-left-color: var(--cyan); color: var(--cyan); }
  .outcome ol { margin: 0.3rem 0 0; padding-left: 1.1rem; }
  /* A manual instruction may carry its suggested command on a second,
     indented line. HTML would collapse that into the sentence, running the
     command on to the end of the prose where it reads as part of it. */
  .outcome li { margin-bottom: 0.25rem; white-space: pre-wrap; }
  .apply { border-color: var(--cyan); color: var(--cyan); }
  .stop { border-color: var(--red); color: var(--red); }
  .scrim { position: fixed; inset: 0; background: rgba(0, 0, 0, 0.72); display: grid; place-items: center; z-index: 40; }
  .dialog { width: min(52ch, 90vw); background: var(--bg); border: 1px solid var(--cyan); padding: 1.2rem; }
  .dialog h3 { margin: 0 0 0.6rem; font-size: 13px; letter-spacing: 0.1em; text-transform: uppercase; }
  .action { margin: 0 0 0.4rem; font-size: 13.5px; }
  .small { font-size: 11.5px; }
  .buttons { display: flex; gap: 0.5rem; margin-top: 1rem; flex-wrap: wrap; }
</style>
