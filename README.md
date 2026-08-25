# Outlaw Repair Kit

A cross-platform tool that scans a computer for hardware and software problems,
explains what it found in plain language, and can attempt fixes with strong
safety rails.

Most of the detection work is deterministic: the tool wraps mature, established
diagnostic utilities rather than guessing. An AI layer sits on top of that to
correlate findings across sources, explain them, and reason through fixes for
problems that have no known deterministic answer -- it reads structured probe
output, never the live system.

> **Status: early.** The diagnostic core and the CLI exist and work. The model
> router, AI analysis, triage queue, fix layer, and desktop app are not built
> yet. See [Roadmap](#roadmap).

> **Built in collaboration with AI.** This project is developed by a human
> author working together with Claude (Anthropic). Design decisions are made
> jointly and reviewed by a human; a substantial portion of the code is
> AI-written. See [Built in collaboration with AI](#built-in-collaboration-with-ai).

## Install

Download the build for your system from the
[latest release](https://github.com/Sup095/outlaw-repair-kit/releases/latest),
unpack it, and run it. There is no installer and no runtime to install first --
it is a single binary.

**Windows**

```powershell
# after unpacking the .zip
.\outlaw.exe scan
```

**Linux**

```bash
tar xzf outlaw-*-x86_64-unknown-linux-gnu.tar.gz
cd outlaw-*-x86_64-unknown-linux-gnu
./outlaw scan
```

On Linux, install `systemd`'s `journalctl` (present on most distributions) for
the log check to see beyond the current boot. Anything missing is reported as a
skipped check with the reason, never as a silent gap.

## What works today

```bash
outlaw scan             # look for problems
outlaw scan --explain   # ...and explain what they mean
outlaw queue            # problems waiting to be worked through
outlaw fix              # show what would be done -- changes nothing
outlaw fix --apply      # allow changes, confirming each one
outlaw audit            # everything the tool has done
outlaw models           # which model would be used, and why
outlaw config           # where settings live and what they say
```

Full reference: [docs/commands.md](docs/commands.md).

Every command accepts `--json`, because nothing in the interface layer is
allowed to do something the core cannot do programmatically.

The Quick tier runs these checks:

| Check | What it catches |
| --- | --- |
| Disk space | Volumes running out of room, judged on both absolute free space and percentage so it misjudges neither a small SSD nor a large array |
| Memory pressure | Being short of memory, and the much worse state of being short of memory *and* swapping heavily |
| Running processes | Processes stuck, leaking memory, or piling up unreaped |
| Device and driver health | Devices the system cannot start, and drivers that no longer match the running kernel -- the cause behind "it broke after I updated" |
| Application launch check | Installed applications that no longer start, or that hang on startup |
| Recent system log errors | Crashes, driver faults, and hardware errors, with repeats grouped together |

Checks that cannot run -- wrong platform, missing tool, elevation not granted --
are reported as skipped with the reason. A scan never quietly covers less than
you think it did.

## Documentation

| | |
| --- | --- |
| [Getting started](docs/getting-started.md) | Install it and run your first scan |
| [Command reference](docs/commands.md) | What every command does |
| [Setting up a model](docs/ai-setup.md) | Local, another machine, or hosted |
| [Using another machine](docs/remote-machine.md) | Borrow a stronger computer's model |
| [Fixing problems safely](docs/fixing.md) | What it will and will not change |
| [Writing runbooks](docs/runbooks.md) | Teach it about a problem it does not know |
| [Troubleshooting](docs/troubleshooting.md) | When something is not working |
| [Architecture](docs/architecture.md) | How the pieces fit together |

## Explaining findings

Detection is deterministic. Explanation happens in two stages, in this order:

1. **The runbook library**, consulted first. Known problems have written-down
   answers with ranked fixes, least disruptive first. This needs no model, no
   network, and no money, and it produces the same answer twice.
2. **A model**, for what the library does not cover, and to correlate findings
   that share a cause across different subsystems.

If no model is available, the runbook answers still stand. The tool is fully
useful with the AI layer switched off, which is the property that keeps the AI
layer honest.

The model receives the structured findings the probes already produced. It has
no access to the machine and cannot ask for more.

### Which model

The router tries three tiers in order, and you can pin any one of them:

| Tier | What it is | Default |
| --- | --- | --- |
| Remote | A model on another machine you own, over your own network | Off until you set an address |
| Local | A model on this machine (LM Studio, Ollama, vLLM, anything OpenAI-compatible) | On |
| Cloud | A hosted model | **Off** |

That order is about where your data goes: a machine you own first, this machine
second, and a third party only if you have explicitly turned it on. Pinning a
tier means the router will never silently fall through to another one -- if you
pin `local` and your local server is down, the scan runs without a model rather
than sending your diagnostics to a cloud provider.

`outlaw models` shows exactly which tier was chosen and why each other one was
not.

## Fixing problems

Problems needing investigation go on a triage queue rather than blocking the
scan. Afterwards the queue is worked one item at a time, worst first: snapshot,
apply one change, test whether it worked, roll back if it did not, then try the
next candidate.

`outlaw fix` is a **dry run**. With `--apply` it still asks before every
individual change.

What it will do is deliberately a short list -- remove a stale lock file after
backing it up, restart a service, run read-only inspection commands. Installing
drivers, changing packages, and anything else arrives as an instruction for you
to carry out. Fixes are a closed set of typed operations; there is no "run this
command" operation at all, for runbooks or for a model, because that would turn
every safety rule here into advice rather than a guarantee.

Two rules that may surprise you: a change is only applied if its result can be
*tested*, and "I could not tell whether that worked" is treated as failure and
rolled back.

[Fixing problems safely](docs/fixing.md) has the details.

## Design commitments

These are load-bearing, not aspirational:

- **OS-agnostic by construction.** Everything OS-specific lives behind a
  `Platform` trait, with Windows and Linux implementations from day one. Adding
  macOS is a new implementation, not a rewrite.
- **Deterministic checks are preferred over AI reasoning.** The AI layer
  correlates and explains; it does not replace a check that can be made
  precisely.
- **No time limits.** No scan tier and no individual check is given a deadline.
  Long-running work is supervised by a liveness check -- is this process still
  doing anything? -- and is always manually cancellable, but it is never cut
  off for taking too long.
- **Skipped is visible.** A missing tool or a denied elevation produces a
  reported skip with a reason, never a silent gap in coverage.
- **One broken check does not lose the scan.** A probe that fails is recorded
  and the scan continues.
- **Least privilege.** The core runs unprivileged; anything needing
  administrator or root rights goes through a separate elevation step.

## Building

Requires a stable Rust toolchain. On Windows you also need the MSVC build
tools; on Linux, a working C toolchain.

```bash
cargo build --release
```

The binary lands at `target/release/outlaw`.

```bash
cargo test
```

## Layout

| Path | What it is |
| --- | --- |
| `crates/ork-core` | Diagnostic core: platform layer, probes, scan orchestration |
| `crates/ork-cli` | The `outlaw` command-line front-end |
| `docs/` | Architecture and design notes |

## Roadmap

1. **Diagnostic core, Quick tier** -- done. See the table above.
2. **Model router and AI analysis** -- done. See
   [Explaining findings](#explaining-findings).
3. **Triage queue and fix-attempt loop** -- done. See
   [Fixing problems safely](docs/fixing.md).
4. **Full and Deep tiers, plus a background watcher.**
5. **Desktop application** -- everything configurable from the interface, with
   no file editing required.

## Privacy

The tool collects information about the machine it runs on.

Nothing leaves your machine unless you ask for an explanation, and even then
where it goes is your choice. The cloud tier is off by default and has to be
turned on deliberately; the local and remote tiers keep everything on hardware
you own. What a model receives is the structured findings -- titles,
severities, and captured evidence -- not raw access to your system.

API keys are stored in the operating system's credential store (Credential
Manager on Windows, the desktop secret service on Linux), never in a
configuration file. `outlaw config` will show you what is stored without
showing you the values.

## Built in collaboration with AI

This tool is written by a human author working together with Claude, Anthropic's
AI assistant. That collaboration is stated plainly here because you deserve to
know what you are running:

- Architecture and design decisions are made jointly and reviewed by a human
  before implementation.
- A substantial portion of the source code is AI-written.
- Every change is built, tested, and exercised on real hardware before it is
  committed. Nothing is merged on the strength of "it looks right".

If that matters to how you evaluate a tool that inspects and repairs your
system -- and it reasonably might -- the full history is in the commit log, and
the code is deliberately commented to explain *why* rather than *what*.

## License

MIT. See [LICENSE](LICENSE).
