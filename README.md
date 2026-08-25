# Outlaw Repair Kit

**by Outlaw Systems**

A cross-platform tool that scans a computer for hardware and software problems,
explains what it found in plain language, and can attempt fixes with strong
safety rails.

Most of the detection work is deterministic: the tool wraps mature, established
diagnostic utilities rather than guessing. An AI layer sits on top of that to
correlate findings across sources, explain them, and reason through fixes for
problems that have no known deterministic answer -- it reads structured probe
output, never the live system.

> **Status: usable, still young.** The diagnostic core, the command line, the
> desktop app, the model router, the AI analysis layer, the triage queue, the
> fix engine, and linking two machines all exist and work. The Quick scan tier
> is complete; the Full and Deep tiers and the background watcher are not built
> yet.
>
> The honest caveat: the fix engine can carry out only two kinds of change --
> restarting a service and removing a stale file -- and it will not apply even
> those unless it can re-test the result afterwards. Two kinds of problem can
> currently be re-tested: a stale lock file, and Steam failing to start. Every
> other problem is explained rather than fixed, and `outlaw fix` tells you how
> many of yours fall on each side before it does anything. See
> [Fixing problems safely](docs/fixing.md) and the [Roadmap](#roadmap).

> **Built in collaboration with AI.** This project is developed by a human
> author working together with Claude (Anthropic). Design decisions are made
> jointly and reviewed by a human; a substantial portion of the code is
> AI-written. See [Built in collaboration with AI](#built-in-collaboration-with-ai).

## Install

**Windows**, in PowerShell:

```powershell
irm https://raw.githubusercontent.com/Sup095/outlaw-repair-kit/main/install/install.ps1 | iex
```

**Linux**:

```sh
curl -fsSL https://raw.githubusercontent.com/Sup095/outlaw-repair-kit/main/install/install.sh | sh
```

The installer checks what it downloaded against the checksum published with the
release, puts `outlaw` on your PATH without needing administrator rights, and
asks -- rather than assumes -- whether you also want a model running on this
machine. It prints any command it is about to run that is not its own.

Or download a build from the
[latest release](https://github.com/Sup095/outlaw-repair-kit/releases/latest)
and run it: it is a single binary with no runtime to install first. There is a
desktop app there too. See [Installing](docs/install.md) for every option,
including building from source.

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
outlaw link             # pair with another computer to borrow its model
outlaw boot             # self-test and update check
outlaw report           # turn a crash or an error into a bug report
```

There is a desktop app too, with the same abilities and a **Machines** screen
for pairing two computers -- see [docs/desktop.md](docs/desktop.md).

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
| Application launch check | Installed command-line programs that no longer start, or that hang on startup |
| Recent system log errors | Crashes, driver faults, and hardware errors, with repeats grouped together |

A **full** scan adds the application launch test, which starts catalogued
applications such as Steam to see whether they actually open, and closes them
again. That one is not part of a quick scan on purpose: a scan you asked to be
quick should not open windows on your desktop.

Checks that cannot run -- wrong platform, missing tool, elevation not granted --
are reported as skipped with the reason. A scan never quietly covers less than
you think it did.

## Documentation

| | |
| --- | --- |
| [Installing](docs/install.md) | The installer, the desktop app, or from source |
| [Getting started](docs/getting-started.md) | Run your first scan |
| [Command reference](docs/commands.md) | What every command does |
| [The desktop app](docs/desktop.md) | The window, and the start-up self-test |
| [Setting up a model](docs/ai-setup.md) | Local, another machine, or hosted |
| [Linking two machines](docs/linking.md) | Pair two computers so one lends the other a model |
| [Using another machine](docs/remote-machine.md) | Point at an endpoint by hand, over any network |
| [Fixing problems safely](docs/fixing.md) | What it will and will not change |
| [Writing runbooks](docs/runbooks.md) | Teach it about a problem it does not know |
| [Troubleshooting](docs/troubleshooting.md) | When something is not working |
| [Reporting a problem](docs/reporting.md) | Turn a crash into an issue, with your details taken out |
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
4. **Desktop application** -- done. Everything is configurable from the window,
   with no file editing required. See [The desktop app](docs/desktop.md).
5. **Start-up self-test, update check, and installers** -- done. See
   [Installing](docs/install.md).
6. **Linking two machines** -- done. Pair two computers with a code so one can
   lend the other a model, no private network required. See
   [Linking two machines](docs/linking.md).
7. **Full and Deep tiers, plus a background watcher.** The Full tier now runs
   the application launch test; the stress and burn-in work for Deep is not
   built yet.
8. **Verifiers for the fix engine** -- started. A stale lock file and the
   Steam-will-not-launch case can now be re-tested, so the loop can act on
   them rather than only describing them. Each new verifier moves another
   class of problem from "explained" to "fixed".
9. **Escalation mode** -- to be proposed and reviewed for safety before it is
   built, not bolted on.

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

Copyright (c) 2026 Outlaw Systems.
