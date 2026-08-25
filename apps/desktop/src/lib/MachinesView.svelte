<script lang="ts">
  import { onMount } from "svelte";
  import { api, onLinkEvent, type Discovered, type LinkEvent } from "./api";

  let status = $state<any | null>(null);
  let found = $state<Discovered[]>([]);
  let searching = $state(false);
  let code = $state("");
  let address = $state("");
  let error = $state<string | null>(null);
  let notice = $state<string | null>(null);
  let viewing = $state<any | null>(null);
  let activity = $state<string[]>([]);

  onMount(() => {
    let unlisten: (() => void) | undefined;
    (async () => {
      unlisten = await onLinkEvent((event: LinkEvent) => {
        const line =
          event.event === "linked"
            ? `${event.name} linked — it can now ask this machine to run its model`
            : event.event === "wrong-code"
              ? event.attempts_left === 0
                ? "Too many wrong codes — pairing closed. Show a new code to try again."
                : `Wrong pairing code — ${event.attempts_left} attempt(s) left`
              : `Running the model for ${event.name}`;
        activity = [...activity, line].slice(-5);
        if (event.event === "linked") load();
      });
    })();
    return () => unlisten?.();
  });

  async function load() {
    try {
      status = await api.linkStatus();
      error = null;
    } catch (problem) {
      error = String(problem);
    }
  }

  async function startHosting() {
    try {
      await api.linkHostStart();
      await load();
      error = null;
    } catch (problem) {
      error = String(problem);
    }
  }

  async function reopen() {
    // A code is good for one machine. Linking a second should not mean
    // stopping and starting the whole thing.
    try {
      await api.linkPairReopen();
      await load();
      error = null;
    } catch (problem) {
      error = String(problem);
    }
  }

  async function stopHosting() {
    await api.linkHostStop();
    await load();
  }

  async function search() {
    searching = true;
    try {
      found = await api.linkFind();
      // One machine showing a code is the common case, so fill it in rather
      // than making them copy an address across the screen.
      const open = found.filter((machine) => machine.pairing_open);
      if (open.length === 1) address = open[0].address;
      error = null;
    } catch (problem) {
      error = String(problem);
    } finally {
      searching = false;
    }
  }

  async function join() {
    try {
      const peer = await api.linkJoin(code, address);
      notice = `Linked to ${peer.name}. Its model will be used automatically.`;
      code = "";
      error = null;
      await load();
    } catch (problem) {
      error = String(problem);
    }
  }

  async function remove(name: string) {
    try {
      await api.linkRemove(name);
      notice = `Unlinked ${name}. Its token has been removed from the credential store.`;
      await load();
    } catch (problem) {
      error = String(problem);
    }
  }

  async function view(name: string) {
    try {
      viewing = await api.linkView(name);
      error = null;
    } catch (problem) {
      error = String(problem);
      viewing = null;
    }
  }

  load();
</script>

<div class="head">
  <h2>Machines</h2>
  <button onclick={load}>Refresh</button>
</div>

<p class="dim intro">
  Pair two computers so one can lend the other a model. A linked machine can be asked
  to think about a problem and to say what it found — and nothing else. No command in a
  link can change the machine at the other end.
</p>

