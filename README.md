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
> fix engine, linking two machines, the background watcher, and the stress and
> burn-in test all exist and work. All three scan tiers run. The Deep tier once
> also promised a rootkit scan; that promise has been withdrawn rather than met
> with something weaker, and [the reason](docs/startup.md) is worth reading.
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

Download **`outlaw-setup.exe`** (Windows) or **`outlaw-setup`** (Linux) from the
[latest release](https://github.com/Sup095/outlaw-repair-kit/releases/latest)
and run it. That is the whole thing: a small window, a quick download, and no
terminal. It is the only file on that page you need — it installs the
command-line tool, the window if you tick it, and a model if you say yes.

It asks which version you want, shows you a list of exactly what it is about to
do to your computer, and does it. It **refuses** to install any file whose
checksum does not match the one published with the release -- and refuses just
as firmly when it cannot check at all. It never asks for
administrator rights. It offers -- rather than assumes -- to set up a model
sized for whatever graphics card it finds, and tells you how many gigabytes
that means before you agree. Afterwards it shows what it did, and writes the
same list beside the installed files.

It carries no copy of the tool inside it, so it stays a quick download however
large the tool becomes.

> Not to be confused with `outlaw-repair-kit-<version>-x64-setup.exe`, also on
> that page, which installs only the window. The one you want is
> `outlaw-setup`.

<details>
<summary>Prefer a terminal?</summary>

**Windows**, in PowerShell:

```powershell
irm https://raw.githubusercontent.com/Sup095/outlaw-repair-kit/main/install/install.ps1 | iex
```

**Linux**:

```sh
curl -fsSL https://raw.githubusercontent.com/Sup095/outlaw-repair-kit/main/install/install.sh | sh
```

The same work: checksum-verified, no administrator rights, and it prints any
command it is about to run that is not its own.

Or download a build from the release and run it -- it is a single binary with
no runtime to install first. There is a desktop app there too.

</details>

See [Installing](docs/install.md) for every option, including building from
source.

### Opening it

| | The window | A terminal |
| --- | --- | --- |
| **Windows** | Start menu -> **Outlaw Repair Kit** | Start menu -> **Outlaw Repair Kit (terminal)**, or type `outlaw` |
| **Linux** | applications list -> **Outlaw Repair Kit** | applications list -> **Outlaw Repair Kit (terminal)**, or type `outlaw` |

`outlaw` on its own is not an error -- it says what the tool is and the handful
of commands worth knowing. Do not click `outlaw.exe` itself: it is a
command-line program, so clicking it opens a console, prints, and closes it
again faster than anybody can read. That is what the shortcut is for.

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
outlaw watch            # keep looking, and speak up only when something changes
outlaw stress           # work the machine hard on purpose, and see what it gets wrong
outlaw processes        # what is running, and what a sweep would leave alone
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
| Failed services | Services set to start automatically that are stopped *and* exited with an error -- the qualifier that keeps a healthy machine from reporting half a dozen it stopped on purpose |
| Application launch check | Installed command-line programs that no longer start, or that hang on startup |
| Recent system log errors | Crashes, driver faults, and hardware errors, with repeats grouped together |
| Disk health *(full scan)* | A drive that says it is failing. Reported as the drive's own verdict, never as an interpretation of raw SMART attributes. On Linux this is Disk health (SMART), which asks the drive directly and so needs `smartmontools` and root -- without them it reports that it could not check, rather than nothing |
| Start-up entries *(full scan)* | Everything that has arranged to start with the machine -- and, among those, entries pointing at a program that is not there, running out of a temporary folder, or carrying a command written so it cannot be read |
| Application launch test *(full scan)* | Launchers and graphical applications that will not start -- started for real and watched, which is why it is not part of a quick scan |
| System file integrity *(deep scan)* | Operating system files that no longer match what installed them -- a half-finished update, a file the disk corrupted, or something that replaced a system binary. It reads and hashes most of what is installed, which is why it is the only thing in the deep tier |

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
| [Watching for changes](docs/watching.md) | Notice a problem appearing, instead of going looking |
| [What starts with your computer](docs/startup.md) | Everything that runs on its own, and why this is not a rootkit scan |
| [Stress and burn-in](docs/stress.md) | Work the machine hard, to find what watching cannot |
| [Writing runbooks](docs/runbooks.md) | Teach it about a problem it does not know |
| [Troubleshooting](docs/troubleshooting.md) | When something is not working |
| [Reporting a problem](docs/reporting.md) | Turn a crash into an issue, with your details taken out |
| [Architecture](docs/architecture.md) | How the pieces fit together |

All of it is also inside the program: `outlaw docs`, or the **Info** screen in
the window. A machine that has gone wrong is often one that cannot reach this
page.

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

The window is built with Tauri rather than with `cargo` directly. Building it
with plain `cargo build` produces a program that expects the development server
to be running and shows *"can't reach this page"* when it is not, because the
front-end is only compiled in by the Tauri build:

```bash
cd apps/desktop && npm install && npm run tauri build
```

The window has its own tests, which include checks that read the Rust source
and compare it against the TypeScript -- the one join here that no compiler can
see across:

```bash
cd apps/desktop && npm test
```

Some tests need the network, a running model server, or a quiet machine, and
are skipped by default. They are the ones that check this tool against the
world rather than against a fixture, so they are worth running before a
release -- and on any platform this is new on:

```bash
cargo test --workspace -- --ignored
```

## Layout

| Path | What it is |
| --- | --- |
| `crates/ork-core` | Diagnostic core: platform layer, probes, scan orchestration, process classification |
| `crates/ork-ai` | Model routing, runbook library, explanation, credential storage |
| `crates/ork-fix` | The fix engine: snapshots, the closed set of actions, the audit log |
| `crates/ork-boot` | Start-up self-test and update check, shared by both front-ends |
| `crates/ork-link` | Pairing two machines so one can lend the other a model |
| `crates/ork-cli` | The `outlaw` command-line front-end |
| `crates/ork-setup` | The graphical installer, which draws its own window |
| `apps/desktop` | The window: Svelte front-end, Tauri back-end |
| `docs/` | The manual, compiled into the binary. `docs/proposals/` is work not yet built |
| `tests/shared/` | Test data read by more than one language |
| `install/` | The one-line install scripts |

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
7. **Reporting a crash** -- done. Errors and crashes are recorded as they
   happen and turned into an issue you can post, with your personal details
   already taken out. Nothing is ever sent for you. See
   [Reporting a problem](docs/reporting.md).
8. **Verifiers for the fix engine** -- started. A stale lock file, a stopped
   service, an application that will not run, and the Steam-will-not-launch
   case can now be re-tested, so the loop can act on them rather than only
   describing them. Each new verifier moves another class of problem from
   "explained" to "fixed", and `outlaw fix` tells you the count before it
   starts.
9. **Full and Deep tiers** -- done. Full runs the disk health check and the
   application launch test. Deep verifies that the operating system's own files
   still match what installed them, which is what makes a deep scan take as
   long as it does. Full also lists **what starts with the machine**, which is
   the usual answer to why a computer is slow for its first two minutes and the
   place anything wanting to survive a restart has to put itself. **The rootkit
   scan has been withdrawn as a promise**, because such a check would be running
   on the machine it is checking and a green tick would be a confident lie --
   see [What starts with your computer](docs/startup.md). Stress and burn-in is
   built, and deliberately not part of any tier.
10. **An installer with a window, and the manual inside the program** -- done.
   Download one small file, run it, and read everything about the tool from
   the tool. See [Installing](docs/install.md).
11. **A background watcher** -- done. `outlaw watch` looks on an interval and
   reports only what changed: a problem appearing, getting worse, easing, or
   going away. The first look is silent, because a machine that already had
   six problems did not just develop six. A check that could not run clears
   nothing, because absent and fixed look identical and only one of them is
   good news. See [Watching for changes](docs/watching.md).
12. **Stress and burn-in** -- done, and deliberately not part of any scan.
    `outlaw stress` loads every core with arithmetic that has a known correct
    answer, so a core returning the wrong number is caught with its number
    attached, and fills a share of free memory with five patterns in turn,
    because each one catches a different physical fault. It watches the
    temperature throughout and stops itself if the machine gets too hot, always
    leaves a gigabyte of memory alone, changes nothing, and says out loud what
    a clean result does *not* prove. See [Stress and burn-in](docs/stress.md).
13. **Escalation mode** -- proposed, not built. For severe cases and in-depth
    debugging: looking much harder, with the argument for whether it should
    ever *act* harder kept deliberately separate. See
    [the proposal](docs/proposals/escalation-mode.md).
14. **Process control and cleanup** -- looking, not yet acting. `outlaw
    processes` and the **Processes** screen show what is running -- grouped by
    program, because nobody thinks in processes -- what could be stopped, what
    is held back and why, and what is never touched at all: system processes,
    drivers, control panels, security software, and anything with a window in
    front of you. Each program says how much of it a sweep would offer, because
    a program is usually not all one thing and stopping part of it leaves it
    running, and anything you want left alone for good can be pinned from
    either front-end. **Nothing stops anything yet**, on purpose:
    the list exists on its own first so that it can be read on real machines
    before a button can act on it. The button, the confirmation, and putting it
    all back are the next stage. See
    [the proposal](docs/proposals/process-control.md).
15. **CritterScript** -- proposed, not built. The way the terminal is spoken to
    is being replaced with a language written for this project: closer to
    saying what you want than to remembering a switch, and ours rather than
    assembled out of somebody else's argument parser. It is a breaking change
    to how commands are typed, it will happen before 1.0, and the old syntax
    will spend one version telling you the new way of asking rather than
    failing. `--json` output is not affected. The language already exists and
    has been read; the plan is written against it rather than against a
    description of it. See [the proposal](docs/proposals/critterscript.md).

These are written down before they exist, because the argument is the hard part
and it is worth losing one about a document rather than about somebody's
machine.

Every released version and what it changed is in [the changelog](CHANGELOG.md),
which is also readable from inside the program: `outlaw docs changelog`, or the
**Info** screen in the window.

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

It also prints every file and folder the tool writes to, and says which of them
exist yet. That list is the whole list -- nothing is written outside it, and
deleting any of it is safe. If you leave the watcher running, one of those
files is a record of problems this machine has had, kept so that a problem
returning is recognised as a return; deleting it is a complete reset.

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
