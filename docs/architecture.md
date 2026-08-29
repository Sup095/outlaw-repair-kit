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

Three front-ends, all thin, all talking to the same core.

`crates/ork-cli` is the command line. `apps/desktop` is the window. Every
human-readable output has a `--json` equivalent, and nothing the window can do
is unreachable from a script -- which is why shared content, right down to the
manual in `crates/ork-core/src/docs.rs`, lives in the core rather than in
whichever front-end happened to need it first.

`crates/ork-setup` is the graphical installer, and is the exception that
proves the rule: it is a separate program that installs the others, so it
depends on the core only for what it genuinely shares -- hardware detection,
running a command -- and draws itself without a system webview, because an
installer cannot have prerequisites.

### Where the terminal's parser stops and the tool begins

`clap` appears in `ork-cli` in exactly two places: the `derive` on the `Cli`
and `Command` types, and the tests that check the manual against the real
command list. Everything past that point matches on a plain Rust enum and has
no idea how the value was arrived at.

That boundary is load-bearing rather than incidental. **Anything that can
produce a `Command` value can drive this tool**, which is what makes replacing
the way commands are typed a contained change rather than a rewrite -- see
[the CritterScript proposal](proposals/critterscript.md), which is that
replacement. Reaching for `clap` inside a command's implementation would quietly
weld the parser to the tool, so it is worth not doing even while `clap` is what
is there.

That is now checked rather than remembered. Four tests read the crate's own
source and require that the program names `clap` exactly once -- the import that
declares the commands -- and nowhere else outside test code, which may use it
freely. A `clap::Error` returned from the middle of a command would compile,
pass clippy, look unremarkable in review, and cost nothing until somebody tried
to move the parser; this fails the build the day it is written.

The same boundary has a second half, and it is easier to miss. **A command
hands back what it decided; only `main` ends the process.** `boot` saying the
machine is not ready and `scan` finding something serious are answers, and they
used to be delivered by calling `std::process::exit` where they stood. A
command that stops the world cannot be one line of a script with more lines
after it, and cannot be called by the window at all — so the exit codes are
computed by the commands and applied in exactly one place, and a test says so.

The same shape holds across the join to the window: a Tauri command is a thin
wrapper that calls into the crates and returns what they returned. Two tests
read the Rust source and compare it against the TypeScript, because a command is
a string on one side and a function on the other and no compiler can look across
that.

## 2. Diagnostic core (non-AI)

`crates/ork-core`. This does most of the real work.

A **probe** is one deterministic check. It declares up front which platforms it
supports, which external tools it needs, whether it requires elevation, and
every problem it is able to report. The orchestrator gates on the first three
and records a *visible skip with a reason* rather than running a probe that
cannot work. Probes emit `Finding`s: a stable slug, a severity, plain-language
text, and structured evidence.

The last of those declarations exists because of a question nothing could
otherwise ask: *is there a prepared answer for everything this tool can say?*
Finding ids are string literals scattered through a probe's body, so a test
that wants the full set has to restate it -- and a hand-written list next to an
assertion cannot fail when a probe grows a new finding. That is not
hypothetical. It is how the start-up check shipped five findings with a runbook
answer for none of them, past a test written to catch exactly that. The list is
now read from the probes, and the orchestrator checks each declaration against
what actually comes back, so a list that drifts is caught rather than believed.
`outlaw probes` prints it, because "what can this thing tell me" is a fair
question to ask before running a diagnostic.

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

## 7. The watcher

`crates/ork-core/src/watch.rs`. A scan on an interval, plus the one thing that
turns a repeated scan into a watcher: a comparison against what was true last
time. It reports transitions and nothing else, because a watcher that reports
what it finds reports the same things every quarter of an hour, and a person
told the same things every quarter of an hour stops reading them.

Findings are matched across looks by `Finding::occurrence_key` -- the finding's
id and its subject -- which already existed for the triage queue, so "this
exact problem on this exact thing" means the same thing to both.

Two rules do the real work, and both are about what *not* to say.

* **Only a check that ran may clear anything.** A skipped, failed, or
  cancelled probe contributes nothing to a comparison -- neither new problems
  nor the absence of old ones. This is the rule that stops the watcher lying:
  a check that could not run reports nothing, reporting nothing is
  indistinguishable from reporting a repair, and "your damaged system files
  have been fixed" because something held a lock would be worse than silence.
  Kept deliberately separate from the question of which non-runs are worth
  *mentioning*, which is a presentation decision and has its own test.
* **A problem that comes and goes is announced once.** After three round trips
  it is reported as flapping and then held quiet -- but recorded in `muted`
  with a reason, and shown by both front-ends, because a watcher with a
  private list of things it has decided not to mention is not one anybody
  should trust.

State is a single JSON file beside the configuration, holding one small record
per problem ever seen, including problems that have gone away -- which is what
lets the same problem returning next week be a return rather than a discovery.
Readable on purpose: somebody who wants to know what the watcher thinks it
knows should be able to open it, and deleting it should be an obvious and
complete reset. An unreadable file starts over rather than refusing to watch,
because one quiet round is a better failure than a watcher that will not run
on account of a file it wrote itself.

It never fixes and never elevates. Findings reach the queue by the ordinary
route.

## Privilege model

The core runs unprivileged. Actions needing administrator or root rights go
through a separate elevation step, so a background watcher and a network-facing
interface never hold system rights. `ProbeContext::is_elevated` reports what
rights the current scan actually has, and probes that need more are skipped
with a visible reason rather than failing obscurely.

## Checking a change before it is pushed

The build is denied warnings on both Windows and Linux, and a good deal of the
platform layer is behind `#[cfg]`. That combination has a trap in it: code
reachable only from `#[cfg(windows)]` and `#[cfg(test)]` compiles cleanly on a
Windows machine and is *dead code* in the Linux library build, so a clean local
run says nothing about the other half of the matrix.

Running clippy the way the workflow runs it is therefore not enough on its own:

```
cargo fmt --all -- --check
cargo clippy --all-targets --all-features
cargo test --workspace --all-features
```

The window is a third of the product and none of the above touches it:

```
cd apps/desktop && npm run check && npm test
```

Some tests need the network, or a running model server, and are skipped by
default. They are the ones that check this tool against the world rather than
against a fixture, which is why they have found faults the fixtures could not
-- a model server offering only an embedding model, a release publishing a
bundle under a name nothing looked for. One of them checks the platform rather
than the world: whether consumed processor time really is a counter that climbs
while a process runs, which is what stops the liveness check killing a busy
program on a loaded machine. Worth running before a release, and worth running
on any platform this is new on:

```
cargo test --workspace -- --ignored
```

**Build the window with `tauri build`, never with `cargo build` alone.** Plain
`cargo build --release -p ork-desktop` produces a binary that expects the
development server and opens on *"can't reach this page"*; the front-end is only
compiled in by the Tauri build. The window opens and looks fine, which is what
makes it worth writing down.

`ork-core` holds nearly all of the conditional code, and it cross-checks
without a C toolchain, so the fourth command closes the gap:

```
rustup target add x86_64-unknown-linux-gnu
cargo clippy --target x86_64-unknown-linux-gnu -p ork-core --all-targets --all-features
```

The other crates pull in C dependencies -- SQLite, dbus, aws-lc -- whose build
scripts need a cross-compiler, so those are left to the workflow. Anything
compiled only on one platform and in tests wants an explicit
`#[cfg(any(windows, test))]` (or the `target_os = "linux"` equivalent) rather
than being left to fall out of whatever the local machine happens to be.
