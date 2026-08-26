<script lang="ts">
  import { readableTime } from "./time";
  import type { Change, Look } from "./api";
  import { watch, listenForChanges, refresh, start, stop, forget } from "./watch.svelte";

  let confirmingForget = $state(false);

  const order = ["critical", "high", "medium", "low", "info"];

  // The store outlives this screen, so opening it is a matter of catching up
  // with what happened while it was closed rather than of starting anything.
  listenForChanges();
  refresh();

  const present = $derived(
    Object.values(watch.status?.baseline.seen ?? {})
      .filter((seen) => seen.present)
      .sort((a, b) => order.indexOf(a.severity) - order.indexOf(b.severity)),
  );
  const gone = $derived(
    Object.values(watch.status?.baseline.seen ?? {}).filter((seen) => !seen.present).length,
  );
  const muted = $derived(watch.status?.baseline.muted ?? []);
  const established = $derived(watch.status?.baseline.established ?? false);

  function headline(change: Change): string {
    switch (change.change) {
      case "appeared":
        return change.finding.title;
      case "worsened":
        return change.finding.title;
      case "eased":
        return change.finding.title;
      case "cleared":
        return change.title;
      case "flapping":
        return change.finding.title;
    }
  }

  function label(change: Change): string {
    switch (change.change) {
      case "appeared":
        return "new";
      case "worsened":
        return `worse — ${change.was} to ${change.finding.severity}`;
      case "eased":
        return `eased — ${change.was} to ${change.finding.severity}`;
      case "cleared":
        return "gone";
      case "flapping":
        return `comes and goes — seen ${change.appearances} times, now held quiet`;
    }
  }

  function tone(change: Change): string {
    if (change.change === "cleared") return "good";
    if (change.change === "eased") return "low";
    return change.finding.severity;
  }

  function detail(change: Change): string | null {
    return change.change === "cleared" ? null : change.finding.detail;
  }

  function stamp(look: Look): string {
    return readableTime(look.at);
  }

  async function confirmForget() {
    confirmingForget = false;
    await forget();
  }
</script>

