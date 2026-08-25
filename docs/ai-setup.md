# Setting up a model

The tool works without one. Detection is deterministic, and known problems have
written-down answers in the [runbook library](runbooks.md). A model adds two
things: it explains problems the library does not cover, and it correlates
findings that share a cause across different subsystems.

## Check what you have now

```bash
outlaw models
```

This shows which model would be used, why each option was or was not chosen,
how much video memory you have, and how many runbook entries are loaded. If a
local model server is already running, it will usually be found with no
configuration at all.

## The three tiers

The tool tries these in order, and stops at the first that works:

| Tier | What it is | On by default |
| --- | --- | --- |
| **Remote** | A model on another computer you control | No -- there is no address to guess |
| **Local** | A model on this computer | Yes |
| **Cloud** | A hosted model, over the internet | **No** |

That order is about where your data goes: a machine you own, then this machine,
then somebody else's only if you have deliberately turned it on.

**Pinning a tier stops fallback.** If you pin `local` and your local server is
switched off, the scan simply runs without a model. It will not quietly send
your diagnostics to a cloud provider instead. This is the intended behaviour,
not a limitation.

## Option 1: a model on this computer

Any server speaking the OpenAI API works. Two common ones:

**[Ollama](https://ollama.com)** -- install it, then pull a model:

```bash
ollama pull qwen3:8b
```

Ollama listens on `http://127.0.0.1:11434` and the tool checks there by
default. Nothing else to configure.

**[LM Studio](https://lmstudio.ai)** -- load a model, then start the local
server from the Developer tab. It listens on `http://127.0.0.1:1234`, which the
tool also checks by default.

Anything else that speaks the same API -- vLLM, llama.cpp's server, text-
generation-webui -- works too; point the tool at it with `urls` below.

### Which size of model

`outlaw models` recommends a size based on your video memory. Roughly:

| Video memory | Practical ceiling |
| --- | --- |
| Under 4 GB | A local model will be slow and limited -- consider [using another machine](remote-machine.md) |
| 4-8 GB | A 7-8B model at 4-bit quantisation |
| 8-16 GB | A 13-14B model at 4-bit |
| 16-24 GB | A 30B-class model at 4-bit |
| 24 GB+ | A 70B-class model at 4-bit; also a good machine to serve others from |

The tool does not load models itself -- it asks whatever server you are running
to use one. So this is advice for choosing, not something it enforces.

## Option 2: a model on another computer

See [Using another machine](remote-machine.md). This is the right answer when
the computer with the problem is not the computer with the graphics card.

## Option 3: a hosted model

Off by default, because this is the only tier that sends anything off hardware
you control. Turn it on deliberately:

```bash
outlaw set-key cloud     # paste your API key when prompted
```

The key is read from standard input rather than taken as an argument, so it
does not end up in your shell history or the process list. It is stored in your
operating system's credential store, never in a configuration file.

Then enable the tier in `config.toml`:

```toml
[ai.cloud]
enabled = true
provider = "anthropic"
model = "claude-opus-5"
```

For automation, `ORK_CLOUD_API_KEY` is checked before the credential store,
which is useful on servers with no desktop session.

## Configuration reference

`outlaw config` shows the file path and current values. Everything has a
working default; you only need to write the parts you want to change.

```toml
[ai]
# auto | remote | local | cloud | off
mode = "auto"
# How long to wait for an endpoint to answer when checking if it is up.
# This is a connection check, not a limit on how long the model may think.
reachability_timeout_ms = 2000

[ai.remote]
enabled = false
# [ai.remote.endpoint]
# url = "http://other-machine:1234/v1"
# model = ""          # empty means "whatever that server has loaded"

[ai.local]
enabled = true
urls = ["http://127.0.0.1:1234/v1", "http://127.0.0.1:11434/v1"]
model = ""            # empty means "whatever is loaded"

[ai.cloud]
enabled = false
provider = "anthropic"
model = "claude-opus-5"
```

Setting `mode = "off"` disables the model layer entirely. Runbook answers still
work.

## What the model actually receives

The structured findings the checks already produced: titles, severities, and
the specific evidence captured -- a log excerpt, an exit code, a driver
version. It has no access to your machine and cannot ask for more.

Three rules limit what it can do to your report:

- A model answer never overwrites a runbook answer for the same problem.
  Runbook entries are written and reviewed by people.
- Answers that do not correspond to a real finding are discarded rather than
  shown, so an invented problem cannot appear alongside real ones.
- The model cannot propose a command to run. Its suggestions are prose you
  read and decide on.

## Using it

```bash
outlaw scan --explain
```
