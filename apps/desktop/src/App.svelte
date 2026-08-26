<script lang="ts">
  import BootScreen from "./lib/BootScreen.svelte";
  import ScanView from "./lib/ScanView.svelte";
  import QueueView from "./lib/QueueView.svelte";
  import ModelsView from "./lib/ModelsView.svelte";
  import MachinesView from "./lib/MachinesView.svelte";
  import SettingsView from "./lib/SettingsView.svelte";
  import AuditView from "./lib/AuditView.svelte";
  import ChecksView from "./lib/ChecksView.svelte";
  import WatchView from "./lib/WatchView.svelte";
  import ReportView from "./lib/ReportView.svelte";
  import InfoView from "./lib/InfoView.svelte";
  import type { BootReport } from "./lib/api";

  let booted = $state<BootReport | null>(null);
  let view = $state<
    | "scan"
    | "checks"
    | "watch"
    | "queue"
    | "models"
    | "machines"
    | "settings"
    | "audit"
    | "report"
    | "info"
  >("scan");
  let updateDismissed = $state(false);

  const tabs = [
    { id: "scan", label: "Scan" },
    { id: "checks", label: "Checks" },
    // Beside Scan, because it is the same work on a timer rather than on a
    // button, and somebody who has just run a scan is exactly the person who
    // wants to be told when the answer changes.
    { id: "watch", label: "Watching" },
    { id: "queue", label: "Queue" },
    { id: "models", label: "Models" },
    { id: "machines", label: "Machines" },
    { id: "settings", label: "Settings" },
    { id: "audit", label: "Audit" },
    { id: "report", label: "Report a problem" },
    // Last, and after the problem-reporting screen, because that is the order
    // somebody arrives at them: something is wrong, then how do I say so, then
    // what is this thing anyway.
    { id: "info", label: "Info" },
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
        <span class="version">v{booted.version}</span>
        {#if warnings.length}
          <span class="warn" title={warnings.map((w) => `${w.name}: ${w.detail}`).join("\n")}>
            {warnings.length} start-up warning{warnings.length === 1 ? "" : "s"}
          </span>
        {/if}
      </div>
      <span class="pulse" aria-hidden="true"></span>
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
      {:else if view === "checks"}
        <ChecksView />
      {:else if view === "watch"}
        <WatchView />
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
      {:else if view === "report"}
        <ReportView />
      {:else}
        <InfoView {booted} />
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

  /* A rule of light under the header, brightest in the middle, so the top of
     the window reads as powered rather than merely drawn. */
  header {
    position: relative;
    display: flex;
    align-items: center;
    gap: 2rem;
    padding: 0.75rem 1.25rem;
    border-bottom: 1px solid var(--line);
    background:
      linear-gradient(180deg, #141a26 0%, #0a0d14 100%);
  }

  header::after {
    content: "";
    position: absolute;
    left: 0;
    right: 0;
    bottom: -1px;
    height: 1px;
    background: linear-gradient(
      90deg,
      transparent,
      var(--amber-dim) 18%,
      var(--amber) 50%,
      var(--amber-dim) 82%,
      transparent
    );
    opacity: 0.75;
  }

  /* A pulse that travels along that rule, once every eight seconds. It is
     confined to a one-pixel line at the top of the window, well away from
     anything anybody is reading, and it is the only thing on this screen that
     moves on its own -- which is the point. It says the tool is running
     without asking for a glance. */
  .pulse {
    position: absolute;
    left: 0;
    bottom: -1px;
    width: 120px;
    height: 1px;
    background: linear-gradient(90deg, transparent, var(--cyan), transparent);
    box-shadow: var(--glow-cyan);
    animation: run 8s linear infinite;
    pointer-events: none;
  }

  @keyframes run {
    0% { transform: translateX(-140px); opacity: 0; }
    8% { opacity: 1; }
    92% { opacity: 1; }
    100% { transform: translateX(100vw); opacity: 0; }
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
    text-shadow: var(--glow-amber);
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
    box-shadow: none;
  }

  nav button:hover:not(.active) {
    box-shadow: none;
    background: rgba(34, 224, 226, 0.05);
    border-bottom-color: var(--cyan-dim);
    color: var(--cyan);
    text-shadow: var(--glow-cyan);
  }

  /* The active tab is the one thing on this screen that is always lit. */
  nav button.active {
    color: var(--amber);
    border-bottom-color: var(--amber);
    text-shadow: var(--glow-amber);
    box-shadow: 0 6px 14px -8px var(--amber);
  }

  .meta {
    margin-left: auto;
    display: flex;
    align-items: center;
    gap: 1rem;
    font-size: 12px;
  }

  .version {
    color: var(--cyan-dim);
    letter-spacing: 0.1em;
  }

  .warn {
    color: var(--yellow);
    border: 1px solid #4a3a12;
    padding: 0.15rem 0.5rem;
    cursor: help;
    text-shadow: 0 0 10px rgba(251, 191, 36, 0.4);
  }

  .update {
    display: flex;
    align-items: center;
    gap: 1rem;
    padding: 0.6rem 1.25rem;
    background: linear-gradient(90deg, #1f1809, #14100633);
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
    position: relative;
    border-top: 1px solid var(--line);
    padding: 0.5rem 1.25rem;
    font-size: 11.5px;
    background: linear-gradient(180deg, transparent, rgba(4, 5, 10, 0.85));
  }
</style>
