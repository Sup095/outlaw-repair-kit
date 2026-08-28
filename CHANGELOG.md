# Changelog

Every released version, newest first, in terms of what changed for somebody
using the tool rather than in terms of what changed in the code.

Versions are `major.minor.patch`. Until 1.0 the minor number moves when
something is added or changes shape, and the patch number when something that
was already there is fixed. 1.0 will mean the interfaces have stopped moving,
not that the work has stopped.

---

## v0.9.0

**Eight kinds of problem the tool could find but had nothing to say about.**

Every finding is supposed to arrive with a prepared answer -- what it means,
and what to do -- so that the common problems never need a language model at
all. Eight did not have one. All five things the start-up check reports, and
all three from the deep scan's system-file check, fell through to whatever
model happened to be configured, every single time they occurred. On a machine
with no model set up, they arrived with nothing attached at all.

There was a test meant to prevent exactly this. It could not: it held a
hand-written list of finding ids and checked each had an answer, and a list
written next to the assertion does not grow when a new check is added. It was
green the whole time.

So the list is no longer written by hand. Each check now declares what it can
report, the test reads that from the checks themselves, and adding a new kind
of finding without writing an answer for it now fails the build. Two things
have to be done deliberately to get past it: declare the finding, and either
write the answer or record here that there deliberately is not one -- which is
the honest state for three of them, where a large process might be a memory
leak or a virtual machine and no canned answer fits.

The declarations are checked against what the checks actually report while they
run, so a list that drifts out of date is caught rather than believed.

### Also in this release

- **`--json` no longer skipped the start-up self-test.** It turned the whole
  thing off rather than quietening it, and the self-test is what refuses to
  change anything when the tool cannot vouch for its own snapshot area. So
  `outlaw --json fix --apply` applied fixes with that refusal switched off.
  Asking for machine-readable output is a statement about the output, not about
  which safety checks apply. The checks now run either way and simply print
  nothing, `--no-boot` is the only thing that skips them, and `outlaw --json
  boot` is now actually JSON rather than JSON with a banner drawn across it.

- **The manual is now checked against the program.** Documentation drifts
  silently, and several pages had: the runbook page said eighteen entries when
  thirty ship and its table was missing eleven findings, the command reference
  said six start-up checks when there are seven, and the front page's table of
  what a scan looks at had lost a whole check. Those are all corrected, and the
  counts, the finding table, the check list, and every link between pages are
  now compared against the code by tests. A manual carried inside the program
  for the machine that cannot reach the internet has to be right, because there
  is nowhere else to look.

- **Two tests that would have failed for somebody else.** Found by running the
  suite while the tool's own stress test held every processor core. One asked a
  600-millisecond run to complete more than two blocks of work, which is a
  statement about how fast the machine is; the other gave a busy process two
  seconds to prove it was alive, and the process was starved for 2.8. Neither
  had failed here before, and both would eventually have failed on a shared
  build machine. Nothing about the tool changed, except that the check which
  reports a stalled program now says a saturated machine looks the same from
  the outside.

- **Piping the output into `head` crashed the tool.** `outlaw docs changelog |
  head` was enough: the reader closes the pipe, the next write fails, and the
  program fell over -- then recorded it as a crash and told you it was worth
  reporting. A reader that goes away first is how a pipe is supposed to end.
  The tool now stops writing and exits quietly with status 0. Found by reading
  the Report a Problem screen and noticing the crash sitting on it was one of
  our own making. A genuine failure to write, such as the disk filling up, is
  still recorded exactly as before.

- **The audit log stopped burying real changes under repeated advice.** Every
  run of `outlaw fix` wrote each piece of advice it offered into the permanent
  log, in full, as an `attempt` -- for something that attempted nothing. Since
  `fix` without `--apply` is a preview somebody may run repeatedly while
  deciding, the record of what the tool actually did to a machine was being
  buried under copies of what it had suggested. Advice now has its own heading
  and is recorded once per problem rather than once per run. Anything that
  touched the machine is still recorded every single time, and the log is still
  never pruned or rewritten.

- **Three ways to ask for a test that does nothing, all now refused.**
  `outlaw stress --minutes 0` finished instantly and reported "nothing went
  wrong in 0 seconds", which reads exactly like a clean result to somebody
  scrolling past. `--threads 0` quietly used one core instead. And
  `outlaw audit --limit 0` printed "Nothing has been recorded yet" on a machine
  with a full audit log -- true about the answer, false about the machine, on
  the one screen whose job is to say what the tool has done. The first two are
  refused with a reason; the third gives you a line rather than a lie. The
  window clamped that limit and the terminal did not, so the rule now lives in
  one place and both get it.

