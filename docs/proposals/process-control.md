# Proposal: process control and cleanup

**Status: proposed, not built.** This is for review before any of it exists,
the same way escalation mode is. Nothing here is in the tool.

*In a subdirectory on purpose: `docs/` is the manual, compiled into the binary,
and a manual must only describe what the program actually does. A proposal is
not documentation.*

---

## What is being asked for

Four things, in the words they were asked for:

1. Choose to shut off all non-essential processes to clear the system and free
   resources.
2. Start things like Steam back up afterwards.
3. Detect processes you do not use at all and that are not essential.
4. Choose to block those permanently, to save the resources they take while
   running for no reason.

Process Lasso is the nearest existing thing. The difference this tool can make
is not more control — it is **being honest about what it did and able to put it
back**, which is the same difference it makes everywhere else.

## The one hard problem

Everything else in this tool is reversible by copying a file first. Processes
are not. You cannot snapshot a running program, and terminating one with
unsaved work in it destroys that work with no way back.

That single fact shapes the whole design. It is why this proposal splits what
looks like one feature into four, and why the safest two are worth building
first even if the other two never happen.

## Four separate capabilities, deliberately not one

They are ordered by how hard they are to undo. Conflating them is exactly how a
tool ends up having permanently disabled something somebody needed.

| | What it does | How you undo it | Risk |
| --- | --- | --- | --- |
| **1. Idle-weight report** | Watches over days and reports what runs constantly without you ever using it | Nothing to undo | None. It only looks |
| **2. Quiet mode** | Suspends non-essential processes for a while; restores them | One command, or automatically | Low. Nothing is closed |
| **3. Stop and restart** | Closes a known set of programs; starts them again on request | The tool starts them back up | Medium. Programs actually close |
| **4. Permanent block** | Stops something starting with the machine at all | Undone from the same screen | Highest. Survives a reboot |

## 1. Idle-weight report

The observation the other three rest on, and useful on its own.

The question "which processes do I not use?" cannot be answered from a snapshot.
A program that has been open for three seconds and one that has been open for
three weeks look identical in a process list. It needs watching over time, and
the watcher already exists.

Per program, recorded over days: total processor time used, memory held,
whether it ever had a window in front of you, and whether it ever received
input.

What it must say, and the exact shape of it:

> `Foo Updater` has been running for 14 days, has held 380 MB the whole time,
> has used 4 seconds of processor time in total, and has never had a window in
> front of you.

What it must **never** say: "you do not use this." That is a claim about you,
made from four numbers, and it will be wrong for the backup tool that quietly
does its job at three in the morning. The report states what was observed and
lets the person recognise their own machine. This is the same rule the start-up
check follows: *this is where something hiding would put itself* is a fair
thing to say; *you have something hiding* is not.

Nothing in this stage stops anything.

## 2. Quiet mode

A session with a beginning and an end. `outlaw quiet start` suspends
non-essential processes; `outlaw quiet stop` puts every one of them back.

**Suspend, not terminate.** A suspended process stops using the processor
immediately, keeps everything it had in memory, and resumes exactly where it
was with nothing lost. `SIGSTOP`/`SIGCONT` on Linux; the equivalent on Windows.
It does not free memory, and the tool must say so rather than let people assume
a number it did not deliver.

**The restore list is written before anything is suspended.** For each process:
its identifier, image path, command line, working directory, and the service it
belongs to if any. It is written to disk first, so that a crash, a forced
reboot, or the tool being killed still leaves `outlaw quiet stop` able to do its
job from the file alone. This is the snapshot, in the only form processes allow.

**A deadman.** If nothing renews the session, everything is resumed
automatically. A machine must never be left in a state the tool put it in and
then forgot about. The stress test already works this way and for the same
reason.

**Verification, for the first time on a resource problem.** Free memory and
processor load are measured before and after, and the report states the actual
difference. If suspending forty processes freed nothing, it says so. It never
prints a benefit it did not measure. This is also what would let
`memory.high-pressure` have a verifier at last — re-running the same
measurement that produced the finding is exactly what the fix engine requires,
and it is why this capability fits the existing loop rather than sitting beside
it.

## 3. Stop and restart

