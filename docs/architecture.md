# Architecture

Five layers over one core. The CLI and the desktop app are both thin clients;
no capability exists in a front-end that is not reachable programmatically.

```
  outlaw (CLI)          desktop app          read-only remote view
        \                    |                        /
         \                   |                       /
          +----------- ork-core / daemon ------------+
                              |
       +----------------+----------------+
       |                |                |
   ork-core          ork-ai          ork-fix
  probes,          router,          triage queue,
  platform,        runbooks,        snapshots,
  orchestration    analysis         fix engine, audit

  Dependencies run one way, towards ork-core. The detection core must keep
  working with no model available and no fixes attempted, so it depends on
  neither of the others and does not link an HTTP client.
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

## 4. Model router

Decision order, each step tried only if the previous is unavailable and the
user has not overridden it:

1. A configured remote endpoint on a private network -- a weaker machine
   offloads AI analysis to a stronger one.
2. A local model sized to detected VRAM.
3. A cloud API.

The user can always force a specific tier, and forcing one means the router will
not fall through to another. That matters for more than convenience: someone who
pinned `local` must never have their diagnostics sent to a cloud provider
because their local server happened to be down.

Because LM Studio, Ollama, and vLLM all speak the OpenAI wire format, "local"
versus "remote" is a base URL rather than a code path.

Every routing decision is recorded and shown. A tool that silently picks a
different model than the user expects -- and silently sends their data somewhere
else -- is worse than one that fails loudly.

Reachability is a real request for the model list, not a socket connect: a port
that accepts connections but has no model loaded is not a usable endpoint. This
is the one place in the tool with a genuine timeout, because a socket waiting on
a server that will never answer is indistinguishable from one waiting on a
server that is thinking.

## 5. AI analysis layer

Input is structured probe output -- never live system access. It correlates
findings across sources, explains them in priority order, and matches symptoms
against a local runbook library *first*, falling back to open-ended reasoning
only when nothing matches.

Three rules hold this together:

* **Runbooks win.** A model answer never overwrites a runbook answer for the
  same finding. Runbook entries are deterministic and have been reviewed; a
  model's opinion does not get to replace one.
* **Invented findings are dropped.** Model answers are matched back to real
  findings by identifier. Anything that does not correspond to a finding the
  probes produced is discarded rather than displayed, because a hallucinated
  problem shown beside real ones would poison the credibility of the report.
* **The model cannot propose a command.** Runbook fixes carry commands, written
  by a person and reviewed. Model suggestions stay prose that a human reads and
  decides on.

Runbooks are TOML rather than YAML. YAML would be the obvious choice for
multi-line prose, but the maintained Rust YAML parsers are in flux, and adding
a second configuration language parsed by an unmaintained crate is a poor trade
for a tool that runs on other people's systems.

The library ships embedded in the binary, so a fresh install has answers
immediately. User entries in the configuration directory are loaded afterwards
and replace built-ins with the same id.

## 6. Fix layer

`crates/ork-fix`. Complex or ambiguous problems go on a triage queue with full
context and are worked one at a time after the scan: snapshot, apply one
candidate, run a test specific to that problem, roll back on failure, try the
next candidate. Successes are recorded per machine so a repeat occurrence
resolves on the first try. There is no time limit on the loop.

The central design decision is that **fixes are a closed set of typed
operations**, not commands. Candidates arrive from two places -- runbooks
written by people, and a model reasoning about a novel problem -- and only the
first has been reviewed. If the executor accepted arbitrary shell, every safety
rule below would be advice rather than a guarantee, and one confidently wrong
model output could destroy a user's data. There is deliberately no variant that
means "run this string", for runbooks or for a model. Anything that cannot be
expressed as a typed operation becomes an instruction for a person.

Three layers of defence, in order: the type system, validation at construction
and again immediately before execution, and execution without a shell (programs
are launched with an argument list, so there is nothing to escape from).

Two rules follow from the brief's "one change at a time, always testable and
reversible":

* A change is only applied when its result **can be tested**. A candidate with
  no verifier is offered as advice rather than applied, because a change nobody
  can measure does not satisfy "always testable".
* A verifier returning "I could not tell" causes a **rollback**, not a shrug.
  Keeping an unverified change would leave the tool having modified a machine
  without being able to say whether it helped.

Safety rails apply everywhere, in every mode: snapshot before any change,
explicit confirmation for anything that modifies the system, one change at a
time, never a destructive operation, and an audit log that is never pruned.

Snapshots come in two kinds and the distinction matters. A **targeted backup**
copies aside exactly the files a fix will touch; it is always available, needs
no privileges, and is what rollback actually uses. A **system-level snapshot**
(restore point, btrfs, Timeshift) is broader but needs administrator rights and
prior setup, so the tool reports whether one appears to exist and never assumes
it does -- claiming a safety net that turns out to be absent is worse than
admitting there is none.

## Privilege model

The core runs unprivileged. Actions needing administrator or root rights go
through a separate elevation step, so a background watcher and a network-facing
interface never hold system rights. `ProbeContext::is_elevated` reports what
rights the current scan actually has, and probes that need more are skipped
with a visible reason rather than failing obscurely.