- **Counts read like English.** `Last 1 entries`, `1 crash(es) recorded`,
  `13 check(s)`. Small, and on the screens where somebody is deciding whether
  this tool looks like it was made carefully.

- **The queue says when it last actually saw each problem.** It kept a problem
  until somebody worked it, and stated every one of them in the present tense,
  so something found a fortnight ago and quietly resolved since read exactly
  like something the machine has now -- and `outlaw fix` offered to act on it.
  Each item now says when it was first seen and when it was last seen, and
  every scan that finds it again moves that forward. Nothing decides on your
  behalf when an item has gone stale, because there is no honest threshold for
  that; it tells you when it was last seen and leaves the judgement to you.
  Databases from earlier versions gain the column on first open, filled in from
  when each problem was first seen -- not from today, which would make every
  stale item look freshly found.

- **`outlaw probes` says what each check can report**, on a `can report:` line,
  and the same list appears on each check in the **Checks** screen. "What can
  this thing actually tell me" is a fair question to ask of a diagnostic tool
  before running it, and the one-line description only gestured at the answer.

- `probes --json` gains a `reports` field on each check, carrying the same
  list. Nothing was removed or renamed.

- The build now checks the half of itself this project's own development
  machine cannot see. Code reachable only from Windows and from tests compiles
  cleanly on Windows and is dead code on Linux, where warnings are errors --
  which is how the last release went out green locally and red on both CI
  runners. `docs/architecture.md` says what to run.

---

## v0.8.1

**What starts with your computer**, as part of a full scan.

A machine that is slow from the moment you turn it on is usually a machine with
thirty things starting alongside it, most of which arrived attached to
something else and none of which anybody chose. Nobody ever gets told this;
they get told to buy more memory. A full scan now lists them.

The same list answers a second question. Anything that wants to still be
running tomorrow has to be in it somewhere -- so entries pointing at a program
that is not there, entries running out of a temporary or downloads folder, and
commands that arrive encoded rather than written out are each called out. On
Linux, `/etc/ld.so.preload` existing at all is reported, because anything named
there loads into every program on the machine.

Each of those is a statement about what was observed, never a verdict about
what the thing is. A test checks that no finding here contains the words
"malware", "virus", "infected", "trojan", or "rootkit", because a tool that
cannot know must not tell somebody their machine is infected -- that is how a
working computer gets reinstalled over a leftover registry entry.

### The rootkit scan has been withdrawn as a promise

The deep tier has listed "an exhaustive rootkit scan" since the first version.
That is now removed from the roadmap, the tier description, and both
front-ends, rather than quietly satisfied with something weaker.

The reason is not effort. The check would be running on the machine it is
checking, and software with control of the kernel decides what every question
it could ask gets told -- the list of things that start automatically is read
*through* the thing that would be hiding in it. Doing it honestly means
examining the disk from a system that is not the compromised one. A green tick
saying "no rootkits found" would be a confident lie, and the absence of the
feature is better than that.

### Three false accusations, found by running it

All three reported a program sitting exactly where it should be as missing,
which is the worst thing this check could do, and all three are now tested:

- **Shortcuts were cut at the first space.** A path like `...\Start
  Menu\Programs\Startup\Thing.lnk` has no `.exe` in it and spaces throughout,
  so reading the program back out of it produced `C:\Users\...\Start`. Sources
  that state the program exactly -- a shortcut's target, a scheduled task's
  action -- are now believed rather than parsed, and shortcuts are followed so
  what gets checked is the program rather than the shortcut.
- **Quoted paths kept their quotes.** Windows stores a scheduled task's program
  with quotation marks around it whenever the path contains a space, and a path
  with quotation marks in the middle of it is a path that does not exist.
- **A bare program name was called missing.** A scheduled task often names a
  program with no folder and carries a working directory this cannot see. Not
  finding one is now recorded as not knowing, which is not the same thing.

### Also in this release

- **A refusal is no longer filed as a bug.** Every command that failed was
  recorded for the problem reporter, including the ones that failed on purpose.
  "There is no page called that", "say which machine with `--at`", "`--json`
  cannot ask before heating the machine" -- all the tool working correctly, all
  landing in a list headed "what would be posted", until somebody eventually
  posts one as an issue because the program told them it was worth reporting.
  Refusals still stop the command and still exit non-zero, so a script can
  tell. They are simply not faults. Genuine failures are recorded exactly as
  before, and there is a test for each direction, because getting this wrong
  the other way would quietly stop real bugs being recorded.

