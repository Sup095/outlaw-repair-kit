# Changelog

Every released version, newest first, in terms of what changed for somebody
using the tool rather than in terms of what changed in the code.

Versions are `major.minor.patch`. Until 1.0 the minor number moves when
something is added or changes shape, and the patch number when something that
was already there is fixed. 1.0 will mean the interfaces have stopped moving,
not that the work has stopped.

---

## v0.6.0

**An installer you can double-click.** Download `outlaw-setup.exe` on Windows
or `outlaw-setup` on Linux and run it. It asks which version you want, shows a
list of exactly what it is about to do to your computer, does it, and then
shows what it did. It refuses -- not warns -- to install any file whose
checksum does not match the one published with the release. It never asks for
administrator rights. It offers to set up a model sized for the graphics card
it finds, and says how many gigabytes that means before you agree.

It carries no copy of the tool inside it, so it stays a quick download however
large the tool becomes. The shell install scripts still work and are not going
anywhere.

**The manual is inside the program.** `outlaw docs` in the terminal, or the new
**Info** screen in the window. Not linked to -- carried, because a machine that
has gone wrong is often one that cannot reach the internet, and the pages most
likely to be needed are the ones least likely to be reachable when they are
needed. The Info screen also shows the version, what this build detected about
the machine, whether an update exists, and the licence.

**The Deep tier does something.** It ran exactly what a full scan ran, and both
front-ends said so. It now verifies that the operating system's own files still
match what installed them: `sfc /verifyonly` on Windows, and whichever of
`pacman`, `rpm` or `debsums` the distribution uses on Linux. It reads and
hashes most of what is installed, so it takes minutes to an hour -- and there is
no time limit on it, only the Stop button.

It never repairs. `/verifyonly` is used in preference to `/scannow` on purpose:
putting a different file over a system file is a system-level change, and those
go through the queue with your confirmation on them. An interrupted check is
never reported as a pass.

**The desktop application now starts.** It had never been built and run as a
real application, only its front-end in a browser against a stub. Built for the
first time, it drew its start-up screen and then sat at nothing for ever: it
had no capability file, so the window was refused permission to listen for
events and never asked for start-up to run. Nobody would have got past the
splash screen. Fixed, along with the reason it failed silently rather than
saying so.

**Fixes found by using the window:**

- A scan's results were thrown away by switching to another screen and back. A
  deep scan can take an hour. So was a bug report you had started writing.
- The audit log printed timestamps exactly as stored, to seven decimal places,
  and repeated the word "queued" beside a column already saying "queued".
- Skipped checks were listed with the tag the reason is keyed by --
  `"unsupported-platform"`, quotes included -- rather than the sentence that
  explains it. Checks belonging to another operating system are no longer
  listed at all, which is what the command line has always done.
- Pressing **Save settings** showed its confirmation at the top of a form
  longer than the window, which looked exactly like nothing happening.

**Elsewhere:**

- Scans now know whether they are running with administrator rights instead of
  assuming they are not, so a check that needs them is no longer skipped when
  they were already granted.
- The **Checks** screen lists every check this build knows how to run, grouped
  by tier, and says which ones cannot run here and why.
- Releases publish the program on its own as well as inside an archive, and
  publish the installer for both platforms.

---

## v0.5.1

Published desktop bundles under names that exist. Tauri names bundles after the
product, which has a space in it, and GitHub rewrites spaces when publishing an
asset -- so the name in `SHA256SUMS` was not the name anyone could download.

---

## v0.5.0

**Linking two machines.** Pair two computers with a code so one can lend the
other a model, over your own network, with no account and no relay. A link
carries inference and a read-only view of what was found. It carries no ability
to change anything at the other end, by design.

Liveness supervision was corrected never to poll faster than processor use can
actually be measured -- below that interval a process pinned at full tilt reads
as perfectly idle, and the supervisor concludes that the busiest thing on the
machine is stuck.

---

## v0.4.0

**The triage queue and the fix engine.** Problems that are not safe to fix
during a scan go on a queue, worst first, and are worked one at a time: a
snapshot before anything changes, a dry run you can read, your explicit
confirmation, and a rollback if the check that follows fails.

Full documentation.

---

## v0.3.0

**The model router, the runbook library, and AI analysis.** Findings can be
explained in plain language. Known problems are answered from runbooks with no
model involved at all; a model is only consulted for problems that are not in
the library. Routing prefers, in order, another machine you own, a model on this
one, and a hosted model -- and the hosted tier is off until you turn it on.

---

## v0.2.0

**The Quick tier, complete.** Running processes, device and driver health, and
launch checks for installed programs.

---

## v0.1.0

The diagnostic core, the command line, and the first checks: disk space, memory
pressure, and system log correlation.
