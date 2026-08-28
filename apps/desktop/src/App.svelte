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
  import StressView from "./lib/StressView.svelte";
  import ReportView from "./lib/ReportView.svelte";
  import InfoView from "./lib/InfoView.svelte";
  import type { BootReport } from "./lib/api";

  let booted = $state<BootReport | null>(null);
  let view = $state<
    | "scan"
    | "checks"
    | "watch"
    | "stress"
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
    // After Watching, because that is the order of escalation: look once, keep
    // looking, and then -- when neither has explained it -- make the machine
    // misbehave on purpose.
    { id: "stress", label: "Stress" },
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
      {:else if view === "stress"}
        <StressView />
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
    border-bottom: 1px solid var(--line-bright);
    background:
      linear-gradient(180deg, rgba(0, 240, 255, 0.05), transparent 60%),
      linear-gradient(180deg, #10102a 0%, #07070f 100%);
    box-shadow: 0 8px 32px -14px rgba(0, 240, 255, 0.35);
  }

  /* The rule under the header runs cyan to magenta and back, so the two
     colours the rest of the window is built from are stated once, at the top,
     before anything else. */
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
      var(--cyan) 14%,
      var(--violet) 38%,
      var(--magenta) 58%,
      var(--amber) 82%,
      transparent
    );
    box-shadow: 0 0 12px rgba(0, 240, 255, 0.5), 0 0 20px rgba(255, 45, 149, 0.3);
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
    width: 160px;
    height: 1px;
    background: linear-gradient(90deg, transparent, #fff, transparent);
    box-shadow: 0 0 14px rgba(255, 255, 255, 0.9), 0 0 28px rgba(0, 240, 255, 0.7);
    animation: run 8s linear infinite;
    pointer-events: none;
  }

  @keyframes run {
    0% { transform: translateX(-180px); opacity: 0; }
    8% { opacity: 1; }
    92% { opacity: 1; }
    100% { transform: translateX(100vw); opacity: 0; }
  }

  .brand {
    display: flex;
    align-items: baseline;
    gap: 0.55rem;
  }

  /* The wordmark is split into two offset colour copies behind the real one --
     the misconvergence of a screen that is not quite aligned. Static, and
     under a pixel, so it reads as a property of the display rather than as an
     effect. */
  .mark {
    position: relative;
    color: var(--amber);
    font-weight: 700;
    letter-spacing: 0.24em;
    text-shadow:
      var(--glow-amber),
      0 0 28px rgba(255, 194, 26, 0.45),
      1px 0 0 rgba(255, 45, 149, 0.55),
      -1px 0 0 rgba(0, 240, 255, 0.55);
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
    color: var(--text-dim);
  }

  nav button:hover:not(.active) {
    box-shadow: none;
    background: rgba(0, 240, 255, 0.06);
    border-bottom-color: var(--cyan);
    color: var(--cyan);
    text-shadow: var(--glow-cyan);
  }

  /* The active tab is the one thing on this screen that is always lit: a
     magenta underline with the glow spilling up behind the label. */
  nav button.active {
    color: var(--amber);
    border-bottom-color: var(--magenta);
    text-shadow: var(--glow-amber), 0 0 26px rgba(255, 194, 26, 0.4);
    background: linear-gradient(180deg, transparent 55%, rgba(255, 45, 149, 0.14));
    box-shadow: 0 10px 22px -12px var(--magenta);
  }

  nav button.active::after {
    /* The sweep belongs to buttons you press. The active tab is a state, not
       an action, so it does not shimmer every time the pointer crosses it. */
    display: none;
  }

  .meta {
    margin-left: auto;
    display: flex;
    align-items: center;
    gap: 1rem;
    font-size: 12px;
  }

  .version {
    color: var(--cyan);
    letter-spacing: 0.1em;
    text-shadow: var(--glow-cyan);
  }

  .warn {
    color: var(--yellow);
    border: 1px solid var(--yellow);
    padding: 0.15rem 0.5rem;
    cursor: help;
    text-shadow: 0 0 10px rgba(255, 217, 61, 0.6);
    box-shadow: 0 0 16px rgba(255, 217, 61, 0.22), inset 0 0 14px rgba(255, 217, 61, 0.07);
  }

  .update {
    display: flex;
    align-items: center;
    gap: 1rem;
    padding: 0.6rem 1.25rem;
    background: linear-gradient(90deg, rgba(255, 194, 26, 0.14), transparent 70%);
    border-bottom: 1px solid var(--amber-dim);
    color: var(--amber);
    font-size: 12.5px;
    text-shadow: var(--glow-amber);
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
    border-top: 1px solid var(--line-bright);
    padding: 0.5rem 1.25rem;
    font-size: 11.5px;
    background: linear-gradient(180deg, transparent, rgba(3, 3, 8, 0.9));
    box-shadow: 0 -8px 26px -16px rgba(255, 45, 149, 0.55);
  }
</style>
