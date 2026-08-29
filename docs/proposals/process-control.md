# Proposal: process control and cleanup

**Status: stages one to three are built. A sweep stops things, and nothing is
put back.**

What exists: the enumeration, the classification, the lists of what is never
touched, `outlaw processes`, the **Processes** screen in the window, and the
sweep itself -- `--stop` in the terminal, **Stop these** in the window -- with
the list shown in full before it is agreed to and every entry judged again
against a fresh look at the machine before anything happens to it.

What does not exist, and the part of this document that is still a proposal:
the elevation broker, and everything that depends on it. **Undo works
differently here from everywhere else in this tool, and that was decided
rather than deferred** -- see "Putting it back" below.

The order was deliberate: the list is worth having on its own, and it needed to
be read on real machines before anything could act on it. Doing that found two
faults that would have mattered a great deal more with a button attached, both
recorded below.

*In a subdirectory on purpose: `docs/` is the manual, compiled into the binary,
and a manual must only describe what the program actually does. A proposal is
not documentation.*

---

## What is being asked for

1. One button that stops everything non-essential, completely, so the machine
   is clear and you can start what you actually want without overhead from
   things you are not using.
2. Start Steam, or whatever else, back up afterwards.
3. Detect programs you never use that are not essential.
4. Block those permanently if you want to.

Essential means: system processes, drivers, control panels — audio, graphics,
that sort of thing — and security software.

Process Lasso is the nearest existing thing. The difference this tool can make
is not more control. It is **being honest about what it did and able to put it
back**, which is the difference it tries to make everywhere else.

## The one hard problem

Everything else in this tool is made reversible by copying a file first. You
cannot snapshot a running program. Closing one that has unsaved work in it
destroys that work, and no confirmation dialog makes that acceptable.

Stopping completely is what was asked for, and it is the right default —
suspending frees processor time but not memory, and memory is most of the point.
So the design cannot lean on "we only suspended it" for safety. It has to lean
on **never stopping anything that could lose you something**, and on knowing how
to start things again before it stops them.

## The button

**Name:** *Stop everything non-essential.* Plain, and says what it does.

**Where:** the Processes screen in the window, and `outlaw quiet start` in the
terminal. Nothing the window can do is unreachable from a script, as everywhere
else.

### Before the button does anything

The screen shows the list first, always. Pressing the button opens the
confirmation; it never acts directly. What is on screen:

- **Every process that would be stopped**, with the memory it is holding and
  how long it has been running, sorted by memory.
- **The total that would be freed**, stated as an estimate, because a working
  set is not the same as memory returned to the machine and it must not be
  presented as though it were.
- **What will not be touched, and why** — collapsed, but there, because a list
  of what a tool considers untouchable is worthless if nobody can read it.
- **Anything it does not know how to start again**, called out separately.

Each row has a checkbox. A row can be excluded permanently, and that choice is
remembered.

### The three classes

Each candidate falls into one, and the class decides the default:

| Class | What it means | Default |
| --- | --- | --- |
| **Safe to stop** | No document state, and the tool knows the command that starts it again — launchers, updaters, tray utilities, Steam, game clients, sync agents | Ticked |
| **May hold unsaved work** | Browsers, chat clients, anything with a text box in it | **Unticked.** You can tick it; it will never be ticked for you |
| **Cannot be restarted by the tool** | It knows how to stop it and not how to start it — started by Explorer, or by a service, or with an environment it cannot reproduce | **Unticked**, and labelled as one-way |

Editors, office applications, and IDEs are never offered at all, at any
severity, however much memory they are holding. There is no severity of
slowness that justifies losing an afternoon's work.

That third class is the one most tools quietly get wrong. If the tool cannot
start something again, saying so **before** you press the button is the whole
difference between a feature and a trap.

### The confirmation

A separate dialog, not a checkbox on the same screen. Roughly:

> **Stop 23 programs?**
>
> This will close them now. About **6.2 GB** should come back, though the real
> figure is usually lower — that is what will actually be measured and reported
> afterwards.
>
> - **2 of these may have unsaved work in them.** *Discord, Firefox.* Anything
>   you have not saved in those will be gone. Untick them if you are not sure.
> - **1 cannot be started again by this tool.** *Realtek Audio Console.* You
>   would start it yourself, or it will come back at the next restart.
> - Some will start themselves again straight away. That is not a fault, and
>   it will be reported honestly rather than counted as freed.
>
> Nothing essential is touched: system services, drivers, your graphics and
> audio control panels, and your security software are all left alone. The full
> list of what that covers is on the previous screen.
>
> **Nothing here is put back for you.** What was stopped is written down and
> shown afterwards, and starting anything again is yours to do.
>
> `[ Cancel ]`  `[ Stop 23 programs ]`

Cancel is the default. The confirm button carries the count so that pressing it
by reflex still shows what is about to happen. In the terminal the same thing
is typed rather than clicked.

