# Outlaw Repair Kit

A cross-platform tool that scans a computer for hardware and software problems,
explains what it found in plain language, and can attempt fixes with strong
safety rails.

Most of the detection work is deterministic: the tool wraps mature, established
diagnostic utilities rather than guessing. An AI layer sits on top of that to
correlate findings across sources, explain them, and reason through fixes for
problems that have no known deterministic answer -- it reads structured probe
output, never the live system.

> **Status: early.** The diagnostic core and the CLI exist and work. The model
> router, AI analysis, triage queue, fix layer, and desktop app are not built
> yet. See [Roadmap](#roadmap).

> **Built in collaboration with AI.** This project is developed by a human
> author working together with Claude (Anthropic). Design decisions are made
> jointly and reviewed by a human; a substantial portion of the code is
> AI-written. See [Built in collaboration with AI](#built-in-collaboration-with-ai).

## Install

Download the build for your system from the
[latest release](https://github.com/Sup095/outlaw-repair-kit/releases/latest),
unpack it, and run it. There is no installer and no runtime to install first --
it is a single binary.

**Windows**

```powershell
# after unpacking the .zip
.\outlaw.exe scan
```

**Linux**

```bash
tar xzf outlaw-*-x86_64-unknown-linux-gnu.tar.gz
cd outlaw-*-x86_64-unknown-linux-gnu
./outlaw scan
```

On Linux, install `systemd`'s `journalctl` (present on most distributions) for
the log check to see beyond the current boot. Anything missing is reported as a
skipped check with the reason, never as a silent gap.

## What works today

```bash
outlaw host          # what the tool detected about this machine
outlaw probes        # the checks this build knows how to run
outlaw scan          # run a quick scan
outlaw scan --json   # same, machine-readable
```

Every command accepts `--json`, because nothing in the interface layer is
allowed to do something the core cannot do programmatically.

The Quick tier runs these checks:

| Check | What it catches |
| --- | --- |
| Disk space | Volumes running out of room, judged on both absolute free space and percentage so it misjudges neither a small SSD nor a large array |
| Memory pressure | Being short of memory, and the much worse state of being short of memory *and* swapping heavily |
| Running processes | Processes stuck, leaking memory, or piling up unreaped |
| Device and driver health | Devices the system cannot start, and drivers that no longer match the running kernel -- the cause behind "it broke after I updated" |
| Application launch check | Installed applications that no longer start, or that hang on startup |
| Recent system log errors | Crashes, driver faults, and hardware errors, with repeats grouped together |

Checks that cannot run -- wrong platform, missing tool, elevation not granted --
are reported as skipped with the reason. A scan never quietly covers less than
you think it did.

## Design commitments

These are load-bearing, not aspirational:

- **OS-agnostic by construction.** Everything OS-specific lives behind a
  `Platform` trait, with Windows and Linux implementations from day one. Adding
  macOS is a new implementation, not a rewrite.
- **Deterministic checks are preferred over AI reasoning.** The AI layer
  correlates and explains; it does not replace a check that can be made
  precisely.
- **No time limits.** No scan tier and no individual check is given a deadline.
  Long-running work is supervised by a liveness check -- is this process still
  doing anything? -- and is always manually cancellable, but it is never cut
  off for taking too long.
- **Skipped is visible.** A missing tool or a denied elevation produces a
  reported skip with a reason, never a silent gap in coverage.
- **One broken check does not lose the scan.** A probe that fails is recorded
  and the scan continues.
- **Least privilege.** The core runs unprivileged; anything needing
  administrator or root rights goes through a separate elevation step.

## Building

Requires a stable Rust toolchain. On Windows you also need the MSVC build
tools; on Linux, a working C toolchain.

```bash
cargo build --release
```

The binary lands at `target/release/outlaw`.

```bash
cargo test
```

## Layout

| Path | What it is |
| --- | --- |
| `crates/ork-core` | Diagnostic core: platform layer, probes, scan orchestration |
| `crates/ork-cli` | The `outlaw` command-line front-end |
| `docs/` | Architecture and design notes |

## Roadmap

1. **Diagnostic core, Quick tier** -- done. See the table above.
2. **Model router and AI analysis** -- a remote endpoint over a private
   network, a local model sized to available VRAM, or a cloud API, in that
   order, with a manual override.
3. **Triage queue and fix-attempt loop** -- snapshot, apply one candidate,
   test, roll back on failure, iterate.
4. **Full and Deep tiers, plus a background watcher.**
5. **Desktop application** -- everything configurable from the interface, with
   no file editing required.

## Privacy

The tool collects information about the machine it runs on. Nothing is
transmitted anywhere unless you configure an AI endpoint, and which endpoint is
always your choice, including a fully local one. API keys are stored in the
operating system's credential store, never in a configuration file.

## Built in collaboration with AI

This tool is written by a human author working together with Claude, Anthropic's
AI assistant. That collaboration is stated plainly here because you deserve to
know what you are running:

- Architecture and design decisions are made jointly and reviewed by a human
  before implementation.
- A substantial portion of the source code is AI-written.
- Every change is built, tested, and exercised on real hardware before it is
  committed. Nothing is merged on the strength of "it looks right".

If that matters to how you evaluate a tool that inspects and repairs your
system -- and it reasonably might -- the full history is in the commit log, and
the code is deliberately commented to explain *why* rather than *what*.

## License

MIT. See [LICENSE](LICENSE).
