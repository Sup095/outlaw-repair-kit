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
       the stress tests and concludes the tool is broken when it finishes. -->
  <p class="dim note">
    Adds the system file check: verifies that the operating system's own files still
    match what installed them. It reads and hashes most of what is installed, so it
    takes minutes to an hour — and there is no time limit on it, only the Stop button.
    On Windows it needs administrator rights and says so if it does not have them.
    The stress and burn-in tests this tier is also meant for are not built yet.
  </p>
{:else if scan.tier === "full"}
  <p class="dim note">
    Adds the disk health check and the application launch test, which starts catalogued
    applications such as Steam and closes them again.
  </p>
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
    background: #1b2029;
  }

  .fill {
    height: 100%;
    background: var(--cyan);
    transition: width 200ms ease;
  }

  .summary {
    margin-bottom: 1rem;
  }

  .bad {
    border-color: var(--red);
    color: var(--red);
    margin-bottom: 1rem;
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

  .sev {
    text-transform: uppercase;
    font-size: 10.5px;
    letter-spacing: 0.14em;
    padding: 0.1rem 0.45rem;
    border: 1px solid currentColor;
  }

  .sev.critical { color: #fff; background: var(--red); border-color: var(--red); }
  .sev.high { color: var(--red); }
  .sev.medium { color: var(--yellow); }
  .sev.low { color: var(--cyan); }
  .sev.info { color: var(--text-dim); }

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
