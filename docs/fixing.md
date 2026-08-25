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
instead of being applied. This is why a lot of what the tool suggests is
currently advice rather than action.

**"I could not tell" is treated as failure.** If the test cannot be carried
out, the change is rolled back rather than kept. Keeping it would mean the tool
had modified your computer and could not say whether that helped -- which is
exactly the state this loop exists to avoid.

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
