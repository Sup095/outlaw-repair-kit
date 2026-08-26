# Changelog

Every released version, newest first, in terms of what changed for somebody
using the tool rather than in terms of what changed in the code.

Versions are `major.minor.patch`. Until 1.0 the minor number moves when
something is added or changes shape, and the patch number when something that
was already there is fixed. 1.0 will mean the interfaces have stopped moving,
not that the work has stopped.

---

## v0.7.0

**It watches now.** `outlaw watch` in the terminal, or the new **Watching**
screen in the window. It looks on an interval -- a quarter of an hour by
default -- and says something only when something changes: a problem appearing,
getting worse, easing, or going away. Between those it says nothing at all.

That last part is the design rather than an omission. A watcher that reports
what it *finds* reports the same eleven things every fifteen minutes, and
somebody told the same eleven things every fifteen minutes stops reading them
-- including on the morning it is twelve.

Three things follow from that:

- **The first look reports nothing.** A computer that already has six problems
  did not just develop six problems. It records how the machine is now, says
  how many things it wrote down, and measures everything after that against it.
- **A check that could not run clears nothing.** A skipped check reports
  nothing, and reporting nothing looks exactly like reporting a repair. A
  problem is only ever declared gone by the check that would have found it,
  having run and not found it.
- **Something that comes and goes is reported once**, as flapping, and then
  held quiet. Held quiet, never hidden -- what is being held, and why, is
  always listed.

It never fixes anything and never asks for administrator rights. Anything it
finds goes on the triage queue in the ordinary way. `--once` takes a single
look and exits, for a scheduled task, sharing its memory with the running
watcher so a machine can be moved between the two.

**A supervised process could be killed for being busy.** Found while testing:
a process sitting in a tight loop was declared stuck, because the only signal
a silent process had was CPU measured as a *rate* -- and a rate stops being
measurable on a computer that is thrashing, which is the exact condition this
tool runs in. Consumed processor time, which is a counter rather than a rate,
now carries the weight: one millisecond of it is proof a process ran, at any
load. This could have aborted a real repair on a struggling machine.

**The installer opened a blank white window.** On an ordinary Windows desktop
with a current graphics card, the installer drew nothing at all -- no error, no
warning, just a white rectangle where the first thing anybody sees should be.
OpenGL started up perfectly, the window was laid out correctly every frame, and
not one of those frames reached the screen, because overlay software of the kind
that ships with graphics cards hooks the point where a frame is handed over.

It now draws through Direct3D or Vulkan, and falls back to OpenGL by itself if
neither is available. That costs about six megabytes on a download you make
once, which is a poor trade only if you value the six megabytes above the
installer working. `ORK_SETUP_RENDERER` forces either one, for the case nobody
has met yet.

While it was open, the last page was found offering to start the window from a
shortcut whether or not one had been made, and telling people to run `outlaw`
from a new terminal whether or not it had been put on their PATH. It now says
what actually applies.

**Elsewhere:**

- The audit log's timestamp handling now covers the whole tool, so the watcher
  shows times the way the audit log does instead of printing a UTC instant to
  seven decimal places.
- Opening the watcher's memory in Notepad and saving it no longer resets it.
- Documentation going stale is now a build failure: every command must be in
  the command reference and vice versa, the changelog must have an entry for
  the version being built, front-page links must point at files that exist,
  and every page in `docs/` must be one the program carries.

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
