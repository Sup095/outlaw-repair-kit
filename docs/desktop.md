# The desktop app

The same tool as the command line, with a window. Anything the app can do,
`outlaw` can do — that is a rule the project is built on, not a coincidence, so
nothing you set up in the app traps you in it.

## Starting up

Both front-ends run the same start-up sequence before anything else:

1. **Six self-checks.** Can it read this machine, are its checks registered
   without clashing, did its settings load, did its runbook library parse, is
   its state database intact, and can it actually write to the folder it keeps
   snapshots in.
2. **An update check.** It asks GitHub whether a newer release exists.

A diagnostic tool that is quietly broken is worse than none, because its clean
bill of health gets believed. That is the whole reason this runs every time.

Checks pass, warn, or fail, and the difference matters:

| | Meaning | Effect |
| --- | --- | --- |
| **ok** | Working | — |
| **warn** | Degraded but usable — say, an unreadable settings file, so defaults are in use | Start-up continues |
| **fail** | Something the tool depends on is broken — say, a snapshot folder it cannot write to | Start-up continues, but `outlaw fix` refuses to change anything |

That last row is the important one. If the tool cannot write a backup, its
promise to roll a failed fix back is empty, so it will not start applying
fixes. Scanning and explaining still work.

### The update check never installs anything

It tells you a newer version exists and where to get it. Replacing a running
program is your decision, and a tool that rewrites itself while you are in the
middle of diagnosing a broken machine is not a tool anybody wants.

### Skipping start-up

```bash
outlaw scan --no-boot
```

Start-up is skipped automatically for `--json` runs and for the quick commands
(`config`, `host`, `probes`, and so on) — putting a network check in front of a
one-line answer would be silly. It runs before `scan` and `fix`, which are the
ones you sit and watch.

Run it on its own with:

```bash
outlaw boot
```

## The screens

| Screen | What it is for |
| --- | --- |
| **Scan** | Pick how thorough to be, watch checks report as they finish, read the findings, and ask for an explanation |
| **Queue** | Problems waiting to be worked through, worst first |
| **Models** | Which model would handle this run, and exactly why the others were passed over |
| **Machines** | Pair with another computer so one can lend the other a model, and see what is wrong over there |
| **Settings** | Everything you would otherwise hand-edit a file for: routing, endpoints, API keys |
| **Audit** | Everything checked, found, attempted, and changed |

### Stopping a scan

The **Stop** button is available the whole time a scan is running. No tier and
no individual check has a time limit — the only thing that ends a scan early is
you. A check that goes quiet is reported as stalled rather than killed on a
timer, because "slow" and "stuck" are different things and only one of them is
a problem.

### Settings, and why they are here

Nobody should have to hand-edit a configuration file to point this at their own
machine. Everything in `config.toml` is editable in the window, and API keys go
to the operating system's own credential store — never into the settings file,
and never back to the window once saved. The Settings screen shows only whether
a key is stored, not what it is.

## Fixing, from the app

Working the queue is still a command-line action:

```bash
outlaw fix          # a dry run: shows what it would do
outlaw fix --apply  # confirms each change individually before making it
```

This is deliberate for now. The confirmation step before a system-level change
is the safety rail that matters most, and it is worth getting the window's
version of it right rather than shipping it early. See [fixing.md](fixing.md).

## Building it yourself

```bash
cd apps/desktop
npm install
npm run tauri dev     # with hot reload
npm run tauri build   # a real installer for your system
```

The window is [Tauri 2](https://tauri.app) with [Svelte 5](https://svelte.dev)
inside it. The Rust side is `apps/desktop/src-tauri`, and every command it
exposes is a call into the shared crates — see
[architecture.md](architecture.md).
