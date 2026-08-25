<script lang="ts">
  import BootScreen from "./lib/BootScreen.svelte";
  import ScanView from "./lib/ScanView.svelte";
  import QueueView from "./lib/QueueView.svelte";
  import ModelsView from "./lib/ModelsView.svelte";
  import MachinesView from "./lib/MachinesView.svelte";
  import SettingsView from "./lib/SettingsView.svelte";
  import AuditView from "./lib/AuditView.svelte";
  import ReportView from "./lib/ReportView.svelte";
  import type { BootReport } from "./lib/api";

  let booted = $state<BootReport | null>(null);
  let view = $state<
    "scan" | "queue" | "models" | "machines" | "settings" | "audit" | "report"
  >("scan");
  let updateDismissed = $state(false);

  const tabs = [
    { id: "scan", label: "Scan" },
    { id: "queue", label: "Queue" },
    { id: "models", label: "Models" },
    { id: "machines", label: "Machines" },
    { id: "settings", label: "Settings" },
    { id: "audit", label: "Audit" },
    { id: "report", label: "Report a problem" },
  ] as const;

  const warnings = $derived(
    booted?.selftest.checks.filter((check) => check.state !== "pass") ?? [],
  );
</script>

{#if !booted}
  <BootScreen onready={(report) => (booted = report)} />
{:else}
  <div class="shell">
    <header>
      <div class="brand">
        <span class="mark">OUTLAW</span>
        <span class="sub">Repair Kit</span>
      </div>
      <nav>
        {#each tabs as tab (tab.id)}
          <button class:active={view === tab.id} onclick={() => (view = tab.id)}>{tab.label}</button>
        {/each}
      </nav>
      <div class="meta">
        <span class="dim">v{booted.version}</span>
        {#if warnings.length}
          <span class="warn" title={warnings.map((w) => `${w.name}: ${w.detail}`).join("\n")}>
            {warnings.length} start-up warning{warnings.length === 1 ? "" : "s"}
          </span>
        {/if}
      </div>
    </header>

    {#if booted.update.state === "available" && !updateDismissed}
      <div class="update">
        <span>Version {booted.update.latest} is available — this is v{booted.update.current}.</span>
        <!-- Reported, never installed. Replacing the program is the user's call. -->
        <code>{booted.update.url}</code>
        <button onclick={() => (updateDismissed = true)}>Dismiss</button>
      </div>
    {/if}

    <main>
      {#if view === "scan"}
        <ScanView />
      {:else if view === "queue"}
        <QueueView />
      {:else if view === "models"}
        <ModelsView />
      {:else if view === "machines"}
        <MachinesView />
      {:else if view === "settings"}
        <SettingsView />
      {:else if view === "audit"}
        <AuditView />
      {:else}
        <ReportView />
      {/if}
    </main>

    <footer class="dim">
      Made by Outlaw Systems, in collaboration with AI. Nothing is changed without a snapshot first.
    </footer>
  </div>
{/if}

<style>
  .shell {
    display: flex;
    flex-direction: column;
    height: 100%;
  }

  header {
    display: flex;
    align-items: center;
    gap: 2rem;
    padding: 0.75rem 1.25rem;
    border-bottom: 1px solid var(--line);
    background: linear-gradient(180deg, #12161e, #0c0f15);
  }

  .brand {
    display: flex;
    align-items: baseline;
    gap: 0.55rem;
  }

  .mark {
    color: var(--amber);
    font-weight: 700;
    letter-spacing: 0.24em;
  }

  .sub {
    color: var(--text-dim);
    letter-spacing: 0.14em;
    font-size: 12px;
    text-transform: uppercase;
  }

  nav {
    display: flex;
    gap: 0.4rem;
  }

  nav button {
    background: transparent;
    border-color: transparent;
    border-bottom: 2px solid transparent;
  }

  nav button.active {
    color: var(--amber);
    border-bottom-color: var(--amber);
  }

  .meta {
    margin-left: auto;
    display: flex;
    align-items: center;
    gap: 1rem;
    font-size: 12px;
  }

  .warn {
    color: var(--yellow);
    border: 1px solid #4a3a12;
    padding: 0.15rem 0.5rem;
    cursor: help;
  }

  .update {
    display: flex;
    align-items: center;
    gap: 1rem;
    padding: 0.6rem 1.25rem;
    background: #1b1508;
    border-bottom: 1px solid #4a3a12;
    color: var(--amber);
    font-size: 12.5px;
  }

  .update button {
    margin-left: auto;
  }

  main {
    flex: 1;
    overflow: auto;
    padding: 1.25rem;
  }

  footer {
    border-top: 1px solid var(--line);
    padding: 0.5rem 1.25rem;
    font-size: 11.5px;
  }
</style>
