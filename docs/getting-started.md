# Getting started

## Install

Download the build for your system from the
[latest release](https://github.com/Sup095/outlaw-repair-kit/releases/latest)
and unpack it. There is no installer and nothing to set up first -- it is a
single file.

**Windows**

```powershell
.\outlaw.exe scan
```

**Linux**

```bash
tar xzf outlaw-*-x86_64-unknown-linux-gnu.tar.gz
cd outlaw-*-x86_64-unknown-linux-gnu
./outlaw scan
```

To run it from anywhere, put the binary somewhere on your `PATH` -- for example
`~/.local/bin` on Linux.

### Building from source

You need a stable Rust toolchain. On Windows you also need the Microsoft C++
build tools; on Linux, a working C compiler.

```bash
git clone https://github.com/Sup095/outlaw-repair-kit
cd outlaw-repair-kit
cargo build --release
```

The binary appears at `target/release/outlaw`.

## Your first scan

```bash
outlaw scan
```

A quick scan takes a few seconds and checks disk space, memory pressure,
running processes, device and driver health, whether installed applications
still start, and the system log for crashes and hardware errors.

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

## Seeing what it can do

```bash
outlaw probes   # every check, what it looks for, what it needs
outlaw host     # what the tool detected about this machine
```

## Next steps

- Findings you do not understand: [set up a model](ai-setup.md) so they can be
  explained, or read [Writing runbooks](runbooks.md).
- Problems you want help fixing: [Fixing problems safely](fixing.md).
- Something not working: [Troubleshooting](troubleshooting.md).

## Where your data lives

Nothing is stored outside these locations, and nothing is transmitted anywhere.

| What | Windows | Linux |
| --- | --- | --- |
| Settings | `%APPDATA%\outlaw-repair-kit\config.toml` | `~/.config/outlaw-repair-kit/config.toml` |
| Queue, history, audit log | `...\outlaw-repair-kit\state.db` | `~/.config/outlaw-repair-kit/state.db` |
| Your own runbooks | `...\outlaw-repair-kit\runbooks\` | `~/.config/outlaw-repair-kit/runbooks/` |
| Backups taken before changes | `...\outlaw-repair-kit\snapshots\` | `~/.config/outlaw-repair-kit/snapshots/` |
| API keys | Windows Credential Manager | Desktop secret service (GNOME Keyring, KWallet) |

`outlaw config` prints these paths for your machine.
