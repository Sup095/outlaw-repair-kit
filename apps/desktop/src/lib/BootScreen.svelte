<script lang="ts">
  import { onMount } from "svelte";
  import { api, onBootEvent, type BootEvent, type BootReport, type CheckState } from "./api";

  const { onready }: { onready: (report: BootReport) => void } = $props();

  let progress = $state(0);
  let lines = $state<{ text: string; state: CheckState }[]>([]);
  let report = $state<BootReport | null>(null);
  let failed = $state(false);
  let error = $state<string | null>(null);

  /** How many log lines the pane keeps. Older ones scroll off. */
  const LOG_LINES = 5;

  function stateOf(event: BootEvent): CheckState {
    if (event.kind === "check") return event.result?.state ?? "pass";
    if (event.kind === "update") return event.status?.state === "up_to_date" ? "pass" : "warn";
    if (event.kind === "finished") return event.ready ? "pass" : "fail";
    return "pass";
  }

  function push(text: string, state: CheckState) {
    lines = [...lines, { text, state }].slice(-LOG_LINES);
  }

  onMount(() => {
    let unlisten: (() => void) | undefined;

    (async () => {
      try {
        // Inside the try, and deliberately. Subscribing can fail on its own --
        // it is a call into the shell like any other -- and when it did, the
        // rejection escaped this function with nothing to catch it: no error,
        // no log line, a progress bar frozen at nothing, and no way in. A
        // start-up screen that can fail silently is worse than one that has no
        // checks at all.
        unlisten = await onBootEvent((event) => {
          if (event.total_steps && event.step) {
            progress = event.step / event.total_steps;
          }
          if (event.kind === "finished") progress = 1;

          const text =
            event.kind === "check" && event.result
              ? `${event.result.name} — ${event.result.detail}`
              : (event.line ?? "");
          if (text) push(text, stateOf(event));
        });

        const finished = await api.boot();
        report = finished;
        failed = !finished.selftest.checks.every((check) => check.state !== "fail");
        // A beat on the finished screen, so the result is readable rather than
        // a flash. Not a loading delay -- the work is already done.
        if (!failed) setTimeout(() => onready(finished), 900);
      } catch (problem) {
        error = String(problem);
        failed = true;
        // A start-up that cannot even report on itself must still leave a way
        // into the application, or a broken check becomes a locked door.
        report = {
          version: "unknown",
          selftest: { checks: [] },
          update: { state: "unknown", reason: "start-up did not finish" },
        };
        push(error, "fail");
        progress = 1;
      }
    })();

    return () => unlisten?.();
  });

  const colours: Record<CheckState, string> = {
    pass: "var(--cyan)",
    warn: "var(--yellow)",
    fail: "var(--red)",
  };
</script>