<div class="head">
  <h2>Watching</h2>
  {#if watch.running}
    <span class="live">watching</span>
  {/if}
  <div class="controls">
    {#if watch.running}
      <button class="stop" onclick={stop}>Stop watching</button>
    {:else}
      <label>
        <span class="dim">Look</span>
        <select bind:value={watch.tier}>
          <option value="quick">quick</option>
          <option value="full">full</option>
          <option value="deep">deep</option>
        </select>
      </label>
      <label>
        <span class="dim">every</span>
        <input type="number" min="1" max="1440" bind:value={watch.everyMinutes} />
        <span class="dim">min</span>
      </label>
      <button class="go" onclick={start}>Start watching</button>
    {/if}
  </div>
</div>

<p class="dim intro">
  Looks on its own and says something only when something changes: a problem appearing,
  getting worse, easing, or going away. Nothing appears here in between, which is what
  a working watcher looks like — if you want to know what it currently thinks, that is
  the panel on the right.
</p>

{#if watch.error}
  <div class="panel bad">
    {watch.error}
    {#if watch.running}<span class="dim"> — still watching.</span>{/if}
  </div>
{/if}

<div class="layout">
  <section>
    {#if watch.recorded !== null}
      <div class="panel note">
        Recorded how this machine is right now: <strong>{watch.recorded}</strong>
        {watch.recorded === 1 ? "thing" : "things"} to keep an eye on. Watching for changes
        from here.
        <p class="dim">
          A computer that already had six problems did not just develop six problems, so the
          first look reports nothing. Everything after this is measured against it.
        </p>
      </div>
    {/if}

    {#if watch.history.length === 0 && watch.recorded === null}
      <div class="panel dim quiet">
        {#if !established}
          Nothing watched yet. Press <strong>Start watching</strong> and the first look will
          record how this machine is now.
        {:else if watch.running}
          Nothing has changed since the watcher started. That is the normal state, and this
          panel staying empty is the good outcome.
        {:else if watch.lastLooked === null}
          <!-- It has watched this machine before, but not since this window
               opened. Saying "nothing changed while it was running" here would
               be a claim about a stretch of time nothing observed. -->
          Not watching at the moment. What it already knows is on the right; press
          <strong>Start watching</strong> to pick up from there.
        {:else}
          Nothing changed while the watcher was running.
        {/if}
      </div>
    {:else}
      {#each watch.history as look (look.at)}
        <div class="panel look">
          <div class="when dim">{stamp(look)}</div>
          {#each look.changes as change (change.change + headline(change))}
            <div class="change {tone(change)}">
              <div class="row">
                <span class="tag {tone(change)}">{label(change)}</span>
                <strong>{headline(change)}</strong>
              </div>
              {#if detail(change)}
                <p class="dim">{detail(change)}</p>
              {/if}
            </div>
          {/each}
          {#if look.did_not_run.length > 0}
            <!-- Said out loud, because a check that could not run reports nothing
                 and reporting nothing looks exactly like reporting a repair. -->
            <p class="dim gap">
              {look.did_not_run.join(", ")} did not run this time, so nothing
              {look.did_not_run.length === 1 ? "it looks" : "they look"} for was judged
              either way.
            </p>
          {/if}
        </div>
      {/each}
    {/if}
  </section>

  <aside>
    <div class="panel">
      <h3>What it knows</h3>
      {#if !established}
        <p class="dim">Nothing yet.</p>
      {:else if present.length === 0}
        <p class="dim">Nothing wrong, as of the last look.</p>
      {:else}
        <ul class="present">
          {#each present as seen (seen.id + (seen.subject ?? ""))}
            <li>
              <span class="sev {seen.severity}">{seen.severity}</span>
              <div>
                <strong>{seen.title}</strong>
                <span class="dim since">since {readableTime(seen.first_seen)}</span>
              </div>
            </li>
          {/each}
        </ul>
      {/if}

      {#if gone > 0}
        <p class="dim footnote">
          {gone} {gone === 1 ? "problem" : "problems"} seen before and not there now,
          remembered so that {gone === 1 ? "it" : "they"} coming back is recognised as coming
          back.
        </p>
      {/if}
    </div>

    {#if muted.length > 0}
      <!-- Always listed. A watcher with a private list of things it has decided
           not to mention is not a watcher anybody should trust. -->
      <div class="panel">
        <h3>Held quiet</h3>
        {#each muted as entry (entry.key)}
          <div class="muted">
            <strong>{entry.title}</strong>
            <span class="dim">{entry.reason}</span>
          </div>
        {/each}
      </div>
    {/if}

    {#if established}
      <div class="panel reset">
        <h3>Start over</h3>
        <p class="dim">
          Forgets everything above. The next look records a fresh starting point and reports
          nothing — useful after fixing a batch of things, and confusing if you did not mean
          it.
        </p>
        {#if confirmingForget}
          <div class="confirm">
            <button class="danger" onclick={confirmForget}>Yes, forget it all</button>
            <button onclick={() => (confirmingForget = false)}>Cancel</button>
          </div>
        {:else}
          <button onclick={() => (confirmingForget = true)}>Forget and start over</button>
        {/if}
        {#if watch.status}
          <p class="dim path" title={watch.status.baseline_path}>
            {watch.status.baseline_path}
          </p>
        {/if}
      </div>
    {/if}
  </aside>
</div>

<style>
  .head { display: flex; align-items: center; gap: 0.9rem; margin-bottom: 0.5rem; }
  .controls { margin-left: auto; display: flex; align-items: center; gap: 0.7rem; }
  .controls label { display: flex; align-items: center; gap: 0.35rem; font-size: 12px; }
  .controls input { width: 4.5rem; }
  .intro { max-width: 78ch; margin: 0 0 1rem; font-size: 12.5px; }
  .bad { border-color: var(--red); color: var(--red); margin-bottom: 0.8rem; }

  .live {
    font-size: 10.5px;
    letter-spacing: 0.14em;
    text-transform: uppercase;
    color: var(--green);
    border: 1px solid var(--green);
    padding: 0.1rem 0.45rem;
  }

  .layout { display: grid; grid-template-columns: 1fr minmax(240px, 300px); gap: 1rem; align-items: start; }

  .quiet { padding: 1.1rem 1.2rem; font-size: 12.5px; line-height: 1.6; }
  .note { border-color: var(--cyan); margin-bottom: 0.8rem; font-size: 12.5px; }
  .note p { margin: 0.5rem 0 0; }

  .look { margin-bottom: 0.7rem; }
  .when { font-size: 11.5px; margin-bottom: 0.5rem; }
  .change { padding-left: 0.7rem; border-left: 2px solid var(--line); margin-bottom: 0.7rem; }
  .change:last-child { margin-bottom: 0; }
  .change.critical, .change.high { border-left-color: var(--red); }
  .change.medium { border-left-color: var(--yellow); }
  .change.good { border-left-color: var(--green); }
  .change p { margin: 0.3rem 0 0; font-size: 12px; }
  .row { display: flex; align-items: baseline; gap: 0.6rem; }

  .tag {
    font-size: 10.5px;
    letter-spacing: 0.1em;
    text-transform: uppercase;
    border: 1px solid currentColor;
    padding: 0.05rem 0.4rem;
    white-space: nowrap;
  }
  .tag.critical { color: #fff; background: var(--red); border-color: var(--red); }
  .tag.high { color: var(--red); }
  .tag.medium { color: var(--yellow); }
  .tag.low { color: var(--cyan); }
  .tag.info { color: var(--text-dim); }
  /* The only good news this screen prints. Showing it in the same red as
     everything else would make somebody's heart sink at being told a problem
     went away. */
  .tag.good { color: var(--green); }

  .gap { margin: 0.6rem 0 0; font-size: 11.5px; }

  aside { display: grid; gap: 0.7rem; }
  aside h3 { margin: 0 0 0.5rem; font-size: 11px; text-transform: uppercase; letter-spacing: 0.14em; color: var(--text-dim); }
  aside p { font-size: 11.5px; margin: 0; line-height: 1.55; }

  .present { list-style: none; margin: 0; padding: 0; display: grid; gap: 0.5rem; }
  .present li { display: flex; gap: 0.5rem; align-items: baseline; font-size: 12px; }
  .present strong { display: block; font-weight: 600; }
  .since { font-size: 11px; }
  .footnote { margin-top: 0.7rem; }


  .muted { margin-bottom: 0.6rem; font-size: 12px; }
  .muted strong { display: block; font-weight: 600; }
  .muted span { font-size: 11px; }

  .reset button { margin-top: 0.6rem; }
  .confirm { display: flex; gap: 0.5rem; margin-top: 0.6rem; }
  .danger { border-color: var(--red); color: var(--red); }
  .path { margin-top: 0.6rem; font-size: 10.5px; overflow-wrap: anywhere; }
</style>
