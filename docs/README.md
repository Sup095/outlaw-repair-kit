# Outlaw Repair Kit documentation

*by Outlaw Systems*

A tool that scans a computer for problems, explains what it found in plain
language, and can help fix things -- with strong limits on what it will do to
your machine without asking.

## Start here

| If you want to... | Read |
| --- | --- |
| Install it | [Installing](install.md) |
| Install it and run your first scan | [Getting started](getting-started.md) |
| Know what every command does | [Command reference](commands.md) |
| Use the window instead of the terminal | [The desktop app](desktop.md) |
| Be told when something changes, rather than going to look | [Watching for changes](watching.md) |
| Work the machine hard on purpose to find a fault | [Stress and burn-in](stress.md) |
| See what starts with your computer | [What starts with your computer](startup.md) |
| Have findings explained in plain language | [Setting up a model](ai-setup.md) |
| Borrow a stronger computer's model | [Linking two machines](linking.md) |
| Use a stronger computer's model from a weaker one | [Using another machine](remote-machine.md) |
| Understand what it will and will not change | [Fixing problems safely](fixing.md) |
| Teach it about a problem it does not know | [Writing runbooks](runbooks.md) |
| Work out why something is not working | [Troubleshooting](troubleshooting.md) |
| Report a crash or an error | [Reporting a problem](reporting.md) |
| See how the pieces fit together | [Architecture](architecture.md) |

Work that is planned but not built lives in `proposals/`, separately from this
manual, because a manual must only describe what the program actually does:
[escalation mode](proposals/escalation-mode.md),
[process control](proposals/process-control.md), and
[CritterScript](proposals/critterscript.md), which will change how commands are
typed before 1.0.

## The short version

```bash
outlaw scan             # look for problems
outlaw scan --explain   # ...and explain what they mean
outlaw queue            # what needs working through
outlaw fix              # see what would be done (changes nothing)
outlaw fix --apply      # allow changes, confirming each one
outlaw audit            # everything the tool has done
outlaw watch            # keep looking, speak up only when something changes
outlaw processes        # what is running, and what a sweep would leave alone
```

Every command takes `--json` for scripting.

> **The way commands are typed is going to change** before 1.0, to a language
> written for this project called CritterScript. `--json` output is unaffected.
> See [the proposal](proposals/critterscript.md).

## Three things worth knowing up front

**It does not need an AI to be useful.** The checks are ordinary deterministic
diagnostics. A model is optional, and when one is configured it explains and
correlates results rather than doing the detection.

**Nothing leaves your computer unless you ask.** No telemetry, no phoning home.
If you set up a model, you choose where it runs -- including entirely on your
own machine. See [Setting up a model](ai-setup.md).

**It will not change anything without permission.** `outlaw fix` is a dry run
by default. With `--apply` it still asks before each individual change, takes a
backup first, and undoes the change if it did not help. See
[Fixing problems safely](fixing.md).
