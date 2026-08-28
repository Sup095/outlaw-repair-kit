<script lang="ts">
  import { type Finding } from "./api";
  import { scan, runScan, explainFindings, cancelScan } from "./scan.svelte";

  // Everything the run consists of lives in `scan`, outside this component, so
  // that switching to another tab and back does not throw away a scan that may
  // have taken an hour. See lib/scan.svelte.ts.

  const order = ["critical", "high", "medium", "low", "info"];

  const findings = $derived<Finding[]>(
    (scan.report?.outcomes ?? [])
      .flatMap((outcome) => outcome.findings ?? [])
      .sort((a, b) => order.indexOf(a.severity) - order.indexOf(b.severity)),
  );

  // Checks for another operating system are left out, exactly as the command
  // line leaves them out. Each one is still announced inline as the scan goes
  // past, so nothing is hidden; this list is there to say what could have been
  // checked on *this* machine and was not, and "the Linux disk check does not
  // run on Windows" is not that. Listing them also put two identically named
  // checks side by side -- one skipped for want of rights, one for being a
  // different platform's -- which read as a fault in the tool.
  const skipped = $derived(
    (scan.report?.outcomes ?? []).filter(
      (outcome) =>
        outcome.status?.status === "skipped" &&
        outcome.status?.reason !== "unsupported-platform",
    ),
  );
</script>

<div class="controls panel">
  <label>
    <span class="dim">Thoroughness</span>
    <select bind:value={scan.tier} disabled={scan.running}>
      <option value="quick">Quick — minutes</option>
      <option value="full">Full — tens of minutes</option>
      <option value="deep">Deep — an hour or more</option>
    </select>
  </label>
  <button class="primary" onclick={runScan} disabled={scan.running}>{scan.running ? "Scanning…" : "Run scan"}</button>
  <!-- Always available. No scan is ever ended by a clock, only by a person. -->
  <button class="danger" onclick={cancelScan} disabled={!scan.running}>Stop</button>
  <button onclick={explainFindings} disabled={!scan.report || scan.explaining}>
    {scan.explaining ? "Thinking…" : "Explain findings"}
  </button>
</div>

