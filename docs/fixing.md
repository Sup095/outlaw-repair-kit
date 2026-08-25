# Fixing problems safely

This is the only part of the tool that changes your computer, so it is worth
being precise about what it will and will not do.

## The short version

```bash
outlaw scan          # problems needing work go on a queue
outlaw queue         # see what is waiting
outlaw fix           # show what would be done -- changes nothing
outlaw fix --apply   # allow changes, confirming each one
outlaw audit         # everything that has been done
```

The desktop window does the identical thing from the Queue screen -- the same
engine, the same queue, the same confirmation before every change -- so nothing
below is specific to the terminal. See [desktop.md](desktop.md#fixing-from-the-app).

`outlaw fix` on its own is a **dry run**. It never changes anything, whatever
it finds.

## How problems get queued

A scan splits what it finds:

- **Simple and unambiguous** problems are reported directly. There is one known
  correct answer and no investigation needed.
- **Complex or ambiguous** problems -- an application that will not start, an
  intermittent crash, a driver mismatch -- go on the **triage queue** with the
  full context captured: the exact error text, the evidence, the timestamp.

The queue does not block the scan. The scan finishes, and the queue is worked
afterwards, worst first: stability and security problems before application
annoyances, however long the annoyance has been sitting there.

The same problem seen by two scans is one queue item, not two. A problem on two
different drives is two items, because fixing one does not fix the other.

## The fix-attempt loop

For each queued problem, working through candidates least disruptive first:

1. **Take a snapshot** -- copy aside every file about to be touched.
2. **Apply one change.** One. Never several at once.
3. **Test whether it worked**, specifically for that problem.
4. **It worked** -- keep it, mark the problem resolved, and remember what
   worked so a repeat is fixed on the first try next time.
5. **It did not work** -- roll back completely, record that this candidate
   failed, and try the next one.
6. **Out of candidates** -- stop, and hand back everything that was tried
   rather than continuing to guess.

There is no time limit on this loop.

## What it will actually do to your machine

This is a short list on purpose.

| It can | It will not |
| --- | --- |
| Remove a stale lock or cache file (after backing it up) | Delete any of your data, ever |
| Restart a system service | Install, remove, or update packages |
| Run read-only inspection commands | Install or roll back drivers |
| | Change firmware or boot settings |
| | Format or partition anything |
| | Run arbitrary commands |

Everything on the right-hand column arrives as **an instruction for you**. The
tool explains what to do and stops. It would rather tell you to reinstall a
driver than reinstall it wrongly.

### Why so restrictive

Candidate fixes come from two places: runbooks written by people, and a model
reasoning about a problem nobody has written down. Only the first has been
reviewed.

If the tool could run arbitrary commands, every safety rule here would be
advice rather than a guarantee, and one confidently wrong model output could
destroy your data. So fixes are a **closed set of typed operations**. There is
no "run this command" operation at all -- not for the model, and not for
runbooks either. Anything outside that set becomes an instruction for a person.

Even within that set, actions are validated before they run:

- Inspection commands are limited to an allowlist of read-only programs, and
  cannot use flags that would change state (`systemctl status` yes,
  `systemctl stop` no).
- Files inside the operating system -- `/etc`, `/boot`, `/usr`, `C:\Windows`,
  and others -- can never be targets, and paths cannot use `..` to reach them.
- Only files that look like lock or cache files can be removed, and only after
  being backed up.
- Programs are launched directly with an argument list, never through a shell,
  so there is nothing for a quote or semicolon to break out of.

An action that fails validation cannot run, whatever produced it. The check
happens again immediately before execution, not only when the candidate was
built.

## Two rules that may surprise you

**A change is only applied if its result can be tested.** "One change at a
time, always testable and reversible" is not satisfied by a change nobody can
measure. A candidate with no way to test the outcome is offered as advice
instead of being applied. This is still why most of what the tool suggests is
advice rather than action -- see [What can be tested](#what-can-be-tested).

**"I could not tell" is treated as failure.** If the test cannot be carried
out, the change is rolled back rather than kept. Keeping it would mean the tool
had modified your computer and could not say whether that helped -- which is
exactly the state this loop exists to avoid.

## What can be tested

`outlaw fix` tells you this before it starts:

```text
1 of 4 can be tested after a change, so only those can be fixed automatically.
The rest are explained instead.
```

That number is the honest measure of how much of this tool fixes things versus
describes them, and it grows as verifiers are written.

| Problem | How it is re-tested |
| --- | --- |
| A stale lock or cache file | The file is gone |
| A service that should be running is not | The service manager is asked again |
| An installed program fails or hangs when run | It is asked to report itself again, the same way |
| Steam will not start | Steam is started, and watched |

### Some things are deliberately not on that list

A failing drive has no entry and never will. There is no change to a disk that
makes it un-fail, and every operation that looks like one -- a surface scan, a
repair pass, a reallocation sweep -- reads the whole drive hard, which is the
load most likely to finish it off. The tool reports it, explains what it means,
and tells you to copy your data off before doing anything else. That is the
correct answer, not a missing feature.

The same reasoning covers anything where the honest fix is a decision rather
than a command. A verifier that cannot exist is not written, because the engine
would then be willing to make a change it could not check.

**Every verifier re-runs the same test that found the problem.** That sounds
obvious and is easy to get wrong: if the check that finds a fault and the check
that declares it repaired are different tests, then "fixed" quietly comes to
mean something other than "not found any more". The launch test lives in the
core for exactly this reason, shared by the scan that reports Steam broken and
the verifier that later says it works.

### Applications, in detail

Two different tests sit behind the same pair of findings, and which one applies
depends on the program.

Most programs can be asked to report themselves and exit -- `git --version` and
its relatives. That is cheap, changes nothing, and runs in a quick scan.
Re-testing is a matter of asking again.

Programs with no such invocation -- launchers, clients, anything that only
really has a "start" -- have to be started and watched instead, which is the
full-tier launch test described below.

The two tables are kept apart deliberately: a program appears in one or the
other, never both, so which test applies is a property of the program rather
than of the order the verifiers happen to be registered in. There is a test
that fails the build if they ever overlap.

One answer is worth calling out. If the program is **no longer installed** when
the re-test runs, that is reported as "cannot tell", never as fixed. Gone is not
the same as mended, and a change that made a program disappear is one to undo
rather than congratulate.

### Services, in detail

A stopped service is only worth reporting if it was *meant* to be running. On
Windows the scan asks for services set to start automatically that are not
running **and exited with a non-zero code** -- without that last condition a
perfectly healthy machine reports a handful of services that stopped on purpose
and stayed stopped, which teaches you to ignore the list. On Linux the scan
reads systemd's own failed-unit list, which already carries that judgement.

Re-testing is a matter of asking the service manager again. The one subtlety is
what to do with an answer that is neither running nor stopped: a service that is
still starting has not arrived yet, and calling that fixed would be believing a
promise instead of an outcome. Those states are read as "cannot tell", and the
change is rolled back like any other untestable one.

### The Steam case, in detail

Steam is a launcher, so it cannot be tested the way `git --version` is. It has
to be started and watched. Three things can happen:

- **it exits with an error** -- still broken, and the error usually names why;
- **it stays up** -- it started, and the tool closes it again;
- **it exits straight away reporting success** -- and this one is *ambiguous*,
  because a launcher handing off to a copy of itself that was already running
  also exits successfully. That is reported as "cannot tell", never as fixed.

That last case is the one worth labouring. Reading it as success would be the
single easiest way for this tool to tell you your problem is solved when it is
not, so it is read as not knowing, and the change is rolled back.

Two more rules apply to the launch test:

- **Nothing is ever killed for being slow.** The tool watches for a failure for
  a period; a program still running at the end of it has *passed*. It is not a
  deadline on the work.
- **Only a process the tool started is ever stopped.** If Steam was already
  running before the test, the test does not run at all -- it would prove
  nothing, and closing something you are using is not the tool's business.

Because this test starts real applications, it belongs to the **full** scan
tier, never the quick one. A scan you asked to be quick has no business opening
windows on your desktop.

## Snapshots and rollback

Two different things, often conflated:

**Targeted backup** -- the tool copies aside exactly the files it is about to
touch, before touching them. Always available, needs no privileges and no setup.
This is what rollback actually uses, and it is why rollback is a promise rather
than a hope.

**System-level snapshot** -- a Windows restore point, a btrfs or Timeshift
snapshot. Much broader, but needs administrator rights and needs to have been
set up in advance. The tool *reports* whether you appear to have one, and never
assumes you do. Claiming a safety net that turns out not to exist would be
worse than admitting there is not one.

`outlaw fix` tells you what it found on your machine. If you do not have a
system-level snapshot, setting one up is worth doing regardless of this tool.

## Confirmation

With `--apply`, every system-changing action is described and confirmed
individually:

```
This would change your system:
  Remove /home/you/.steam/steam.lock (left behind by a crash) -- a copy is kept first
  to address: Steam hangs instead of starting
Allow it? [y]es / [n]o / [s]top:
```

Anything that is not clearly "yes" is taken as no. A misread keystroke must
never count as consent.

Press Ctrl-C at any point to stop after the current step.

## The audit log

Everything the tool checks, finds, attempts, and changes is recorded, and the
log is never pruned:

```bash
outlaw audit
outlaw audit --limit 200
outlaw audit --json
```

It lives in `state.db` alongside your settings. If something goes wrong, this
is the record of what happened -- including which snapshot a change belongs to,
so a change can be traced back to the backup taken before it.

## Privileges

The tool runs as you, not as an administrator. Actions that need more rights --
restarting a system service, for instance -- will fail, and say plainly that
they need rights the process does not have. That is deliberate: a tool that
watches your logs and talks to a network endpoint should not be running with
full system rights all the time.

Run it with elevated rights yourself when you have decided a specific action
warrants it.
