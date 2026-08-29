# The desktop app

The same tool as the command line, with a window. Anything the app can do,
`outlaw` can do — that is a rule the project is built on, not a coincidence, so
nothing you set up in the app traps you in it.

## Opening it

| | |
| --- | --- |
| **Windows** | Start menu -> **Outlaw Repair Kit** |
| **Linux** | applications list -> **Outlaw Repair Kit**, or type `outlaw-repair-kit` |

The window is a separate download from the command-line program -- see
[Installing](install.md). Its own installer adds the Start menu entry on
Windows and the `.deb` adds the applications entry on Linux; if you installed
the AppImage with the install script, that adds one too, because an AppImage
has no installer of its own to do it.

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
| **Checks** | Every check this build knows how to run, grouped by tier, what each one can report, and whether it can run on this machine |
| **Watching** | Look on an interval and hear only about what changed |
| **Stress** | Work the machine hard on purpose, and see whether it gets anything wrong |
| **Processes** | What is running, what could be stopped, what is held back and why, and what is never touched. It only looks — see below |
| **Queue** | Problems waiting to be worked through, worst first, each saying when it was last actually seen — and the buttons that work them |
| **Models** | Which model would handle this run, and exactly why the others were passed over |
| **Machines** | Pair with another computer so one can lend the other a model, and see what is wrong over there |
| **Settings** | Everything you would otherwise hand-edit a file for: routing, endpoints, API keys |
| **Audit** | Everything checked, found, attempted, and changed |
| **Report a problem** | Turn a crash or an error into an issue you can post |
| **Info** | The whole manual, the version, and the licence — carried inside the program rather than linked to |

A scan you have run stays where it is when you look at another screen and come
back. So does a report you have started writing. Neither is thrown away by
going to look something up, which is exactly when somebody would.

### The Processes screen only looks

It stops nothing. There is no button on it that changes anything, and it says
so at the top rather than leaving somebody hunting for one.

It opens with **By program**, because nobody thinks in processes. Several
processes sharing one name are one application to whoever is looking at them,
and the number that means anything is the total. Each line says what a program
holds between its processes, how many of them there are, and how many a sweep
would offer — and that last column is the point, because **a program is often
not all one thing.** Some of its processes would be offered and some held back,
so stopping the offered ones leaves it running with fewer processes rather than
closing it. Where that is true of anything on screen, the screen says so
underneath, in amber, rather than letting somebody discover it by finding the
window still open afterwards.

Then three groups, of which the second two are the interesting ones: what could
be stopped, heaviest first, with what each is holding; what is held back and
why; and what is never touched at all, counted by reason. Nothing with a window
in front of you is offered — nor what started it, nor its other processes,
because a game and the launcher it is running inside go together and a browser
is forty processes sharing one name.

Among the reasons for holding something back is **`this tool could not start
it again`** -- a program whose own path could not be read. A sweep is
reversible only in the sense that a person can start again what it stopped, and
that is not an instruction anybody can follow about a program the tool cannot
name a way back from. It is rare, and it is the rail for the remainder rather
than a rule that shapes the list.

Each program has a **Leave alone** button. A program left alone is never
offered for stopping, whatever else the tool decides about it -- and it is the
one control on this screen that changes anything, though what it changes is a
setting rather than the machine. It appears only where it would mean something:
a program nothing would ever touch is already left alone. The same thing on the
terminal is `outlaw processes --pin <name>`.

The per-process list is the honest one and stays. It is what to check when a
number in the grouped view looks wrong, and the two are built from one survey
so they cannot disagree.

The memory figure says what those programs are **holding**, never what stopping
them would give back. Those are different numbers, the second is always
smaller, and the honest version can only be produced by measuring afterwards.

Where a rule could not be applied, the screen says so in place of applying it
quietly. On a Linux machine running Wayland the tool cannot ask what has the
window in front of you — that is deliberate on Wayland's part — so the list
carries **One rule did not run** and names the reason, because that rule not
running means the list may include what you are looking at.

The same thing on the terminal is `outlaw processes`.

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

## The look