{#if scan.tier === "deep"}
  <!-- Say what it adds *and* what it does not, so nobody picks it expecting
       more and concludes the tool is broken when it finishes. -->
  <p class="dim note">
    Adds the system file check: verifies that the operating system's own files still
    match what installed them. It reads and hashes most of what is installed, so it
    takes minutes to an hour — and there is no time limit on it, only the Stop button.
    On Windows it needs administrator rights and says so if it does not have them.
    The rootkit scan this tier is also meant for is not built yet. Stress and burn-in
    is built, and is on its own tab — no scan will heat your machine.
  </p>
{:else if scan.tier === "full"}
  <p class="dim note">
    Adds the disk health check and the application launch test, which starts catalogued
    applications such as Steam and closes them again.
  </p>
{/if}

{#if !scan.report && !scan.running && !scan.error}
  <!-- Something to look at, and something worth reading, instead of a screen
       that is blank until you press a button. Somebody opening this for the
       first time should be able to tell what it is about to do to their
       computer before they ask it to. -->
  <div class="idle panel">
    <div class="idle-mark" aria-hidden="true">
      <span class="ring"></span>
      <span class="ring two"></span>
      <span class="dot"></span>
    </div>
    <div class="idle-words">
      <h3>Standing by</h3>
      <p class="dim">
        Nothing has been read from this machine yet. A scan runs a list of checks and
        reports what it finds — it changes nothing on its own, and anything worth fixing
        goes to the <strong>Queue</strong> for you to work through one at a time.
      </p>
      <dl class="tiers">
        <div><dt>Quick</dt><dd class="dim">Disks, memory, processes, drivers, services, logs, launchers.</dd></div>
        <div><dt>Full</dt><dd class="dim">Adds drive health and starting applications for real.</dd></div>
        <div><dt>Deep</dt><dd class="dim">Adds verifying the operating system's own files against what installed them.</dd></div>
      </dl>
      <p class="dim hint">
        No tier has a time limit. <strong>Stop</strong> is available the whole way through.
      </p>
    </div>
  </div>
{/if}

{#if scan.progress}
  <div class="progress panel">
    <div class="track"><div class="fill" style="width: {(scan.progress.index / Math.max(scan.progress.total, 1)) * 100}%"></div></div>
    <span class="dim">{scan.progress.index} of {scan.progress.total} — {scan.progress.name}</span>
  </div>
{/if}

{#if scan.error}
  <div class="panel bad">{scan.error}</div>
{/if}

{#if scan.report}
  <div class="summary panel">
    <strong>{findings.length}</strong> finding{findings.length === 1 ? "" : "s"}
    <span class="dim">
      · {scan.report.outcomes.length} checks considered · {skipped.length} skipped
      {#if scan.report.cancelled}· stopped early{/if}
    </span>
  </div>

  {#each findings as finding (finding.id + (finding.subject ?? ""))}
    <article class="finding panel">
      <header>
        <span class="sev {finding.severity}">{finding.severity}</span>
        <h3>{finding.title}</h3>
        <span class="dim subject">{finding.subject ?? ""}</span>
      </header>
      <p>{finding.detail}</p>
      {#if finding.evidence?.length}
        <dl>
          {#each finding.evidence as item (item.label)}
            <div><dt class="dim">{item.label}</dt><dd>{item.value}</dd></div>
          {/each}
        </dl>
      {/if}
      {#if finding.remediation_hint}
        <p class="suggestion">{finding.remediation_hint}</p>
      {/if}
      {#if scan.explanation?.analysis?.items}
        {#each scan.explanation.analysis.items.filter((item: any) => item.finding_id === finding.id) as item (item.title)}
          <div class="explained">
            <span class="dim">
              {item.source?.entry_id ? `known problem: ${item.source.entry_id}` : item.source?.model ? `reasoned by ${item.source.model}` : "no known answer"}
            </span>
            <p>{item.explanation}</p>
          </div>
        {/each}
      {/if}
    </article>
  {/each}

  {#if skipped.length}
    <details class="panel">
      <!-- Skipped checks are listed rather than hidden: a gap you cannot see
           is indistinguishable from a clean result. -->
      <summary>{skipped.length} check{skipped.length === 1 ? "" : "s"} did not run</summary>
      <ul>
        {#each skipped as outcome (outcome.probe)}
          <!-- The sentence the back end wrote, not the tag it is keyed by.
               This used to print `"requires-elevation"`, quotes and all. -->
          <li>{outcome.name} — <span class="dim">{outcome.skipped_because ?? outcome.status.reason ?? ""}</span></li>
        {/each}
      </ul>
    </details>
  {/if}
{/if}

<style>
  /* The idle panel. A slow pulse rather than a spinner: a spinner says "wait",
     and nothing here is being waited for. */
  .idle {
    display: flex;
    gap: 1.6rem;
    align-items: flex-start;
    margin-bottom: 0.8rem;
  }

  .idle-mark {
    position: relative;
    width: 88px;
    height: 88px;
    flex: none;
    margin: 0.4rem 0 0 0.3rem;
  }

  .idle-mark .ring,
  .idle-mark .dot {
    position: absolute;
    border-radius: 50%;
    left: 50%;
    top: 50%;
    transform: translate(-50%, -50%);
  }

  .idle-mark .ring {
    width: 100%;
    height: 100%;
    border: 1px solid var(--cyan);
    box-shadow: 0 0 16px rgba(0, 240, 255, 0.5), inset 0 0 16px rgba(0, 240, 255, 0.18);
    animation: sweep 3.4s ease-in-out infinite;
  }

  .idle-mark .ring.two {
    width: 62%;
    height: 62%;
    border-color: var(--magenta);
    box-shadow: 0 0 16px rgba(255, 45, 149, 0.5), inset 0 0 16px rgba(255, 45, 149, 0.18);
    animation-delay: 1.1s;
  }

  .idle-mark .dot {
    width: 7px;
    height: 7px;
    background: var(--amber);
    box-shadow: var(--glow-amber), 0 0 24px rgba(255, 194, 26, 0.6);
  }

  @keyframes sweep {
    0%, 100% { opacity: 0.25; transform: translate(-50%, -50%) scale(0.92); }
    50% { opacity: 0.9; transform: translate(-50%, -50%) scale(1); }
  }

  .idle-words { min-width: 0; }
  .idle-words h3 { margin-bottom: 0.45rem; }
  .idle-words p { margin: 0 0 0.7rem; max-width: 74ch; font-size: 12.5px; }

  .tiers { margin: 0 0 0.7rem; display: grid; gap: 0.25rem; font-size: 12px; }
  .tiers div { display: flex; gap: 0.8rem; }
  .tiers dt {
    color: var(--cyan);
    min-width: 5.5ch;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    font-size: 11px;
    padding-top: 0.1rem;
  }
  .tiers dd { margin: 0; }

  .hint { margin-bottom: 0 !important; }

  .note { max-width: 66ch; margin: 0.4rem 0 0.8rem; font-size: 12.5px; }
  .controls {
    display: flex;
    align-items: end;
    gap: 1rem;
    margin-bottom: 1rem;
  }

  .controls label {
    display: grid;
    gap: 0.25rem;
    font-size: 12px;
    min-width: 15rem;
  }

  .progress {
    margin-bottom: 1rem;
    display: grid;
    gap: 0.5rem;
    font-size: 12px;
  }

  .track {
    height: 6px;
    background: var(--bg-deep);
    border: 1px solid var(--line);
  }

  /* The bar runs cyan into magenta and casts light on the track around it, so
     a scan in progress is visible from across a room. */
  .fill {
    height: 100%;
    background: linear-gradient(90deg, var(--cyan), var(--violet) 60%, var(--magenta));
    box-shadow: 0 0 14px rgba(0, 240, 255, 0.65), 0 0 28px rgba(255, 45, 149, 0.4);
    transition: width 200ms ease;
  }

  .summary {
    margin-bottom: 1rem;
  }

  .bad {
    border-color: var(--red);
    color: var(--red);
    margin-bottom: 1rem;
    box-shadow: 0 0 22px rgba(255, 45, 85, 0.25), inset 0 1px 0 rgba(255, 45, 85, 0.35);
  }

  .finding {
    margin-bottom: 0.75rem;
  }

  .finding header {
    display: flex;
    align-items: baseline;
    gap: 0.75rem;
    margin-bottom: 0.5rem;
  }

  .finding h3 {
    font-size: 13px;
  }

  .subject {
    margin-left: auto;
    font-size: 12px;
  }



  .finding p {
    margin: 0 0 0.5rem;
  }

  dl {
    margin: 0.5rem 0 0;
    display: grid;
    gap: 0.2rem;
    font-size: 12.5px;
  }

  dl div {
    display: flex;
    gap: 0.6rem;
  }

  dt {
    min-width: 9rem;
  }

  dd {
    margin: 0;
  }

  .suggestion {
    border-left: 2px solid var(--amber-dim);
    padding-left: 0.7rem;
    color: var(--amber);
  }

  .explained {
    margin-top: 0.7rem;
    border-top: 1px solid var(--line);
    padding-top: 0.6rem;
    font-size: 12.5px;
  }

  ul {
    margin: 0.6rem 0 0;
    padding-left: 1.2rem;
  }

  summary {
    cursor: pointer;
  }
</style>