Closing programs, and starting them again afterwards. This is where the "start
Steam back up" part lives.

Only for programs whose restart is **defined**: the tool knows the command that
starts it, and the program holds no user document that could be lost. Steam, a
launcher, an updater, a tray utility — yes. A text editor, an office
application, a browser, anything with a document in it — never, at any
severity, however much memory it is holding. There is no confirmation dialog
that makes destroying somebody's unsaved work acceptable.

That distinction has to be a property of a program, not a guess, which means a
list that ships with the tool and is added to deliberately. A program that is
not on it is suspended rather than stopped, or left alone.

## 4. Permanent block

Stopping something from starting with the machine at all.

Most of this already exists: the start-up check enumerates every entry, from
the registry, both Start-up folders, scheduled tasks, and the Linux equivalents.
Blocking is disabling one of those entries, and it is reversible because the
entry is recorded in full before it is touched.

The rules that would apply:

- Never on first sight. Something has to have been observed idle for days
  before it can be offered.
- The entry is recorded in full first, and one screen restores it.
- Nothing is ever blocked without being asked, and never in bulk.
- It changes what starts; it never deletes the program.

## What "essential" means

The word doing the most work in the request, and the one most likely to cause
harm if it is loose. Proposed as a layered list, never as a heuristic:

**Never touched, on any platform, for any reason:**
the kernel and session infrastructure; anything the security software of the
machine is made of (antivirus and endpoint agents — suspending one is
indistinguishable from what malware does, and will get the tool flagged, quite
rightly); the display, input, and audio stack; the network stack; disk
encryption; accessibility software, because suspending a screen reader locks
somebody out of their own computer; the tool itself and the terminal or window
it is running in.

**Not touched by default:** anything running as SYSTEM or root; anything with a
window currently in front of you; anything started in the last few minutes;
anything the person has pinned.

**Candidates:** everything else, and even then only what the idle-weight report
has actually watched.

The list ships with the tool, is visible, and is editable. A hidden list of
what a tool considers essential is a list nobody can check.

## What it must never do

- Never terminate a process that could hold unsaved work.
- Never act without being asked, and never in bulk without showing the list
  first.
- Never claim resources were freed without measuring the difference.
- Never say "you do not use this." Say what was observed.
- Never leave the machine in a state it will not restore itself from.
- Never require administrator rights to run the parts that do not need them.

## Where it would live

Following the existing seams rather than cutting new ones:

- `ork-core/src/processes/` — enumeration, classification, and the essential
  lists, one per platform behind the existing platform trait.
- A probe, `processes.idle-weight`, emitting `process.idle-resident`. It
  declares what it emits like every other probe, and needs a runbook answer
  before it can ship.
- Two new typed actions in the fix engine's closed set — suspend and resume —
  and a verifier that re-measures memory and load. Typed operations, never a
  command string, exactly as now.
- `outlaw processes` and `outlaw quiet start|stop|status`, with `--json`.
- A **Processes** screen in the window, doing nothing the command line cannot.

## Order of work

1. **Idle-weight report.** No risk, immediately useful, and it produces the
   observations everything else needs. Worth building even alone.
2. **Quiet mode with suspend and restore**, including the restore file and the
   deadman.
3. **Measured verification**, which also gives the memory-pressure finding its
   first verifier.
4. **Stop and restart**, for the defined list only.
5. **Permanent block**, built on the start-up enumeration that already exists.

Stages 1 to 3 are worth having on their own. If the answer to stage 4 is ever
"not safely", the first three still do most of what was asked.

## Questions this proposal does not answer

- **Elevation.** A good number of the heaviest processes run as SYSTEM. Reaching
  them means the elevation broker, which is a separate piece of work. Quiet mode
  without it would still work on everything running as you, and should say
  plainly what it could not reach rather than quietly covering less.
- **Should quiet mode survive a reboot?** Proposed as no: a reboot restores
  everything. That is the safer default and the easier promise to keep.
- **Launching a game into quiet mode.** `outlaw quiet run <program>` — quiet
  until it exits, then everything back. Attractive, and worth deciding on
  separately.
- **How long is "you do not use this"?** Fourteen days is a guess. It should be
  a setting with a stated default, not a constant buried in the code.
