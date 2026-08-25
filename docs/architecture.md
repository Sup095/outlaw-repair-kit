# Architecture

Five layers over one core. The CLI and the desktop app are both thin clients;
no capability exists in a front-end that is not reachable programmatically.

```
  outlaw (CLI)          desktop app          read-only remote view
        \                    |                        /
         \                   |                       /
          +----------- ork-core / daemon ------------+
                              |
     +----------+-------------+-------------+----------+
     |          |             |             |          |
  probes    model router  AI analysis   fix layer   platform
```

## 1. Interface layer

`crates/ork-cli` today; a desktop application later. Both talk to the same
core. Every human-readable output has a `--json` equivalent.

## 2. Diagnostic core (non-AI)

`crates/ork-core`. This does most of the real work.

A **probe** is one deterministic check. It declares up front which platforms it
supports, which external tools it needs, and whether it requires elevation. The
orchestrator gates on those declarations and records a *visible skip with a
reason* rather than running a probe that cannot work. Probes emit `Finding`s:
a stable slug, a severity, plain-language text, and structured evidence.

Probes run one at a time on purpose. Several of them measure CPU, I/O, and
memory pressure; running them concurrently would have them measure each other.

**Scan tiers** are `Quick`, `Full`, and `Deep`, ordered. A probe declares the
lowest tier it participates in. None of them has a time limit.

## 3. Platform layer

`crates/ork-core/src/platform`. The seam for everything OS-specific. Probes
never call an OS API or shell out directly -- they go through the `Platform`
trait.

Where a maintained cross-platform crate already does the OS-specific work
correctly, both implementations delegate to `platform::common`. The seam stays;
the duplication does not. Where the platforms genuinely differ -- the Windows
Event Log versus journald, `mhwd` versus Windows driver enumeration, System
Restore versus btrfs snapshots -- each implementation does its own thing.

`PlatformKind::MacOs` exists as a variant with no implementation, so probes can
already declare support and the compiler will point at everything that needs
attention when it lands.

## 4. Model router (not built)

Decision order, each step tried only if the previous is unavailable and the
user has not overridden it:

1. A configured remote endpoint on a private network -- a weaker machine
   offloads AI analysis to a stronger one.
2. A local model sized to detected VRAM.
3. A cloud API.

The user can always force a specific tier. Because LM Studio, Ollama, and
vLLM all speak the OpenAI wire format, "local" versus "remote" is a base URL
rather than a code path.

## 5. AI analysis layer (not built)

Input is structured probe output -- never live system access. It correlates
findings across sources, explains them in priority order, and matches symptoms
against a local runbook library *first*, falling back to open-ended reasoning
only when nothing matches.

## 6. Fix layer (not built)

Simple deterministic issues are fixed inline during the scan. Complex or
ambiguous ones go on a triage queue with full context and are worked one at a
time after the scan: snapshot, apply one candidate, run a test specific to that
issue, roll back on failure, try the next candidate. Successes are recorded per
machine so a repeat occurrence resolves on the first try.

Safety rails apply everywhere, in every mode: snapshot before any change, a
dry-run diff before anything non-trivial, explicit confirmation for anything
system-level, one change at a time, never an automatic destructive operation,
and a full audit log.

## Privilege model

The core runs unprivileged. Actions needing administrator or root rights go
through a separate elevation step, so a background watcher and a network-facing
interface never hold system rights. `ProbeContext::is_elevated` reports what
rights the current scan actually has, and probes that need more are skipped
with a visible reason rather than failing obscurely.
