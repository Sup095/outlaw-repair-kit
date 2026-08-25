# The desktop app

The same tool as the command line, with a window. Anything the app can do,
`outlaw` can do — that is a rule the project is built on, not a coincidence, so
nothing you set up in the app traps you in it.

## Starting up

Both front-ends run the same start-up sequence before anything else:

1. **Seven self-checks.** Can it read this machine, are its checks registered
   without clashing, did its settings load, did its runbook library parse, is
   its state database intact, can it actually write to the folder it keeps
   snapshots in — and did anything crash last time.
2. **An update check.** It asks GitHub whether a newer release exists.

A diagnostic tool that is quietly broken is worse than none, because its clean
bill of health gets believed. That is the whole reason this runs every time.

Checks pass, warn, or fail, and the difference matters:

| | Meaning | Effect |
| --- | --- | --- |
| **ok** | Working | — |
| **warn** | Degraded but usable — say, an unreadable settings file, so defaults are in use | Start-up continues |
| **fail** | Something the tool depends on is broken — say, a snapshot folder it cannot write to | Start-up continues, but nothing is allowed to change your system |

That last row is the important one. If the tool cannot write a backup, its
promise to roll a failed fix back is empty, so it will not start applying
fixes. Scanning and explaining still work.

### It tells you if it crashed last time

A window has no terminal behind it, so a crash leaves nothing on screen: you
close the app, open it again, and never learn there was anything to report.
The last self-check exists for that. A recorded crash shows as a **warn** with
a pointer at [Reporting a problem](reporting.md), which is not enough to stop
anything — the tool works; it merely fell over once.

Handled errors are counted and mentioned but never warned about. A great many
of them are a network hiccup or a machine saying no, and warning about those on
every start would teach you to skip the line that matters.

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
| **Queue** | Problems waiting to be worked through, worst first — and the buttons that work them |
| **Models** | Which model would handle this run, and exactly why the others were passed over |
| **Machines** | Pair with another computer so one can lend the other a model, and see what is wrong over there |
| **Settings** | Everything you would otherwise hand-edit a file for: routing, endpoints, API keys |
| **Audit** | Everything checked, found, attempted, and changed |
| **Report a problem** | Turn a crash or an error into an issue you can post |

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

The Queue screen has two buttons.

**Preview** works the whole queue without being allowed to change anything. It
is not a separate code path pretending to be a rehearsal — it takes exactly the
same route as a real run and is simply never given permission, so what it shows
is what would actually have happened.

**Work the queue** allows changes, and asks before every single one:

> **This would change your system**
> Restart the service `spooler`
> to address: A service that should be running is not

Nothing happens until that question is answered. Three things are worth knowing
about it:

- **Only "Allow it" is consent.** Closing the window, an answer that arrives
  garbled, a stopped run — all of them decline. There is no path through this
  code where silence or confusion means yes.
- **Every question is answered once, by name.** A click that arrives after the
  question has moved on is discarded rather than applied to whatever is on
  screen now.
- **There is no time limit on answering.** A prompt about changing your computer
  that answers itself because you went to make a cup of tea is not a prompt.
  **Stop** is available the whole time instead.

Before it starts, the screen says how many of the waiting problems can actually
be tested after a change — because only those can be fixed rather than
explained, and that number is the honest measure of what this tool is doing for
you. If no system-level snapshot tool was found, it says that too, rather than
letting you assume a safety net that is not there.

The command line does the identical thing, and both go through the same engine
and the same queue:

```bash
outlaw fix          # a dry run: shows what it would do
outlaw fix --apply  # confirms each change individually before making it
```

See [fixing.md](fixing.md) for what happens between the confirmation and the
result: the snapshot, the test, and the rollback when the test does not pass.

## Reporting a problem

A window has no terminal behind it, so a crash there would otherwise leave
nothing at all. Errors and crashes are recorded to a file as they happen, and
the **Report a problem** screen turns that record into something postable.

What it shows is exactly what would be posted, with personal details already
taken out — home directory paths, account and machine names, email and network
addresses, and anything shaped like a key. **The text is editable**, and what
gets carried into the issue form is what is on screen when you press the button,
not what the tool generated. There is also a folded-away view of the raw record
so you can see what the report was built from.

**Nothing is sent for you.** The button opens GitHub's issue form with the text
filled in; you read it and press Submit there. See
[Reporting a problem](reporting.md).

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
