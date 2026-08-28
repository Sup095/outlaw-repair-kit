<script lang="ts">
  import { formatBytes, readableDuration } from "./bytes";
  import { stress, listenForProgress, refresh, start, stop } from "./stress.svelte";

  // Everything the run consists of lives in `stress`, outside this component,
  // so that switching to another tab and back does not throw away a burn-in
  // somebody has been sitting through for an hour. See lib/stress.svelte.ts.
  listenForProgress();
  refresh();

  let confirming = $state(false);

  // Re-asked whenever the share changes, because the number this screen shows
  // is the number of bytes that would really be touched -- not the share
  // multiplied by something and hoped for.
  $effect(() => {
    void stress.memoryShare;
    if (!stress.running) void refresh();
  });

  const fraction = $derived(stress.total > 0 ? Math.min(1, stress.elapsed / stress.total) : 0);

  const report = $derived(stress.report);
  const ending = $derived(report?.ending);

  function beginConfirmed() {
    confirming = false;
    void start();
  }
</script>

<div class="head">
  <h2>Stress and burn-in</h2>
  {#if stress.running}
    <span class="live">running</span>
  {/if}
</div>

<p class="dim intro">
  The one thing here that works your computer rather than watching it. Every core is
  given arithmetic with a known correct answer and asked to repeat it, so a core that
  quietly returns the wrong number is caught with its number attached; a share of the
  free memory is filled with five different patterns and read back. It finds what a
  scan cannot — memory that corrupts a bit an hour, a core that computes wrongly only
  when hot, a cooling system full of dust. Nothing is changed and nothing is written.
</p>

{#if stress.error}
  <div class="panel bad">{stress.error}</div>
{/if}

<div class="layout">
  <section>
    <div class="panel controls">
      <div class="row">
        <label class="check">
          <input type="checkbox" bind:checked={stress.cpu} disabled={stress.running} />
          <span>Processor</span>
        </label>
        <label class="check">
          <input type="checkbox" bind:checked={stress.memory} disabled={stress.running} />
          <span>Memory</span>
        </label>
        <label>
          <span class="dim">for</span>
          <input
            class="minutes"
            type="number"
            min="1"
            max="1440"
            bind:value={stress.minutes}
            disabled={stress.running}
          />
          <span class="dim">minutes</span>
        </label>
      </div>

      {#if stress.memory}
        <label class="share">
          <span class="dim">Memory to test</span>
          <input
            type="range"
            min="0.05"
            max="0.95"
            step="0.05"
            bind:value={stress.memoryShare}
            disabled={stress.running}
          />
          <span class="amount">
            {#if stress.status && stress.status.memory_bytes > 0}
              {formatBytes(stress.status.memory_bytes)}
            {:else if stress.status}
              <span class="dim">not enough spare</span>
            {/if}
          </span>
        </label>
      {/if}

      <div class="row">
        {#if stress.running}
          <!-- Always available, always first. This is the one screen where
               stopping is a safety control and not merely a convenience. -->
          <button class="danger" onclick={stop}>Stop</button>
        {:else if confirming}
          <button class="primary" onclick={beginConfirmed}>Yes, start</button>
          <button onclick={() => (confirming = false)}>Cancel</button>
        {:else}
          <button class="primary" onclick={() => (confirming = true)}>Start</button>
        {/if}
      </div>

      {#if confirming && stress.status}
        <!-- Said in numbers, before it happens rather than after. Somebody
             about to have their machine pinned at full load and heated should
             see exactly what that means on this machine. -->
        <div class="warning">
          <strong>About to work this machine hard.</strong>
          <ul>
            <li>
              For {stress.minutes} minute{stress.minutes === 1 ? "" : "s"}, or until you press
              Stop.
            </li>
            {#if stress.cpu}
              <li>{stress.status.cores} cores at full load.</li>
            {/if}
            {#if stress.memory}
              {#if stress.status.memory_bytes > 0}
                <li>
                  {formatBytes(stress.status.memory_bytes)} of memory filled and checked, of the
                  {formatBytes(stress.status.memory_available_bytes)} free.
                  {formatBytes(stress.status.memory_reserved_bytes)} is always left alone.
                </li>
              {:else}
                <li>
                  The memory will not be tested: only
                  {formatBytes(stress.status.memory_available_bytes)} is free, and this always
                  leaves {formatBytes(stress.status.memory_reserved_bytes)} for the machine to
                  keep running in.
                </li>
              {/if}
            {/if}
          </ul>
          <p class="dim">
            The machine will get hot and will be slow to use. Nothing is changed and nothing
            is written; it stops itself if any part of the machine reaches the temperature
            this machine says is critical.
          </p>
        </div>
      {/if}
    </div>

    {#if stress.running}
      <div class="panel live-panel">
        <div class="bar"><div class="fill" style="width: {fraction * 100}%"></div></div>
        <div class="numbers">
          <span>{readableDuration(stress.elapsed)} of {readableDuration(stress.total)}</span>
          <span class="dim">{stress.blocks.toLocaleString()} blocks</span>
          {#if stress.memory}
            <span class="dim">
              {stress.memoryPatterns} memory pattern{stress.memoryPatterns === 1 ? "" : "s"}
            </span>
          {/if}
          {#if stress.hottest}
            <span class="heat">{stress.hottest.label} {stress.hottest.peak_c.toFixed(0)}°C</span>
          {/if}
        </div>
        {#if !stress.watchingHeat}
          <!-- Before the run finishes, not buried in the result. Somebody
               heating a laptop should know now that nothing is watching. -->
          <p class="dim blind">
            This machine reports no temperature that can be believed, so nothing is watching
            for overheating and the run cannot stop itself. Press Stop if it gets loud.
          </p>
        {/if}
      </div>
    {/if}

    {#each stress.faults as fault (fault.part + fault.detail)}
      <!-- Shown the moment it arrives, not held until the end. -->
      <div class="panel fault">
        <span class="sev critical">fault</span>
        <strong>{fault.part}</strong>
        <p>{fault.detail}</p>
      </div>
    {/each}

    {#if report && !stress.running}
      <div class="panel result">
        <h3>
          {#if ending?.ending === "too-hot"}
            Stopped because it got too hot
          {:else if ending?.ending === "cancelled"}
            Stopped after {readableDuration(report.ran_for_secs)}
          {:else}
            Finished after {readableDuration(report.ran_for_secs)}
          {/if}
        </h3>

        {#if ending?.ending === "too-hot"}
          <p class="hot">
            {ending.sensor} reached {ending.reached_c.toFixed(0)}°C, past the
            {ending.ceiling_c.toFixed(0)}°C this machine says is its limit. That is a real
            result, not a failed test: a machine that cannot be worked hard without
            overheating throttles itself under any real load. The causes are physical —
            dust in the cooling path, a fan that has stopped, thermal paste that has dried
            out.
          </p>
        {/if}

        <ul class="counts">
          {#if report.cpu}
            <li>
              <strong>{report.cpu.threads}</strong> cores,
              <strong>{report.cpu.blocks.toLocaleString()}</strong> blocks,
              <strong class:bad-count={report.cpu.wrong > 0}>{report.cpu.wrong}</strong> wrong
            </li>
          {/if}
          {#if report.memory?.memory === "ran"}
            <li>
              <strong>{formatBytes(report.memory.bytes)}</strong> of memory,
              <strong>{report.memory.patterns}</strong>
              pattern{report.memory.patterns === 1 ? "" : "s"} checked,
              <strong class:bad-count={report.memory.mismatches.length > 0}>
                {report.memory.mismatches.length}
              </strong> bad
            </li>
            {#if report.memory.patterns === 0}
              <!-- Said plainly. Zero next to "0 bad" in a panel headed
                   "finished" reads as a clean check of the memory, and it is
                   the opposite: the run ended before any one pattern had been
                   read back across the whole region. -->
              <li class="dim">
                The run ended before any one pattern had been read back across the whole of
                that, so the memory was not fully checked even once. Give it longer, or test
                less of it.
              </li>
            {/if}
          {:else if report.memory?.memory === "not-run"}
            <li class="dim">{report.memory.reason}</li>
          {/if}
        </ul>

        {#if report.watched_heat && report.heat.length > 0}
          <div class="heats">
            {#each report.heat.slice(0, 6) as heat (heat.label)}
              <span class="chip">{heat.label} <strong>{heat.peak_c.toFixed(0)}°C</strong></span>
            {/each}
          </div>
        {:else}
          <p class="dim">
            Nothing was watching the temperature during this run — this machine did not
            report one that could be believed. That is not a fault, and it is not a claim
            that the machine stayed cool.
          </p>
        {/if}

        {#if report.faults.length === 0 && ending?.ending === "completed"}
          <p class="dim caveat">
            Nothing went wrong, which means less than it sounds like: for as long as it ran,
            every core agreed with itself and the memory it could reach held what was written
            to it. Faults of this kind are intermittent by nature, and only the memory the
            operating system was willing to hand this program could be tested. A clean result
            narrows where a problem can be hiding. It does not prove there isn't one.
          </p>
        {/if}
      </div>
    {/if}
  </section>

  <aside>
    <div class="panel">
      <h3>What it is for</h3>
      <p class="dim">
        A computer that is <em>unreliable</em> rather than broken. A file that was fine
        yesterday will not open. A game crashes about once a week, never in the same place.
        A build fails and building again works. The machine is fast for a minute and slow
        after that.
      </p>
      <p class="dim">
        Every one of those gets blamed on software, and none of them are. People reinstall
        the operating system over them, twice, and then buy a new computer.
      </p>
    </div>

    <div class="panel">
      <h3>Never part of a scan</h3>
      <p class="dim">
        No scan runs this, including a deep one. Choosing to have your computer checked
        carefully is not the same as agreeing to have it pinned at full load and heated, so
        it is asked for here, on its own, every time.
      </p>
    </div>

    <div class="panel">
      <h3>How long</h3>
      <p class="dim">
        Ten minutes is long enough to get a machine properly hot, which is when marginal
        hardware misbehaves. An hour is a better test. Overnight is what to do about a
        fault that appears once a week.
      </p>
      <p class="dim">
        The number of minutes is the work being asked for, not a limit on it. Nothing here
        is ever cut short for taking too long.
      </p>
    </div>
  </aside>
</div>

<style>
  .head { display: flex; align-items: center; gap: 0.9rem; margin-bottom: 0.5rem; }
  .intro { max-width: 78ch; margin: 0 0 1rem; font-size: 12.5px; }
  .bad { border-color: var(--red); color: var(--red); margin-bottom: 0.8rem; }

  .live {
    font-size: 10.5px;
    letter-spacing: 0.14em;
    text-transform: uppercase;
    color: var(--magenta);
    border: 1px solid var(--magenta);
    padding: 0.1rem 0.45rem;
    text-shadow: 0 0 10px rgba(255, 45, 149, 0.6);
    box-shadow: 0 0 18px rgba(255, 45, 149, 0.3), inset 0 0 14px rgba(255, 45, 149, 0.08);
  }

  .layout { display: grid; grid-template-columns: 1fr minmax(240px, 300px); gap: 1rem; align-items: start; }

  .controls { display: grid; gap: 0.8rem; margin-bottom: 0.8rem; }
  .row { display: flex; align-items: center; gap: 0.9rem; flex-wrap: wrap; }
  .check { display: flex; align-items: center; gap: 0.35rem; font-size: 12.5px; }
  .minutes { width: 4.5rem; }
  .share { display: flex; align-items: center; gap: 0.6rem; font-size: 12.5px; }
  .share input[type="range"] { flex: 1; max-width: 16rem; }
  .amount { min-width: 8rem; font-variant-numeric: tabular-nums; }

  .warning { border-top: 1px solid var(--line); padding-top: 0.7rem; font-size: 12.5px; }
  .warning ul { margin: 0.4rem 0 0.5rem; padding-left: 1.1rem; }
  .warning li { margin-bottom: 0.2rem; }
  .warning p { margin: 0; font-size: 11.5px; }

  .live-panel { margin-bottom: 0.8rem; }
  .bar {
    height: 6px;
    background: var(--bg-deep);
    border: 1px solid var(--line);
    overflow: hidden;
    margin-bottom: 0.6rem;
  }
  .fill {
    height: 100%;
    /* The same cyan-into-magenta the scan uses, so a machine under load reads
       as the same kind of "this is happening now" from across a room. */
    background: linear-gradient(90deg, var(--cyan), var(--magenta));
    box-shadow: var(--bloom-cyan);
    transition: width 0.4s linear;
  }
  .numbers { display: flex; gap: 1rem; font-size: 12px; flex-wrap: wrap; font-variant-numeric: tabular-nums; }
  .heat { color: var(--amber); text-shadow: 0 0 10px rgba(255, 194, 26, 0.5); }
  .blind { margin: 0.6rem 0 0; font-size: 11.5px; }

  .fault { border-color: var(--red); margin-bottom: 0.7rem; }
  .fault strong { margin-left: 0.5rem; }
  .fault p { margin: 0.4rem 0 0; font-size: 12px; }

  .result h3 { margin: 0 0 0.6rem; font-size: 13px; }
  .hot { color: var(--yellow); font-size: 12.5px; margin: 0 0 0.6rem; line-height: 1.55; }
  .counts { list-style: none; margin: 0; padding: 0; display: grid; gap: 0.3rem; font-size: 12.5px; }
  .bad-count { color: var(--red); text-shadow: var(--glow-red); }
  .heats { display: flex; gap: 0.4rem; flex-wrap: wrap; margin-top: 0.7rem; }
  .chip {
    font-size: 11px;
    border: 1px solid var(--line);
    padding: 0.1rem 0.45rem;
    color: var(--text-dim);
  }
  .chip strong { color: var(--amber); }
  .caveat { margin-top: 0.7rem; font-size: 11.5px; line-height: 1.6; }
  .result p.dim { margin: 0.6rem 0 0; font-size: 11.5px; line-height: 1.6; }

  aside { display: grid; gap: 0.7rem; }
  aside h3 { margin: 0 0 0.5rem; font-size: 11px; text-transform: uppercase; letter-spacing: 0.14em; color: var(--text-dim); }
  aside p { font-size: 11.5px; margin: 0 0 0.5rem; line-height: 1.55; }
  aside p:last-child { margin-bottom: 0; }
</style>