Neon. Electric cyan and hot magenta over a near-black that leans violet, with
amber for the things that are ours and red for the things that are wrong.
Behind everything: two grids offset from each other so they cross rather than
stack, a magenta bloom low and a cyan one high as though something off-screen
is lit, scanlines, and a vignette that keeps the corners dark so the middle
reads as the lit part. Panels are lit boxes, bracketed cyan where reading
starts and magenta where it ends. The same palette as the terminal boot
screen, so the two front-ends read as one tool.

It is deliberately disciplined about where the neon goes. Chrome, headings,
borders, and states glow: the active tab, a button under the pointer, a
severity badge, a scan in progress. Body text does not -- no glow, no tinting,
no colour games. This is a screen somebody reads when their computer is broken,
and one that looks tremendous and is tiring to read has failed at the only job
it has.

The one thing that moves on its own is a pulse travelling along the rule under
the header, once every eight seconds. It sits in a single row of pixels at the
top of the window, well clear of anything anybody is reading, and it is there
so the window reads as running without asking for a glance.

If you have asked your system not to animate things, nothing moves. The
start-up sequence still runs; it just stops sliding.

## Watching, from the app

The **Watching** screen starts a watcher that looks on an interval and reports
only transitions -- a problem appearing, getting worse, easing, or going away.
It is the same watcher as [`outlaw watch`](watching.md), sharing the same
memory, so a machine can be moved between the window and a scheduled task
without losing its history.

The screen is laid out around the fact that **an empty left-hand panel is the
good outcome**. Nothing appears there between changes, which is what a working
watcher looks like. What it currently believes is always on the right: what is
wrong now, how many problems it has seen before and are not there any more,
and -- always listed, never merely dropped -- anything it is holding quiet for
coming and going too often.

The first look records how the machine is now and reports nothing. That is
deliberate, and the screen says so: a computer that already had six problems
did not just develop six problems.

Two things worth knowing:

- **It keeps running when you leave the screen.** That is the entire point of
  it. What it noticed while you were on another tab is waiting when you come
  back, and so is what it noticed while the window was shut, because the
  record it keeps is on disk rather than in the window.
- **A check that could not run clears nothing**, and the screen says which
  checks those were, because a check reporting nothing and a check reporting a
  repair look identical and only one of them is good news.

**Forget and start over** throws away everything it remembers. It asks first.
Afterwards the next look records a fresh starting point and reports nothing,
which -- if you did not mean to press it -- looks exactly like a watcher that
has stopped noticing anything.

Nothing here fixes anything. Findings go to the **Queue** in the ordinary way.

## Stress, from the app

The one screen here that acts on your computer rather than watching it. It
loads every core and fills a share of the free memory, deliberately, for as
long as you ask -- because a whole class of fault is invisible to observation
and shows up only under load. The whole of it is described in [Stress and
burn-in](stress.md); what is worth saying about the *screen* is what it does
before and during a run.

**It says what it is about to do, in numbers, and waits.** Pressing **Start**
does not start anything. It replaces itself with the real figures for this
machine -- how many minutes, how many cores, how much memory of how much free,
and how much is being left alone -- and a second button. Those numbers track
the slider as you move it, so the amount shown is the amount that would really
be touched rather than a share you have to do arithmetic on.

**Stop is the first thing on the panel while it runs**, and it is immediate.
This is the one screen in the application where stopping is a safety control
rather than a convenience, and it is placed accordingly.

**It tells you if nothing is watching the temperature** while the run is going,
not afterwards in the result. On a machine that reports no temperature that can
be believed -- common on Windows without administrator rights, and on desktop
boards that never publish one -- the run cannot stop itself when things get
hot, and somebody about to heat a laptop should know that while there is still
something they can do about it.

**A fault appears the moment it happens.** It is not held back until the end.

Leaving the screen does not stop the run or lose the result: both live outside
the screen, the same way a scan does. Closing the window *does* stop it, which
is the difference between this and the watcher -- a watcher that keeps watching
after the window is closed is doing its job, and a stress test that kept
heating the machine would be a program that had escaped.

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