**There is no `--yes`, and that is a decision rather than an omission.** Every
other confirmation this tool skips is skipped so that a scheduled task can run
unattended, and an unattended sweep is precisely the thing that must not exist:
nothing is snapshotted, nothing is restarted, and the only record afterwards is
one a person reads. A flag that closed somebody's programs every night would be
the tool working exactly as designed and doing something nobody wanted. The
capability is still reachable from a script -- `--stop` *is* the script -- it
just needs somebody at the keyboard, for the same reason the window needs
somebody at the mouse.

### After

- What was actually stopped, what refused to stop, and what started itself
  again within seconds.
- The measured difference in free memory — **measured, not estimated**. If
  stopping 23 programs freed 900 MB rather than 6 GB, it says 900 MB.
- A list, on screen until it is dismissed, of what was stopped and what was
  left alone.

## Putting it back

**Decided: the tool records what it stopped and shows you. It does not start
anything again.** This is the one place where the tool's usual promise --
snapshot first, undo available -- cannot be kept, and saying so plainly is
better than an *Undo* that half works.

Three things stand in the way, and the first is enough on its own.

**A snapshot of a running program does not exist.** Every other change this
tool makes is to something at rest: a setting, a file, a service definition. A
running program is its memory, its open handles, its half-written files, and
the state of whatever it was talking to. What could be written down is the
*recipe* -- image path, arguments, working directory -- and running the recipe
again produces a new program that resembles the old one and is not it. Calling
that *undo* would be the tool claiming to have reversed something it replaced.

**The recipe is where the secrets are.** Command lines routinely carry
passwords, tokens, and database connection strings; that is a well-known bad
habit and it is somebody else's bad habit, on the machine this is running on.
Writing every argument of every stopped process to a restore file would mean
this tool creating a plaintext credential dump as a side effect of tidying up,
and then keeping it. That is a worse outcome than the one it was avoiding.

**A relaunch is not the same program.** Started again by the tool, it inherits
the tool's account, its environment, and its place in the process tree. For
anything that cares -- and services, elevated programs, and anything started by
another program all care -- that is a different program wearing the same name.

So the promise is smaller and it is kept: everything attempted is written to
the audit log, including everything left alone and why, and the report says
what was stopped and what it was holding when last seen. Starting something
again is a person's decision, made with the name in front of them.

*Restarting the machine remains the honest general answer, and it is the one
the screen gives.*

## What "essential" means

The word doing the most work here, and the one most likely to cause harm if it
is loose. A layered list, shipped visible and editable — a hidden list of what a
tool considers essential is a list nobody can check.

**Never touched, on any platform, for any reason:**

- The kernel, the session, and the process tree the desktop hangs off.
- **Security software.** Antivirus and endpoint agents. Suspending one is
  indistinguishable from what malware does, and would get this tool flagged,
  quite rightly.
- **Drivers and their control panels** — graphics, audio, chipset, input. The
  NVIDIA and Realtek style of thing. Some of them are genuinely idle and it does
  not matter: stopping the panel that owns your audio to save 40 MB is a bad
  trade every time.
- The display, input, and audio stack itself.
- Networking, and anything holding a network connection open on behalf of the
  system.
- Disk encryption and anything holding a volume open.
- **Accessibility software.** Stopping a screen reader locks somebody out of
  their own computer with no way to undo it.
- Backup and sync agents **mid-transfer** — idle ones are fair game, one with a
  file open is not.
- The tool itself, and the terminal or window it is running in.

**Not stopped by default:**

- Anything running as SYSTEM or root.
- Anything with a window in front of you right now.
- Anything started in the last few minutes.
- Anything you have pinned.

**Candidates:** everything else, and even then only shown with what it is
holding so the decision is yours.

## Finding what you never use

Cannot be answered from a snapshot. A program open for three seconds and one
open for three weeks look identical in a process list. It needs watching over
time, and the watcher already exists.

Per program, over days: processor time used, memory held, whether it ever had a
window in front of you, whether it ever received input.

What it says:

> `Foo Updater` has been running for 14 days, has held 380 MB the whole time,
> has used 4 seconds of processor time, and has never had a window in front of
> you.

What it must **never** say: *"you do not use this."* That is a claim about you
made from four numbers, and it will be wrong for the backup tool that quietly
does its job at three in the morning. State what was observed; let the person
recognise their own machine. Same rule the start-up check follows.

## Blocking permanently

Mostly already built. The start-up check enumerates every entry — registry, both
Start-up folders, scheduled tasks, and the Linux equivalents — and blocking is
disabling one of those, reversibly, because the entry is recorded in full first.

- Never on first sight. Something must have been observed idle for days.
- Never in bulk.
- It changes what starts. It never deletes the program.

## What it must never do

- Never close anything that could hold unsaved work without being ticked
  deliberately, one at a time.
- Never stop something it cannot start again without saying so first.
- Never claim memory it did not measure.
- Never say "you do not use this."
- Never leave the machine in a state it will not restore itself from.
- Never touch security software, drivers, or accessibility software.

