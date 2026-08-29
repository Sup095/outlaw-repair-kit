# Getting started

## Install it

Pick whichever of these you would rather do. They all end up with the same
program. [Installing](install.md) covers each in full, including building from
source.

**A window, if you would rather not open a terminal.** Download `outlaw-setup`
(`outlaw-setup.exe` on Windows) from the
[latest release](https://github.com/Sup095/outlaw-repair-kit/releases/latest)
and run it. **It is the only file on that page you need** -- it installs the
command-line tool, the window if you tick it, and a model if you say yes. It
shows you what it is about to do before it does any of it, and writes down what
it did afterwards.

> The release also lists `outlaw-repair-kit-<version>-x64-setup.exe`, which
> installs *only* the window. The one that installs everything is
> `outlaw-setup`.

**One line, if you would.**

```powershell
irm https://raw.githubusercontent.com/Sup095/outlaw-repair-kit/main/install/install.ps1 | iex
```

```sh
curl -fsSL https://raw.githubusercontent.com/Sup095/outlaw-repair-kit/main/install/install.sh | sh
```

Add `-Desktop` on Windows or `--desktop` on Linux to get the window as well.

**By hand.** Download the build for your system, unpack it, and put `outlaw`
somewhere on your `PATH` -- `~/.local/bin` on Linux, any folder you have added
on Windows. It is a single file and needs nothing set up.

Nothing above needs administrator rights, and every download is refused unless
its checksum matches the one published with the release.

## Open it

Whichever way you installed it, there are two ways in and both are on this
page because nothing else tells you.

### The window

| | |
| --- | --- |
| **Windows** | Start menu -> **Outlaw Repair Kit** |
| **Linux** | your applications list -> **Outlaw Repair Kit**, or type `outlaw-repair-kit` |

The setup program installs the window if you tick it. If you installed another
way, or left it unticked, the window is a separate download and will not be in
your Start menu or applications list -- see [Installing](install.md).

### The terminal

Open a terminal and type:

```bash
outlaw
```

That on its own is not an error. It tells you what the tool is and the handful
of commands worth knowing. Everything else is `outlaw --help`.

The installers also add **Outlaw Repair Kit (terminal)** to your Start menu or
applications list, which opens a terminal with the tool ready and *stays open*
so you can read what it says and type the next thing. It runs a small script
called `outlaw-terminal` that sits beside the program; you can read it, and
deleting it removes nothing but the convenience.

> **`outlaw: command not found`?** A terminal that was already open when you
> installed does not have the new `PATH`. Close it and open a new one. If it
> still cannot find it, run the program by its full path once and use
> `outlaw config` to see where things are.

> **Clicked something and nothing happened?** Do not click `outlaw` or
> `outlaw.exe` directly -- it is a command-line program, so it opens a console,
> prints, and closes it again faster than you can read. Use the shortcut, or a
> terminal.

## Your first scan

```bash
outlaw scan
```

A quick scan takes a few seconds and checks disk space, memory pressure,
running processes, device and driver health, whether installed applications
still start, and the system log for crashes and hardware errors.

In the window it is the **Scan** screen, and it is the screen it opens on.

You will get one of two results.

**Nothing found.** That is a real result, not a failure to look. The scan
reports how many checks ran.

**One or more findings.** Each has a severity, a plain-language title, what it
means, and what might fix it.

### Checks that did not run

Some checks need a tool that may not be installed, or need administrator
rights. Those are reported as skipped *with the reason*, rather than silently
passed over:

```
2 check(s) did not run
  Storage health -- `smartctl` is not installed
```

A scan that quietly covered less than you think is worse than one that tells
you, so the tool always tells you.

## You do not need a model

Every check runs without one, and the built-in runbook library explains the
problems people have already written down. A model is for the problems nobody
has written down yet, and the tool says plainly when it has one and when it
does not.

```bash
outlaw models    # what would be used, and why the rest were passed over
```

## Seeing what it can do

```bash
outlaw probes      # every check, what it looks for, what it needs
outlaw host        # what the tool detected about this machine
outlaw processes   # what is running, and what a sweep would leave alone
outlaw docs        # this manual, carried inside the program
```

`outlaw processes` is worth a look on your own machine. On its own it changes
nothing: it shows what is holding the memory, what would be left alone and why,
and what is never touched at all.
It opens grouped **by program**, because a browser is one program however many
processes it is, and each line says how much of that program a sweep would
actually offer -- which is often not all of it. If there is something you never
want it to offer, `outlaw processes --pin <name>` says so once and for good. In
the window it is the **Processes** screen, and the **Leave alone** button on
each row.

When you want to act on that list rather than read it, `outlaw processes
--stop` shows it, asks, and stops what you agreed to -- or **Stop these** in
the window. It asks every time, and there is no way to make it not ask.
**Nothing is put back for you**, so save what is open first.

## Next steps

- Findings you do not understand: [set up a model](ai-setup.md) so they can be
  explained, or read [Writing runbooks](runbooks.md).
- Problems you want help fixing: [Fixing problems safely](fixing.md).
- Being told when something changes, rather than going looking:
  [Watching for changes](watching.md).
- Something not working: [Troubleshooting](troubleshooting.md).

> **One thing to know before you write scripts.** The way commands are typed is
> going to change before 1.0, to a language written for this project called
> CritterScript. `--json` output is not affected, the old way of asking will be
> recognised for a version afterwards and will tell you the new one, and it
> will appear in [the changelog](../CHANGELOG.md) before it happens. See
> [the proposal](proposals/critterscript.md).

## Where your data lives

Nothing is stored outside these locations, and nothing is transmitted anywhere.

| What | Windows | Linux |
| --- | --- | --- |
| Settings | `%APPDATA%\outlaw-repair-kit\config.toml` | `~/.config/outlaw-repair-kit/config.toml` |
| Queue, history, audit log | `...\outlaw-repair-kit\state.db` | `~/.config/outlaw-repair-kit/state.db` |
| Your own runbooks | `...\outlaw-repair-kit\runbooks\` | `~/.config/outlaw-repair-kit/runbooks/` |
| Backups taken before changes | `...\outlaw-repair-kit\snapshots\` | `~/.config/outlaw-repair-kit/snapshots/` |
| What the watcher knows | `...\outlaw-repair-kit\watch-baseline.json` | `~/.config/outlaw-repair-kit/watch-baseline.json` |
| Paired machines | `...\outlaw-repair-kit\peers.json` | `~/.config/outlaw-repair-kit/peers.json` |
| API keys | Windows Credential Manager | Desktop secret service (GNOME Keyring, KWallet) |

`outlaw config` prints this whole list for your machine, and says which of them
exist yet. Deleting any of them is safe -- the tool starts over.