{#if status && status.credential_store === false}
  <div class="panel bad">
    This machine has no credential store running, so there is nowhere safe to keep an
    access token — linking will not work until there is one. On Linux, start a secret
    service such as GNOME Keyring or KWallet.
  </div>
{/if}
{#if error}<div class="panel bad">{error}</div>{/if}
{#if notice}<div class="panel good">{notice}</div>{/if}

{#if status}
  <section class="panel">
    <h3>Lend this machine's model</h3>
    {#if status.hosting}
      <p class="code">{status.hosting.pairing_code ?? ""}</p>
      <p class="dim">
        Listening on port {status.hosting.port}.
        {status.hosting.pairing_open ? "A pairing code is showing." : "The code has been used or has expired."}
      </p>
      <div class="row">
        {#if !status.hosting.pairing_open}
          <button class="primary" onclick={reopen}>Show a new code</button>
        {/if}
        <button class="danger" onclick={stopHosting}>Stop lending</button>
      </div>
      {#if activity.length}
        <div class="activity">
          {#each activity as line, index (line + index)}<div>{line}</div>{/each}
        </div>
      {/if}
    {:else}
      <p class="dim">
        Start this, then type the code it shows on the other machine. It lasts ten
        minutes, works once, and stops accepting guesses after five wrong tries.
      </p>
      <button class="primary" onclick={startHosting}>Show a pairing code</button>
    {/if}
  </section>

  <section class="panel">
    <h3>Borrow another machine's model</h3>
    <div class="row">
      <button onclick={search} disabled={searching}>{searching ? "Looking…" : "Find on this network"}</button>
      <span class="dim">
        {#if found.length}{found.length} machine{found.length === 1 ? "" : "s"} answered{:else}Broadcasts do not leave this network — for one somewhere else, type its address{/if}
      </span>
    </div>
    {#each found as machine (machine.machine_id)}
      <button class="found" onclick={() => (address = machine.address)}>
        {machine.name} <span class="dim">{machine.address} · {machine.pairing_open ? "showing a code" : "not pairing"}</span>
      </button>
    {/each}
    <label><span class="dim">Address</span><input bind:value={address} placeholder="192.168.1.20" /></label>
    <label><span class="dim">Pairing code</span><input bind:value={code} placeholder="XXXX-XXXX-XXXX" /></label>
    <div><button class="primary" onclick={join} disabled={!code.trim() || !address.trim()}>Link</button></div>
  </section>

  <section class="panel">
    <h3>Linked machines</h3>
    {#if status.peers.length === 0}
      <p class="dim">Nothing is linked yet.</p>
    {/if}
    {#each status.peers as peer (peer.id + peer.role)}
      <div class="peer">
        <div>
          <strong>{peer.name}</strong>
          <span class="dim">{peer.role === "lender" ? "lends to us" : "borrows from us"}</span>
          <div class="dim small">{peer.address || "connects to us"}</div>
        </div>
        <div class="peer-actions">
          {#if peer.role === "lender"}
            <button onclick={() => api.linkCheck(peer.name).then(() => (notice = `${peer.name} is answering.`)).catch((problem) => (error = String(problem)))}>Check</button>
            <button onclick={() => view(peer.name)}>What is wrong there</button>
          {/if}
          <button class="danger" onclick={() => remove(peer.name)}>Unlink</button>
        </div>
      </div>
    {/each}
    <p class="dim small">
      This machine is <code>{status.machine_name}</code>. Unlinking removes this side's token;
      the other machine keeps its own record until it unlinks too.
    </p>
  </section>

  {#if viewing}
    <section class="panel">
      <h3>{viewing.host_name}</h3>
      <p class="dim">{viewing.host?.os_name ?? ""} · {viewing.host?.cpu_brand ?? ""}</p>
      {#if viewing.queue_error}
        <p class="bad-text">Could not read what it found: {viewing.queue_error}</p>
      {:else if (viewing.waiting ?? []).length === 0}
        <p class="dim">Nothing is waiting on that machine.</p>
      {:else}
        {#each viewing.waiting as item, index (index)}
          <div class="waiting">
            <span class="sev {item.severity}">{item.severity}</span>
            <div><strong>{item.title}</strong><div class="dim small">{item.detail}</div></div>
          </div>
        {/each}
        <p class="dim small">Fixing is done at that machine's own keyboard.</p>
      {/if}
    </section>
  {/if}
{/if}

<style>
  .head { display: flex; align-items: center; gap: 1rem; margin-bottom: 0.5rem; }
  .head button { margin-left: auto; }
  .intro { max-width: 66ch; margin: 0 0 1rem; font-size: 12.5px; }
  section { margin-bottom: 1rem; display: grid; gap: 0.8rem; }
  section h3 { font-size: 12.5px; color: var(--amber); }
  section p { margin: 0; font-size: 12.5px; max-width: 66ch; }
  .code {
    font-size: 2rem;
    letter-spacing: 0.25em;
    color: var(--amber);
    text-shadow: 0 0 18px rgba(255, 176, 0, 0.4);
  }
  .activity {
    border-left: 2px solid var(--line);
    padding-left: 0.8rem;
    display: grid;
    gap: 0.2rem;
    font-size: 12px;
    color: var(--cyan);
  }
  .row { display: flex; align-items: center; gap: 0.8rem; font-size: 12.5px; }
  .found {
    text-align: left;
    text-transform: none;
    letter-spacing: normal;
    font-size: 12.5px;
  }
  label { display: grid; gap: 0.25rem; font-size: 12.5px; max-width: 30rem; }
  .peer { display: flex; align-items: center; gap: 1rem; border-top: 1px solid var(--line); padding-top: 0.6rem; }
  .peer-actions { margin-left: auto; display: flex; gap: 0.4rem; }
  .small { font-size: 11.5px; }
  .waiting { display: flex; gap: 0.8rem; align-items: start; }
  .bad { border-color: var(--red); color: var(--red); margin-bottom: 1rem; }
  .bad-text { color: var(--red); }
  .good { border-color: var(--green); color: var(--green); margin-bottom: 1rem; }
  .sev { text-transform: uppercase; font-size: 10.5px; letter-spacing: 0.14em; padding: 0.1rem 0.45rem; border: 1px solid currentColor; }
  .sev.critical { color: #fff; background: var(--red); border-color: var(--red); }
  .sev.high { color: var(--red); }
  .sev.medium { color: var(--yellow); }
  .sev.low { color: var(--cyan); }
  .sev.info { color: var(--text-dim); }
</style>
