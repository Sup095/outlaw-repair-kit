# Writing runbooks

A runbook is a known problem and its ranked fixes. The tool consults runbooks
*before* it consults a model, because a problem someone has already solved
should not be re-derived every time it happens -- the runbook answer is
instant, free, identical every time, and works with no model at all.

Eighteen entries ship with the tool. You can add your own, and they take
precedence.

## Where yours go

Any `.toml` file in the runbooks directory beside your configuration:

| | Path |
| --- | --- |
| Windows | `%APPDATA%\outlaw-repair-kit\runbooks\` |
| Linux | `~/.config/outlaw-repair-kit/runbooks/` |

Run `outlaw config` to see the exact path. Create the directory if it does not
exist. Files are loaded in alphabetical order after the built-ins.

## The format

```toml
[[entry]]
id = "my-thing.wont-start"
title = "MyThing fails to start after an update"
finding_ids = ["app.launch-failed"]
keywords = ["mything", "libmything.so"]
explanation = """
MyThing links against a library that the last update moved to a new version. \
It fails at startup with a missing-library error rather than anything that \
mentions the update."""

[[entry.fixes]]
description = "Check exactly which libraries are missing."
invasiveness = "inspect"
command = "ldd $(which mything) | grep 'not found'"
platforms = ["linux"]

[[entry.fixes]]
description = "Reinstall the package, which pulls the correct library versions."
invasiveness = "medium"
command = "pacman -S mything"
platforms = ["linux"]
```

### Fields

**`id`** -- unique. Using an id that already exists **replaces** that entry,
which is how you correct a built-in that is wrong for your machine.

**`title`** -- one line, how a person would describe the problem.

**`finding_ids`** -- which findings this answers. Run `outlaw scan --json` and
look at the `id` field of a finding, or see the list below.

**`keywords`** -- optional. When present, at least one must appear in the
finding's title, detail, or captured evidence. This is how several distinct
problems can share one finding id: an application that fails because of a
missing library is a different problem from one that fails because of
permissions, and both are `app.launch-failed`.

Matching is case-insensitive and searches **evidence values too**, which is
usually where the exact error string lives.

**`explanation`** -- what is actually going on, in plain language, for someone
who is not an expert. This is shown directly to the user.

**`fixes`** -- ranked candidates. Order in the file does not matter; they are
sorted by `invasiveness`.

### Fix fields

**`description`** -- what to do. Required.

**`invasiveness`** -- one of:

| Value | Meaning |
| --- | --- |
| `inspect` | Changes nothing; gathers information |
| `low` | Reversible and contained |
| `medium` | Reversible but disruptive -- a restart, a reinstall |
| `high` | Changes system-level state -- drivers, kernels, partitions |

Defaults to `low`. Least invasive is always offered first.

**`command`** -- optional suggested command, shown to the user.

> Commands in runbooks are **displayed, never executed**. The tool does not run
> command strings from text files -- doing so would defeat the entire safety
> model described in [Fixing problems safely](fixing.md). Write them for a
> human reader.

**`platforms`** -- optional. `["linux"]`, `["windows"]`, or omitted for all.
Fixes for other platforms are hidden rather than shown and skipped.

## Which specificity wins

When several entries match one finding, the one with the most keywords wins --
the more specific answer beats the general one. A general entry with no
keywords is a good fallback for anything the specific ones miss.

## Finding ids you can target

| Finding id | Raised when |
| --- | --- |
| `storage.volume-low-on-space` | A drive is running out of room |
| `memory.high-pressure` | Short of memory, or swapping heavily |
| `process.memory-hog` | One process holds most of the machine's memory |
| `process.sustained-high-cpu` | A process has been pinned for a long time |
| `process.zombie-buildup` | Finished processes are not being cleaned up |
| `device.driver-mismatch` | A driver does not match the running system |
| `device.driver-missing` | A device has no driver |
| `device.not-working` | A device is present but will not start |
| `device.reboot-required` | An update is installed but not yet in use |
| `app.launch-failed` | An installed application exits with an error |
| `app.launch-hung` | An installed application hangs on startup |
| `logs.unexpected-shutdown` | The machine stopped without shutting down cleanly |
| `logs.bugcheck` | Windows recorded a blue screen |
| `logs.hardware-error` | The processor reported a machine-check error |
| `logs.display-driver-timeout` | The graphics driver was reset |
| `logs.kernel-panic` | The Linux kernel crashed |
| `logs.gpu-fault` | The graphics card reported a fault |
| `logs.oom-kill` | The kernel killed a process to free memory |
| `logs.storage-error` | A drive reported read or write errors |
| `logs.repeated-error` | An unrecognised error is repeating |

Three of these deliberately have **no** built-in entry:
`process.memory-hog`, `process.sustained-high-cpu`, and `logs.repeated-error`.
They are genuinely ambiguous -- a process holding 40 GB might be a memory leak
or a virtual machine doing its job -- and a canned answer would be worse than
admitting there is not one. These are exactly the cases where a model earns its
place, and good candidates for a runbook of your own if you know what they mean
*on your machine*.

## Checking your entry works

```bash
outlaw models   # reports how many entries loaded
outlaw scan --explain
```

If the count did not go up, the file failed to parse. Run with logging to see
why:

```bash
ORK_LOG=ork_ai=warn outlaw models
```

A malformed file is skipped with a warning rather than breaking the library --
one bad file does not cost you the rest.

## Contributing an entry

If your entry is generally useful rather than specific to your machine, please
open a pull request adding it to `crates/ork-ai/runbooks/built-in.toml`. The
bar is that it should be **conservative and verifiable**: a wrong runbook is
worse than no runbook, because it sends someone confidently in the wrong
direction. Anything speculative belongs in your own directory rather than in
the shipped library.
