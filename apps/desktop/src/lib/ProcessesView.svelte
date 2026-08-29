<script lang="ts">
  /**
   * What is running, and what a sweep would do to each.
   *
   * Stages two and three of `docs/proposals/process-control.md`, in the
   * window: the list, and the button that acts on it.
   *
   * The judgement is not made here. It comes from the same `Survey` the
   * terminal prints, so the two cannot come to different conclusions about
   * what "held back" means. Neither is the acting: the button sends a list and
   * the back end judges every entry on it again, against a fresh look at the
   * machine, so what this screen showed a moment ago cannot become permission
   * to stop something that has since become protected.
   *
   * What is shown before the button is the whole list, grouped, rather than a
   * count. A dialog that asks "stop 16 processes?" is asking somebody to agree
   * to a number they have no way of checking.
   */
  import {
    api,
    type ProcessProgram,
    type ProcessSurvey,
    type StopReport,
  } from "./api";
  import { formatBytes } from "./bytes";
  import { compactDuration } from "./time";

  let survey = $state<ProcessSurvey | null>(null);
  let error = $state<string | null>(null);
  let loading = $state(false);
  let showAll = $state(false);
  let showAllPrograms = $state(false);
  /** The program whose pin is being written, so its control can say so. */
  let pinning = $state<string | null>(null);
  /** Whether the list is being shown for agreement. Nothing has been sent. */
  let confirming = $state(false);
  let stopping = $state(false);
  /**
   * What happened, kept on screen until it is dismissed.
   *
   * It outlives the refresh that follows deliberately. The refresh is what
   * makes the list above true again, and it would otherwise wipe the only
   * account of what was just done to the machine off the screen in the same
   * instant.
   */
  let report = $state<StopReport | null>(null);

  const ENOUGH = 20;

  const candidates = $derived(
    survey ? survey.rows.filter((row) => row.standing.standing === "candidate") : [],
  );
  const held = $derived(
    survey ? survey.rows.filter((row) => row.standing.standing === "held-back") : [],
  );
  const shown = $derived(showAll ? candidates : candidates.slice(0, ENOUGH));
  const programs = $derived(survey ? survey.programs : []);
  const programsShown = $derived(
    showAllPrograms ? programs : programs.slice(0, ENOUGH),
  );
  /**
   * Only said when it is true of something on screen. A caveat printed under a
   * list it does not apply to is a caveat people learn to skip.
   */
  const anyPartly = $derived(
    programsShown.some((program) => program.sweep.how === "part-of-it"),
  );
  /** What the sweep would touch, grouped the way somebody reads it. */
  const offeredPrograms = $derived(
    programs.filter((program) => program.offered > 0),
  );
  const stoppedAttempts = $derived(
    report ? report.attempts.filter((attempt) => attempt.changed_anything) : [],
  );
  const leftAlone = $derived(
    report ? report.attempts.filter((attempt) => !attempt.changed_anything) : [],
  );


  /**
   * Put a program on the leave-alone list, or take it off.
   *
   * The survey is read again afterwards rather than the row being adjusted in
   * place. Pinning changes what the classifier decides about every process of
   * that name, and guessing the new answer here would be a second opinion --
   * the screen would show what it expected rather than what the tool decided.
   */
  async function togglePin(program: ProcessProgram) {
    pinning = program.name;
    try {
      await api.processPin(program.name, !program.pinned);
      await load();
      error = null;
    } catch (problem) {
      error = String(problem);
    } finally {
      pinning = null;
    }
  }

  /**
   * Stop everything the sweep offers, having shown it and been told yes.
   *
   * The list is built from what is on screen, and is not what decides. Between
   * the panel opening and the button being pressed somebody can alt-tab, and
   * the program they switched to is a program they are looking at -- so every
   * entry is judged again in the back end, one at a time, against a fresh look
   * at the machine.
   */
  async function stopThem() {
    stopping = true;
    try {
      report = await api.processStop(
        candidates.map((row) => ({ pid: row.pid, name: row.name })),
      );
      confirming = false;
      error = null;
      await load();
    } catch (problem) {
      error = String(problem);
    } finally {
      stopping = false;
    }
  }

  async function load() {
    loading = true;
    try {
      survey = await api.processSurvey();
      error = null;
    } catch (problem) {
      error = String(problem);
    } finally {
      loading = false;
    }
  }

  load();
