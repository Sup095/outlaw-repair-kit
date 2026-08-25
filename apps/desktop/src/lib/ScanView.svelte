<script lang="ts">
  import { api, onScanEvent, type Finding, type ScanEvent, type ScanReport } from "./api";

  let tier = $state("quick");
  let running = $state(false);
  let progress = $state<{ index: number; total: number; name: string } | null>(null);
  let report = $state<ScanReport | null>(null);
  let error = $state<string | null>(null);
  let explanation = $state<any | null>(null);
  let explaining = $state(false);

  const order = ["critical", "high", "medium", "low", "info"];

  const findings = $derived<Finding[]>(
    (report?.outcomes ?? [])
      .flatMap((outcome) => outcome.findings ?? [])
      .sort((a, b) => order.indexOf(a.severity) - order.indexOf(b.severity)),
  );

  const skipped = $derived(
    (report?.outcomes ?? []).filter((outcome) => outcome.status?.status === "skipped"),
  );

  async function run() {
    error = null;
    explanation = null;
    report = null;
    running = true;
    progress = null;

    // Every check reports as it finishes, so a long scan never looks stuck.
    const unlisten = await onScanEvent((event: ScanEvent) => {
      if (event.event === "probe-started") {
        progress = { index: event.index ?? 0, total: event.total ?? 0, name: event.name ?? "" };
      }
    });

    try {
      report = await api.startScan(tier);
    } catch (problem) {
      error = String(problem);
    } finally {
      unlisten();
      running = false;
      progress = null;
    }
  }

  async function explain() {
    if (!report) return;
    explaining = true;
    error = null;
    try {
      explanation = await api.explain(report);
    } catch (problem) {
      error = String(problem);
    } finally {
      explaining = false;
    }
  }
</script>

<div class="controls panel">
  <label>
    <span class="dim">Thoroughness</span>
    <select bind:value={tier} disabled={running}>
      <option value="quick">Quick — minutes</option>
      <option value="full">Full — tens of minutes</option>
      <option value="deep">Deep — hours, no cap</option>
    </select>
  </label>
  <button class="primary" onclick={run} disabled={running}>{running ? "Scanning…" : "Run scan"}</button>
  <!-- Always available. No scan is ever ended by a clock, only by a person. -->
  <button class="danger" onclick={() => api.cancelScan()} disabled={!running}>Stop</button>
  <button onclick={explain} disabled={!report || explaining}>
    {explaining ? "Thinking…" : "Explain findings"}
  </button>
</div>

{#if progress}
  <div class="progress panel">
    <div class="track"><div class="fill" style="width: {(progress.index / Math.max(progress.total, 1)) * 100}%"></div></div>
    <span class="dim">{progress.index} of {progress.total} — {progress.name}</span>
  </div>
{/if}

{#if error}
  <div class="panel bad">{error}</div>
{/if}

{#if report}
  <div class="summary panel">
    <strong>{findings.length}</strong> finding{findings.length === 1 ? "" : "s"}
    <span class="dim">
      · {report.outcomes.length} checks considered · {skipped.length} skipped
      {#if report.cancelled}· stopped early{/if}
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
      {#if explanation?.analysis?.items}
        {#each explanation.analysis.items.filter((item: any) => item.finding_id === finding.id) as item (item.title)}
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
          <li>{outcome.name} — <span class="dim">{JSON.stringify(outcome.status.reason ?? "")}</span></li>
        {/each}
      </ul>
    </details>
  {/if}
{/if}

<style>
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
