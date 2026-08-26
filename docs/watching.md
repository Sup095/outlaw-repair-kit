# Watching for changes

Everything else in this tool answers a question you asked. The watcher is the
part that asks it repeatedly, and speaks up only when the answer changes.

```bash
outlaw watch                 # look every 15 minutes
outlaw watching              # what does it currently remember?
```

## Why it is quiet

A watcher that tells you what it finds is a watcher that tells you the same
eleven things every fifteen minutes. After the second day you stop reading it,
and then it is worse than nothing, because the morning it says twelve things
you do not notice either.

So this one reports **transitions** and nothing else:

| It says something when | It says nothing when |
| --- | --- |
| A problem appears that was not there | Nothing changed |
| A problem gets worse | A problem is still there, unchanged |
| A problem eases | A check ran and found nothing, as usual |
| A problem goes away | A check that was never going to run did not run |

A quiet watcher is a working watcher. If you want to know that it is alive and
what it thinks, ask it: `outlaw watching`.

## The first look reports nothing

A computer that already has six problems did not just develop six problems.
Opening with six alerts is how somebody learns, inside a minute, that these
alerts are not worth reading.

The first look records how the machine is right now, says how many things it
wrote down, and reports no changes. Everything after that is measured against
that starting point.

To start over -- after fixing a batch of things, say, or on a machine you have
just rebuilt -- delete the file whose path `outlaw watching` prints at the top.
The next look records a fresh starting point.

## What it will not do

**It will not clear a problem because a check could not run.** This is the one
that would make a watcher lie, and it is worth being explicit about.

Suppose the system file check cannot run this round -- something else holds the
lock, or the tool it needs was uninstalled. Its findings are missing from that
round's report. Missing looks exactly like fixed. A watcher that announced
"your damaged system files have been repaired" because a check was skipped
would be worse than one that said nothing at all.

So a problem is only ever declared gone by the check that would have found it,
having run to completion and not found it. A skipped, failed, or interrupted
check judges nothing in either direction.

**It will not repeat itself.** Something that comes and goes -- a drive
crossing a threshold as a download finishes, a service that restarts itself --
would otherwise produce an endless alternation of "appeared" and "cleared".
After three round trips it is reported once as flapping, which is the actual
finding and more useful than either half of it, and then held quiet.

Held quiet, never hidden. `outlaw watching` lists everything being held and
why. A watcher with a private list of things it has decided not to mention is
not a watcher anybody should trust.

**It will not fix anything, and it will not ask for administrator rights.**
Watching is watching. Anything found goes on the triage queue in the ordinary
way, to be worked through with [`outlaw fix`](fixing.md) -- with a snapshot, a
dry run, and your explicit confirmation, exactly as if you had run the scan
yourself.

## How often, and how thorough

```bash
outlaw watch --every 60      # hourly instead of every 15 minutes
outlaw watch --tier full     # include the disk health and launch checks
```

Quick is the default tier deliberately. A check heavy enough to be felt is a
check you should ask for, not one that arrives behind your work every quarter
of an hour. The Deep tier reads and hashes most of the operating system; it is
available here, and it is almost certainly not what you want on a timer.

Intervals below one minute are raised to one minute. That is a floor rather
than a limitation: a scan is real work, and running one every few seconds would
make this tool the heaviest thing on the machine, which is a peculiar way to
look after a computer.

There is no time limit on a look, as there is no time limit anywhere in this
tool. Ctrl-C stops it, which ends the round in progress cleanly and writes down
what it learned.

## Running it from a scheduled task

```bash
outlaw watch --once
```

One look, then exit. The operating system's own scheduler decides when; this
decides what changed. It prints nothing when nothing changed and nothing at
start-up, so a scheduler that mails you its output mails you only the rounds
that mattered.

It shares its memory with the running watcher, so you can move a machine
between the two without losing its history or getting a fresh wall of alerts.

**Windows**, every half hour:

```bash
schtasks /create /tn "Outlaw watch" /tr "\"%LOCALAPPDATA%\Programs\outlaw-repair-kit\outlaw.exe\" watch --once" /sc minute /mo 30
```

**Linux**, with systemd -- a timer and a service, as your own user:

```ini
# ~/.config/systemd/user/outlaw-watch.service
[Unit]
Description=Look for changes on this machine

[Service]
Type=oneshot
ExecStart=%h/.local/bin/outlaw watch --once
```

```ini
# ~/.config/systemd/user/outlaw-watch.timer
[Unit]
Description=Look for changes every half hour

[Timer]
OnBootSec=5min
OnUnitActiveSec=30min

[Install]
WantedBy=timers.target
```

```bash
systemctl --user enable --now outlaw-watch.timer
```

As your own user, not as root. A background process that reads your logs and
may talk to a network endpoint has no business holding administrator rights;
the same reasoning is in [Fixing problems safely](fixing.md).

## What it remembers, and where

```bash
outlaw watching
outlaw watching --json
```

A single JSON file beside your configuration, holding one small record per
problem it has ever seen: what it was, which check found it, how bad it was,
whether it is there now, when it first appeared, and how many separate times it
has come back.

JSON, and readable, on purpose. Somebody who wants to know what the watcher
thinks it knows should be able to open the file and read it, and deleting it
should be a complete and obvious reset. If it is ever unreadable, the watcher
starts over rather than refusing to start -- one quiet round is a better
failure than a watcher that will not watch because of a file it wrote itself.

Records are kept for problems that have gone away, which is why the file grows
slowly and never empties. That is what lets the same problem returning next
week be recognised as a return rather than reported as a discovery.