</script>

<div class="head">
  <h2>Processes</h2>
  <button onclick={load} disabled={loading}>{loading ? "Looking" : "Refresh"}</button>
</div>

<p class="dim intro">
  What is running, and what a sweep would do to each. Only what runs as you is
  ever offered; anything the system owns, anything you are looking at, and
  anything you have said to leave alone is held back with the reason shown.
  <strong>Nothing is put back for you.</strong> There is no snapshot of a
  running program, so what was stopped is written down and shown afterwards,
  and starting anything again is yours to do.
</p>

{#if error}
  <div class="panel bad">{error}</div>
{:else if !survey}
  <div class="panel dim">Looking at what is running…</div>
{:else}
  <div class="panel counts">
    <div><strong>{survey.running}</strong> <span class="dim">running</span></div>
    <div><strong>{candidates.length}</strong> <span class="dim">could be stopped</span></div>
    <div><strong>{held.length}</strong> <span class="dim">held back</span></div>
    <div>
      <strong>{survey.rows.length - candidates.length - held.length}</strong>
      <span class="dim">never touched</span>
    </div>
  </div>

  {#if survey.in_front_unchecked}
    <!-- Called out, and not quietly. Every other unknown in this tool makes it
         more careful; this one makes it less, because not knowing what is in
         front of you holds nothing back. A screen that hid it would be showing
         a list that looked complete and was not. -->
    <div class="panel unchecked">
      <h3>One rule did not run</h3>
      <p>
        Nothing with a window in front of you is offered for stopping, and on
        this machine that could not be checked: {survey.in_front_unchecked}.
        Everything else below still applies. It means the list may include what
        you are looking at.
      </p>
    </div>
  {/if}

  <section>
    <div class="section-head">
      <h3>By program</h3>
      <span class="dim">several processes of one name are one program to you</span>
    </div>
    {#if programs.length === 0}
      <div class="panel dim">Nothing.</div>
    {:else}
      <div class="panel rows">
        {#each programsShown as program (program.name)}
          <div class="row">
            <span class="name" title={program.name}>{program.name}</span>
            <span class="mem">{formatBytes(program.memory_held)}</span>
            <span class="dim when">
              {program.processes}
              {program.processes === 1 ? "process" : "processes"}
            </span>
            <span
              class="dim offered"
              class:partly={program.sweep.how === "part-of-it"}
              title={program.sweep_says}
            >
              {program.sweep_briefly}
            </span>
            <!-- Only where it would mean something. A program nothing would
                 ever touch is already left alone, and a control that changed
                 a setting with no effect would be teaching the wrong thing
                 about what the setting does. -->
            {#if program.protected < program.processes}
              <button
                class="pin"
                class:on={program.pinned}
                disabled={pinning === program.name}
                onclick={() => togglePin(program)}
                title={program.pinned
                  ? `Stop leaving ${program.name} alone`
                  : `Never offer ${program.name} for stopping`}
              >
                {pinning === program.name ? "…" : program.pinned ? "Left alone" : "Leave alone"}
              </button>
            {:else}
              <span class="pin placeholder"></span>
            {/if}
          </div>
        {/each}
        {#if programs.length > programsShown.length}
          <button class="more" onclick={() => (showAllPrograms = true)}>
            Show the other {programs.length - programsShown.length}
          </button>
        {/if}
      </div>
      {#if anyPartly}
        <p class="dim note">
          Where fewer are offered than are running, stopping the offered ones
          leaves the program running with fewer processes. It does not close it.
        </p>
      {/if}
    {/if}
  </section>

  {#if report}
    <!-- What happened, including everything that did not. The ones left alone
         are shown as prominently as the ones stopped: a report that listed
         only its successes would leave somebody believing a program had gone
         when it is still running. -->
    <div class="panel report">
      <div class="section-head">
        <h3>
          Stopped {report.stopped}
          {report.stopped === 1 ? "program" : "programs"}
        </h3>
        <span class="dim">
          holding {formatBytes(report.memory_held_by_stopped)} when last seen
        </span>
      </div>
      {#if stoppedAttempts.length === 0}
        <p class="dim">Nothing was stopped.</p>
      {:else}
        <div class="rows">
          {#each stoppedAttempts as attempt (attempt.pid)}
            <div class="row">
              <span class="name" title={attempt.name}>{attempt.name}</span>
              <span class="mem">{formatBytes(attempt.memory_held_bytes)}</span>
              <span class="dim when">{attempt.says}</span>
            </div>
          {/each}
        </div>
      {/if}
      {#if leftAlone.length > 0}
        <h4>Left alone</h4>
        <div class="rows">
          {#each leftAlone as attempt (attempt.pid)}
            <div class="row">
              <span class="name" title={attempt.name}>{attempt.name}</span>
              <span class="dim when wide">{attempt.says}</span>
            </div>
          {/each}
        </div>
      {/if}
      <p class="dim note">
        "Holding" is what they had, not what came back to the machine. Every one
        of these is in the audit log, including the ones left alone, so what
        happened here is answerable tomorrow as well as now.
      </p>
      <button onclick={() => (report = null)}>Done</button>
    </div>
  {/if}

  {#if confirming}
    <!-- The whole list, before anything is sent. -->
    <div class="panel confirm">
      <h3>This would stop</h3>
      <div class="rows">
        {#each offeredPrograms as program (program.name)}
          <div class="row">
            <span class="name" title={program.name}>{program.name}</span>
            <span class="mem">{formatBytes(program.memory_held)}</span>
            <span class="dim when" title={program.sweep_says}>
              {program.sweep_briefly}
            </span>
          </div>
        {/each}
      </div>
      <p class="dim note">
        Nothing here is put back for you. What was stopped is written down and
        shown afterwards, and starting anything again is yours to do — so a
        program you want left alone is worth leaving alone above, first.
      </p>
      <p class="dim note">
        Programs are ended rather than asked to close. Anything that might be
        holding unsaved work is held back from this list for that reason, but
        it is worth saving what is open before agreeing.
      </p>
      <div class="choices">
        <button class="act" onclick={stopThem} disabled={stopping}>
          {stopping
            ? "Stopping…"
            : `Stop ${candidates.length} ${candidates.length === 1 ? "process" : "processes"}`}
        </button>
        <button onclick={() => (confirming = false)} disabled={stopping}>
          Cancel
        </button>
      </div>
    </div>
  {/if}

  <section>
    <div class="section-head">
      <h3>Could be stopped</h3>
      <span class="dim">
        holding {formatBytes(survey.memory_held_by_candidates)} between them
      </span>
      {#if candidates.length > 0 && !confirming}
        <button class="act" onclick={() => (confirming = true)}>
          Stop these…
        </button>
      {/if}
    </div>
    {#if candidates.length === 0}
      <div class="panel dim">Nothing.</div>
    {:else}
      <div class="panel rows">
        {#each shown as row (row.pid)}
          <div class="row">
            <span class="name" title={row.name}>{row.name}</span>
            <span class="mem">{formatBytes(row.memory_bytes)}</span>
            <span class="dim when">running {compactDuration(row.run_time_secs)}</span>
          </div>
        {/each}
        {#if candidates.length > shown.length}
          <button class="more" onclick={() => (showAll = true)}>
            Show the other {candidates.length - shown.length}
          </button>
        {/if}
      </div>
      <p class="dim note">
        <strong>Holding</strong> is what they have now, not what stopping them
        would give back. The second number is always smaller, because memory
        shared between programs is counted against every one of them, and it is
        only knowable by measuring afterwards.
      </p>
    {/if}
  </section>

  <section>
    <h3>Held back, and why</h3>
    <p class="dim note">
      Not offered by default. You could still choose them one at a time, once
      there is anything to choose them for.
    </p>
    {#if survey.why_held_back.length === 0}
      <div class="panel dim">Nothing.</div>
    {:else}
      <div class="panel reasons">
        {#each survey.why_held_back as reason (reason.reason)}
          <div class="reason">
            <span>{reason.reason}</span>
            <span class="dim count">
              {reason.count} {reason.count === 1 ? "process" : "processes"}
            </span>
          </div>
        {/each}
      </div>
    {/if}
  </section>

  <section>
    <h3>Never touched</h3>
    <p class="dim note">
      There is no setting that changes this. A list of what a tool considers
      untouchable is worthless if nobody can read it, so here it is.
    </p>
    <div class="panel reasons">
      {#each survey.why_protected as reason (reason.reason)}
        <div class="reason">
          <span>{reason.reason}</span>
          <span class="dim count">
            {reason.count} {reason.count === 1 ? "process" : "processes"}
          </span>
        </div>
      {/each}
    </div>
  </section>
{/if}

<style>
  .head { display: flex; align-items: center; gap: 1rem; margin-bottom: 0.5rem; }
  .head button { margin-left: auto; }
  .intro { max-width: 68ch; margin: 0 0 1rem; font-size: 12.5px; }
  .note { max-width: 68ch; margin: 0.5rem 0 0; font-size: 12px; }

  .counts {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem 2rem;
    font-size: 13px;
    margin-bottom: 1rem;
  }
  .counts strong { color: var(--cyan); font-size: 15px; }

  .unchecked { border-color: var(--amber); margin-bottom: 1rem; }
  .unchecked h3 { margin: 0 0 0.35rem; color: var(--amber); }
  .unchecked p { margin: 0; font-size: 12.5px; max-width: 68ch; }

  section { margin-bottom: 1.5rem; }
  section h3 { margin: 0 0 0.5rem; }
  .section-head {
    display: flex;
    align-items: baseline;
    gap: 0.9rem;
    flex-wrap: wrap;
    margin-bottom: 0.5rem;
  }
  .section-head h3 { margin: 0; }

  .rows { display: grid; gap: 0.2rem; font-size: 12.5px; }
  .row { display: flex; gap: 0.9rem; align-items: baseline; }
  .name {
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .mem { flex: none; min-width: 5.5rem; text-align: right; color: var(--cyan); }
  .when { flex: none; min-width: 8rem; }
  .offered { flex: none; min-width: 9rem; }
  .pin { flex: none; min-width: 6.5rem; font-size: 11px; padding: 0.15rem 0.45rem; }
  .pin.on { border-color: var(--cyan); color: var(--cyan); }
  /* Keeps the column aligned where no control is drawn. */
  .pin.placeholder { border: none; background: none; }
  /* The one case worth a colour: the program is still there afterwards. */
  .offered.partly { color: var(--amber); }
  .more { margin-top: 0.5rem; justify-self: start; font-size: 12px; padding: 0.3rem 0.7rem; }

  .reasons { display: grid; gap: 0.25rem; font-size: 12.5px; }
  .reason { display: flex; gap: 1rem; align-items: baseline; }
  .reason span:first-child { flex: 1 1 auto; min-width: 0; }
  .count { flex: none; }

  /* The list before it is agreed to, and the account of it afterwards. Both
     are marked out from the reading below them: they are the two moments on
     this screen where something is about to change or just has. */
  .confirm { border-color: var(--amber); margin-bottom: 1.5rem; }
  .confirm h3 { margin: 0 0 0.5rem; color: var(--amber); }
  .report { border-color: var(--cyan); margin-bottom: 1.5rem; }
  .report h4 { margin: 0.9rem 0 0.4rem; font-size: 12.5px; }
  .report .section-head h3 { color: var(--cyan); }
  .choices { display: flex; gap: 0.6rem; margin-top: 0.9rem; }
  .act { border-color: var(--amber); color: var(--amber); }
  /* Where the reason is the whole of what is being said, it gets the room. */
  .when.wide { flex: 1 1 auto; min-width: 0; }

  .bad { border-color: var(--red); color: var(--red); }
</style>