<div class="boot">
  <div class="grid" aria-hidden="true"></div>
  <div class="glow" aria-hidden="true"></div>

  <div class="content">
    <div class="title" data-text="OUTLAW">
      <span>OUTLAW</span>
    </div>
    <div class="subtitle">REPAIR&nbsp;KIT</div>
    <div class="maker">by Outlaw Systems{#if report}<span class="version"> · v{report.version}</span>{/if}</div>

    <div class="bar" role="progressbar" aria-valuenow={Math.round(progress * 100)} aria-valuemin="0" aria-valuemax="100">
      {#each Array(40) as _, index (index)}
        <span class="seg" class:on={index / 40 < progress}></span>
      {/each}
    </div>
    <div class="percent">{Math.round(progress * 100)}%</div>

    <div class="log" aria-live="polite">
      {#each lines as line, index (line.text + index)}
        <div class="line" style="color: {colours[line.state]}; opacity: {0.45 + 0.55 * ((index + 1) / lines.length)}">
          <span class="caret">›</span> {line.text}
        </div>
      {/each}
    </div>

    {#if failed}
      <div class="failed">
        <p>{error ?? "Start-up checks failed. Fix the problems above before trusting this run."}</p>
        <button onclick={() => report && onready(report)} disabled={!report}>Continue anyway</button>
      </div>
    {/if}
  </div>
</div>

<style>
  .boot {
    position: fixed;
    inset: 0;
    display: grid;
    place-items: center;
    background: radial-gradient(ellipse at 50% 40%, #131a24 0%, #07080b 70%);
    overflow: hidden;
  }

  /* A perspective grid receding to the horizon: the cheapest way to make a
     flat panel feel like a place. */
  .grid {
    position: absolute;
    inset: -50% -50% -20% -50%;
    background-image: linear-gradient(rgba(34, 224, 226, 0.13) 1px, transparent 1px),
      linear-gradient(90deg, rgba(34, 224, 226, 0.13) 1px, transparent 1px);
    background-size: 46px 46px;
    transform: perspective(420px) rotateX(66deg);
    animation: drift 6s linear infinite;
    mask-image: linear-gradient(to top, #000 5%, transparent 62%);
  }

  @keyframes drift {
    to {
      background-position: 0 46px;
    }
  }

  .glow {
    position: absolute;
    inset: 0;
    background: radial-gradient(circle at 50% 30%, rgba(255, 176, 0, 0.09), transparent 55%);
    pointer-events: none;
  }

  .content {
    position: relative;
    text-align: center;
    width: min(680px, 88vw);
  }

  .title {
    position: relative;
    font-size: clamp(3rem, 11vw, 6.5rem);
    font-weight: 700;
    letter-spacing: 0.22em;
    color: var(--amber);
    text-shadow: 0 0 26px rgba(255, 176, 0, 0.45);
    line-height: 1;
  }

  /* Chromatic split, offset a hair each way and jittered occasionally. */
  .title::before,
  .title::after {
    content: attr(data-text);
    position: absolute;
    inset: 0;
    letter-spacing: 0.22em;
  }

  .title::before {
    color: var(--cyan);
    animation: glitch-a 4.5s infinite steps(1);
  }

  .title::after {
    color: #ff3b6b;
    animation: glitch-b 4.5s infinite steps(1);
  }

  @keyframes glitch-a {
    0%, 92%, 100% { transform: translate(-2px, 0); clip-path: inset(0 0 62% 0); opacity: 0.45; }
    94% { transform: translate(-7px, -2px); opacity: 0.85; }
    97% { transform: translate(4px, 1px); clip-path: inset(48% 0 0 0); }
  }

  @keyframes glitch-b {
    0%, 92%, 100% { transform: translate(2px, 0); clip-path: inset(58% 0 0 0); opacity: 0.35; }
    95% { transform: translate(6px, 2px); opacity: 0.7; }
    98% { transform: translate(-4px, -1px); clip-path: inset(0 0 55% 0); }
  }

  .subtitle {
    font-size: clamp(0.9rem, 2.6vw, 1.5rem);
    letter-spacing: 0.72em;
    color: var(--text);
    margin: 0.35rem 0 0 0.6em;
  }

  .maker {
    margin-top: 0.9rem;
    color: var(--text-dim);
    letter-spacing: 0.18em;
    font-size: 12px;
    text-transform: uppercase;
  }

  .version {
    color: var(--amber-dim);
  }

  .bar {
    display: flex;
    gap: 3px;
    margin: 2.2rem auto 0;
    width: 100%;
  }

  .seg {
    flex: 1;
    height: 12px;
    background: #1b2029;
    border: 1px solid #232a36;
    transition: background 180ms ease, box-shadow 180ms ease;
  }

  .seg.on {
    background: var(--amber);
    border-color: var(--amber);
    box-shadow: 0 0 9px rgba(255, 176, 0, 0.65);
  }

  .percent {
    margin-top: 0.5rem;
    color: var(--amber);
    letter-spacing: 0.2em;
    font-size: 12px;
  }

  .log {
    margin-top: 1.8rem;
    text-align: left;
    height: calc(5 * 1.55em);
    font-size: 12.5px;
    border-left: 2px solid var(--line);
    padding-left: 0.9rem;
  }

  .line {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    animation: in 220ms ease-out;
  }

  @keyframes in {
    from { transform: translateY(6px); opacity: 0; }
  }

  .caret {
    color: var(--amber-dim);
  }

  .failed {
    margin-top: 1.6rem;
    border: 1px solid var(--red);
    padding: 1rem;
    text-align: left;
  }

  .failed p {
    margin: 0 0 0.8rem;
    color: var(--red);
  }
</style>