- **`outlaw stress --json` no longer starts without being told to.** It skipped
  the confirmation, on the reasoning that a prompt would corrupt
  machine-readable output. True, and not a reason to treat "I would like JSON"
  as "I agree to have this machine heated". It now requires `--yes` as well and
  says so.

- **`outlaw watch --once` said nothing at all when nothing had changed.** Right
  for the scheduled task it is meant for -- a log that says "no change" every
  hour is a log nobody reads -- and wrong for a person who has just typed it,
  to whom silence and failure look identical. It now says one line when
  somebody is watching and stays quiet when the output is going anywhere else.
  If a check could not run, it says that instead, because "nothing changed" and
  "nothing changed among the checks that ran" are different claims and only one
  of them would be true.

### Also

- Video memory is reported through the shared size formatter, in the same
  binary units as everywhere else.

## v0.8.0

**Stress and burn-in.** The Deep tier has promised this since the first
version and never delivered it. It is here now -- as `outlaw stress` and as its
own tab in the window -- and it is deliberately not part of any tier.

Everything else in this tool watches your computer. This is the part that makes
it work, because a whole class of fault is invisible to watching. Memory that
corrupts one bit an hour. A processor core that computes wrongly only when it
is hot. A cooling system that was fine when the machine was new and is now full
of dust. None of those appear in a log. They appear as a computer that is
*unreliable*: a file that was fine yesterday will not open, a game crashes
about once a week and never in the same place, a build fails and building again
works. Every one of those gets blamed on software. People reinstall the
operating system over them, twice, and then buy a new computer.

Every core is given a block of arithmetic with a **known correct answer** and
asked to repeat it, so a core that quietly returns the wrong number is caught,
with its number attached. The correct answer is worked out at the start of the
run on the same core that is then checked against it -- because the question is
not whether your processor agrees with some reference machine, it is whether it
agrees with itself.

A share of the free memory is filled and read back under five patterns in turn.
Five, because each catches a different physical fault: cells stuck high, cells
stuck low, cells disturbed by what is written next to them, two addresses
landing on one physical cell, and faults that happen to agree with whatever
regular pattern is being written. A test that only writes zeroes catches almost
nothing.

The rails, because this is the one thing here that acts on the hardware:

- **It watches the temperature and stops itself** if any part of the machine
  reaches the temperature that machine states is critical for it, with a
  margin. Where the machine states nothing, 95 °C is assumed.
- **If nothing can be read it says so** -- before starting and again in the
  result. An empty list of temperatures must never be mistaken for a machine
  that stayed cool. On Windows the reading needs administrator rights, and many
  desktop boards never publish it at all; the message says that too.
- **A gigabyte of memory is always left alone**, whatever share you ask for.
  Taking everything pushes the machine into swap, which tests the disk rather
  than the memory and makes the computer unusable while it runs.
- **Nothing is changed and nothing is written.**
- **Stopping is instant**, and works even if the window is closed or the run is
  abandoned by whatever started it.

And it says what a clean result does *not* mean, because that is the way this
feature could do harm: somebody runs it for ten minutes, reads "nothing went
wrong", and concludes the hardware is fine when the fault they are chasing
happens twice a week.

No scan runs it. Choosing to have your computer checked carefully is not the
same as agreeing to have it pinned at full load and heated, and a tool that
treated one as the other would be doing something to your machine you did not
ask for.

### Also fixed

- **Ctrl-C did not end `outlaw watch`.** The watcher stopped cleanly and wrote
  down what it had learned, and then the process sat there, apparently ignoring
  the interrupt, until it was killed. It was waiting on a channel that could
  never close.
- **A stress run that was abandoned rather than stopped kept going.** Found by
  breaking the overheating rail on purpose to check the test for it would
  notice: the test stopped failing and started hanging. Giving up on a run now
  stops the workers, which matters most in the window, where a closed tab would
  otherwise have left a laptop at full load with nothing watching how hot it
  got.
- **A memory test that did not finish reported as one that passed.** A short
  run over a large region finished no complete pattern and reported "0", next
  to "0 bad", under a heading saying it had finished -- three true numbers
  adding up to the false impression that the memory had been checked. Coverage
  is now counted per pattern read back in full, and a run that completed none
  says so in those words.
- Checkboxes and sliders in the window were still the operating system's blue,
  which was the last thing on screen not in the application's own colours.

---

## v0.7.1

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