## Where it would live

- `ork-core/src/processes/` — enumeration, classification, and the essential
  lists, one per platform behind the existing platform trait.
- A probe, `processes.idle-weight`, emitting `process.idle-resident`, declaring
  what it emits like every other probe and needing a runbook answer to ship.
- Typed actions in the fix engine's closed set — stop, suspend, resume, restart
  — never a command string, exactly as now.
- A verifier that re-measures free memory. This gives `memory.high-pressure` its
  first verifier: re-running the same measurement that produced the finding is
  precisely what the fix engine requires.
- `outlaw processes` and `outlaw quiet start|stop|status|resume`, with `--json`.
- A **Processes** screen in the window.

## Order of work

1. ~~**Enumeration and classification**, with the essential lists. No
   stopping.~~ **Built.**
2. ~~**The screen and the list**, showing what would be stopped and what is
   held back. Still no stopping.~~ **Built**, in both the terminal
   (`outlaw processes`) and the window (the **Processes** screen), from the
   same judgement so the two cannot disagree about what "held back" means.
3. **The button**, the confirmation, and the report. *Done.* The restore file
   was dropped rather than deferred -- see "Putting it back".
   **Next.**
4. **Measured verification**, which also gives memory pressure its verifier.
5. **Session survival** — the reboot toggle and picking up where it left off.
6. **Idle-weight watching** over days.
7. **Permanent blocking**, on top of the start-up enumeration.

Stages 1 and 2 were worth having alone, and looking at them on real machines
paid for itself twice before anything could act on the list: once when every
process on the machine came back a candidate because the owner check asked one
question for the whole machine rather than one per process, and once when the
running game was offered because nothing read the foreground window. Both would
have been considerably more expensive to discover with a button attached.

## Still open

- ~~**Nothing decides "could not be started again".**~~ **Decided.** A process
  whose own path cannot be read is held back, with that reason. It is narrow on
  purpose: a readable path is not a promise that starting the program from it
  restores anything, so the rule rules out the case with no answer at all
  rather than claiming to know which programs come back cleanly. Whether the
  tool should ever start something again *itself* is a separate question and
  the answer is no, and it has now been decided -- see "Putting it back".
- **Elevation.** Many of the heaviest processes run as SYSTEM. Reaching them
  needs the elevation broker, which is separate work. Without it the sweep still
  works on everything running as you, and must say plainly what it could not
  reach rather than quietly covering less.
- **How long is "you never use this"?** Fourteen days is a guess. A setting with
  a stated default, not a constant buried in the code.
- ~~**Nothing reads the foreground window yet.**~~ **Built.** Windows is
  asked directly, and the answer is widened to the focused process, everything
  it is running inside, and everything sharing a name with those -- so a game
  started from Steam protects Steam, and a browser's fortieth process protects
  the other thirty-nine. On Linux it is not answered: Wayland deliberately
  does not permit the question, and X11 would need a display connection this
  tool does not otherwise make. **Where it cannot be answered the list says
  so**, because unlike every other unknown here, not knowing holds nothing
  back -- it means the rail did not run, and a rail that did not run must not
  look like one that did.
- ~~**A row is a process, and a person reads it as a program.**~~ **Settled,
  and built as a second view rather than a replacement.** The problem was found
  by looking at the finished screen on a real machine: thirteen rows called
  `claude.exe` and eight called `steamwebhelper.exe`, which reads as twenty-one
  programs and is two. Honest -- they are twenty-one processes -- and still
  misleading, and the confirmation dialog would have inherited it in its worst
  form: *"Stop 23 programs?"* is wrong about the number and about the word.

  Both front-ends now open with **By program**: one line per name, with what it
  holds between its processes, how many there are, and how many a sweep would
  offer. The per-process list stays underneath and is still the honest one --
  grouping alone would have fixed the reading and lost the detail, and the
  detail is what somebody checks when a number looks wrong.

  Building it first, before anything can act, turned up the thing that made it
  worth doing early: **a group is usually not all one thing.** A program has
  processes a sweep would offer and processes it holds back, so stopping the
  offered ones leaves the program running with fewer processes. Saying "this
  would close Chrome" would have been a lie, and it is the kind that only
  shows itself after the button is pressed. The outcome of a sweep over a
  group is therefore a separate answer from the group -- `all of it`, `part of
  it`, or `none of it` -- and the screens say which, in the same voice as
  everything else here. Had this been left until the confirmation dialog, it
  would have been discovered from inside a dialog, at the moment of highest
  consequence.

- **Restarting something the sweep stopped.** Answered, and the answer is that
  this tool does not: see "Putting it back". Left here because it is the first
  thing anybody asks for after using the sweep once, and the reason should be
  findable rather than inferred from its absence.
- **`outlaw quiet run <program>`** — quiet until that program exits, then
  everything back. Attractive for launching a game. Decide separately.
