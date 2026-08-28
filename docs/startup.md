# What starts with your computer

Part of a **full** scan. Nothing to run separately — `outlaw scan --tier full`,
or the Full option in the window, includes it.

Two quite different questions share one list.

The ordinary one, which affects almost everybody: **a computer that is slow
from the moment you turn it on is usually a computer with thirty things
starting alongside it**, most of which arrived attached to something else and
none of which anyone chose. Nobody ever gets told this. They get told to buy
more memory.

The less ordinary one: **anything that wants to still be running tomorrow has
to be in this list somewhere.** That is not a claim that anything in it is
unwanted — almost all of it is a printer utility and a chat program. It is a
statement about where to look.

## What it says something about

| It reports | Why |
| --- | --- |
| How many things start automatically | Only when there are enough to be felt. Not a fault at any number |
| Entries pointing at a program that is not there | An uninstaller did half its job. Harmless clutter |
| Entries running from a temporary or downloads folder | Installed software does not live there |
| Commands that arrive encoded rather than written out | Nothing ordinary needs to hide what it runs |
| `/etc/ld.so.preload` existing at all (Linux) | Anything named there loads into **every** program on the machine |

Each of those is a statement about what was observed. None of them is a verdict
about what the thing is, and the wording keeps that line deliberately — a tool
that cannot know must not tell you your machine is infected. That is how a
working computer gets reinstalled over a leftover registry entry.

## This is not a rootkit scan

Earlier versions of this project listed "an exhaustive rootkit scan" as part of
the deep tier. That promise should never have been made, and it has been
withdrawn rather than quietly satisfied with something weaker.

The reason is not effort. It is that **the check would be running on the
machine it is checking**. Software that has taken control of the operating
system's kernel decides what every question in this file gets told: the list of
things that start automatically is read *through* the thing that would be
hiding in it. Detecting that honestly means examining the disk from a system
that is not the compromised one — a rescue image, a second machine, a read-only
boot — and a program running as an ordinary user on the machine in question
cannot do it.

A green tick saying "no rootkits found" would be a confident lie. The absence
of the feature is better than that, and knowing *why* it is absent is more
useful than either.

What is here instead is deterministic, checkable, and honest about its own
limits — which is the same standard everything else in this tool is held to.

## Where it looks

**Windows**

- The `Run` and `RunOnce` keys, under both the machine and the current user,
  including the 32-bit view
- Both Startup folders — yours and the one shared by everybody. Shortcuts are
  followed, so what gets checked is the program rather than the shortcut
- Scheduled tasks that trigger at logon or at boot

**Linux**

- `~/.config/autostart` and `/etc/xdg/autostart`
- User systemd units in `~/.config/systemd/user`
- `/etc/ld.so.preload`

## What it deliberately leaves out

**On Windows, the several hundred scheduled tasks under `\Microsoft\`.** They
come with the operating system, they are all normal, and they would bury
everything else. A list nobody reads is worse than no list.

**On Linux, system-wide systemd units.** Almost every one was put there by a
package, the package manager already knows about them, and listing four hundred
would drown the handful someone installed by hand. User units — which by
definition are not package-managed — are included.

**Anything needing administrator rights.** Everything above is readable by the
person whose machine it is. A check that only runs for administrators is a
check most people never see.

## Getting it wrong is the thing to avoid

Two kinds of mistake matter here, and they are not equal.

Missing something is a missed opportunity. **Accusing a program that is sitting
exactly where it should be is worse**, because the person believes it, and acts
on it. Three specific ways that happened while this was being built, all now
fixed and all now tested:

- A shortcut path like `...\Start Menu\Programs\Startup\Thing.lnk` was cut at
  the first space and reported as a missing program. Sources that state the
  program exactly are now believed instead of parsed.
- Windows stores a scheduled task's program with quotation marks around it when
  the path has a space. Kept, those quotes make a path that cannot exist, and
  two programs sitting where they should be were reported as missing.
- A bare program name with no folder was reported as missing when it may simply
  live somewhere this cannot see. Not finding one is now recorded as not
  knowing, which is not the same thing.

## Related

- [Command reference](commands.md)
- [Stress and burn-in](stress.md) — the other half of "the machine itself is
  the problem"
- [Watching for changes](watching.md) — something appearing in this list later
  is exactly the sort of change the watcher reports
