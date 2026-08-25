<script lang="ts">
  import { api } from "./api";

  // The point of this screen: nobody should have to hand-edit a file to point
  // the tool at their own machine, their own model, or their own key.
  let config = $state<any | null>(null);
  let path = $state("");
  let status = $state<{ cloud: boolean; remote: boolean }>({ cloud: false, remote: false });
  let saved = $state<string | null>(null);
  let error = $state<string | null>(null);
  let cloudKey = $state("");
  let remoteToken = $state("");

  async function load() {
    try {
      const loaded = await api.settingsLoad();
      config = loaded.config;
      path = loaded.path;
      status = await api.secretStatus();
      error = null;
    } catch (problem) {
      error = String(problem);
    }
  }

  async function save() {
    if (!config) return;
    try {
      const where = await api.settingsSave(config);
      saved = `Saved to ${where}`;
      error = null;
      setTimeout(() => (saved = null), 4000);
    } catch (problem) {
      error = String(problem);
    }
  }

  async function storeSecret(which: "cloud" | "remote", value: string) {
    try {
      await api.secretSet(which, value);
      // Cleared immediately: the value has no reason to stay in the window.
      if (which === "cloud") cloudKey = "";
      else remoteToken = "";
      status = await api.secretStatus();
      error = null;
    } catch (problem) {
      error = String(problem);
    }
  }

  async function clearSecret(which: "cloud" | "remote") {
    try {
      await api.secretClear(which);
      status = await api.secretStatus();
    } catch (problem) {
      error = String(problem);
    }
  }

  function ensureEndpoint() {
    if (config && !config.ai.remote.endpoint) {
      config.ai.remote.endpoint = { url: "http://127.0.0.1:1234/v1", model: "" };
    }
  }

  load();
</script>

<h2>Settings</h2>
<p class="dim intro">Stored in <code>{path}</code>. Keys never go in that file — they go to the operating system's own credential store.</p>

{#if error}<div class="panel bad">{error}</div>{/if}
{#if saved}<div class="panel good">{saved}</div>{/if}

{#if config}
  <section class="panel">
    <h3>Which model to use</h3>
    <label>
      <span class="dim">Routing</span>
      <select bind:value={config.ai.mode}>
        <option value="auto">Automatic — remote, then local, then cloud</option>
        <option value="remote">Only the remote endpoint</option>
        <option value="local">Only a local model</option>
        <option value="cloud">Only the cloud provider</option>
        <option value="off">No model at all — checks and runbooks only</option>
      </select>
    </label>
    <label>
      <span class="dim">How long to wait for an endpoint to answer (ms)</span>
      <input type="number" min="100" max="60000" bind:value={config.ai.reachability_timeout_ms} />
      <small class="dim">A connection check, not a limit on how long a model may think.</small>
    </label>
  </section>

  <section class="panel">
    <h3>Another machine</h3>
    <p class="dim">
      Any reachable OpenAI-compatible endpoint: a machine on your LAN, over a VPN, or over
      Tailscale. It only needs an address.
    </p>
    <label class="check">
      <input type="checkbox" bind:checked={config.ai.remote.enabled} onchange={ensureEndpoint} />
      <span>Use a model on another machine</span>
    </label>
    {#if config.ai.remote.enabled}
      {#if !config.ai.remote.endpoint}{ensureEndpoint()}{/if}
      {#if config.ai.remote.endpoint}
        <label><span class="dim">Address</span><input bind:value={config.ai.remote.endpoint.url} placeholder="http://100.x.y.z:1234/v1" /></label>
        <label><span class="dim">Model name (blank picks the first it offers)</span><input bind:value={config.ai.remote.endpoint.model} /></label>
      {/if}
      <div class="secret">
        <span class="dim">Access token: {status.remote ? "stored" : "not set"}</span>
        <input type="password" bind:value={remoteToken} placeholder="paste a token, if that endpoint needs one" />
        <button onclick={() => storeSecret("remote", remoteToken)} disabled={!remoteToken.trim()}>Save token</button>
        <button class="danger" onclick={() => clearSecret("remote")} disabled={!status.remote}>Remove</button>
      </div>
    {/if}
  </section>

  <section class="panel">
    <h3>A model on this machine</h3>
    <label class="check">
      <input type="checkbox" bind:checked={config.ai.local.enabled} />
      <span>Use a local model when one is running</span>
    </label>
    <label>
      <span class="dim">Addresses to try, one per line</span>
      <textarea
        rows="3"
        value={config.ai.local.urls.join("\n")}
        onchange={(event) => (config.ai.local.urls = (event.currentTarget as HTMLTextAreaElement).value.split("\n").map((line) => line.trim()).filter(Boolean))}
      ></textarea>
      <small class="dim">LM Studio and Ollama defaults are filled in already.</small>
    </label>
    <label><span class="dim">Model name (blank picks the first it offers)</span><input bind:value={config.ai.local.model} /></label>
  </section>

  <section class="panel">
    <h3>Cloud provider</h3>
    <label class="check">
      <input type="checkbox" bind:checked={config.ai.cloud.enabled} />
      <span>Fall back to a cloud model</span>
    </label>
    <label><span class="dim">Provider</span><input bind:value={config.ai.cloud.provider} /></label>
    <label><span class="dim">Model</span><input bind:value={config.ai.cloud.model} /></label>
    <div class="secret">
      <span class="dim">API key: {status.cloud ? "stored" : "not set"}</span>
      <input type="password" bind:value={cloudKey} placeholder="paste your API key" />
      <button onclick={() => storeSecret("cloud", cloudKey)} disabled={!cloudKey.trim()}>Save key</button>
      <button class="danger" onclick={() => clearSecret("cloud")} disabled={!status.cloud}>Remove</button>
    </div>
  </section>

  <div class="actions">
    <button class="primary" onclick={save}>Save settings</button>
    <button onclick={load}>Discard changes</button>
  </div>
{/if}

<style>
  .intro { margin: 0 0 1rem; font-size: 12.5px; }
  section { margin-bottom: 1rem; display: grid; gap: 0.8rem; }
  section h3 { font-size: 12.5px; color: var(--amber); }
  section p { margin: 0; font-size: 12.5px; max-width: 62ch; }
  label { display: grid; gap: 0.25rem; font-size: 12.5px; max-width: 40rem; }
  label.check { display: flex; align-items: center; gap: 0.6rem; }
  label.check input { width: auto; }
  small { font-size: 11.5px; }
  .secret { display: flex; align-items: center; gap: 0.6rem; flex-wrap: wrap; font-size: 12.5px; }
  .secret input { max-width: 22rem; }
  .actions { display: flex; gap: 0.6rem; margin-top: 0.5rem; }
  .bad { border-color: var(--red); color: var(--red); margin-bottom: 1rem; }
  .good { border-color: var(--green); color: var(--green); margin-bottom: 1rem; }
</style>
