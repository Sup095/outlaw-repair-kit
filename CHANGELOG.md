# Changelog

Every released version, newest first, in terms of what changed for somebody
using the tool rather than in terms of what changed in the code.

Versions are `major.minor.patch`. Until 1.0 the minor number moves when
something is added or changes shape, and the patch number when something that
was already there is fixed. 1.0 will mean the interfaces have stopped moving,
not that the work has stopped.

---

## Unreleased

**The sweep list no longer offers you the thing you are looking at.**

`outlaw processes` is the list a later "stop everything non-essential" button
will act on, and one of the rules it was supposed to follow was never built:
nothing asked which window was in front of you. Pointed at a real machine
mid-game, it offered the running game. Harmless while nothing can act on the
list, and not something to leave standing before something can.

Windows is now asked directly, and the answer is widened rather than taken
literally. The focused process is held back, and so is everything it is running
inside and everything sharing a name with those. A game started from Steam
protects Steam -- Steam sits there idle holding several hundred megabytes and
looks like an excellent candidate right up until stopping it takes the game
down. A browser is forty processes sharing one name and only one of them owns
the window; stopping the other thirty-nine is stopping the one being read.

**And where it cannot be asked, the list says so.** This is the one rule here
that not knowing does not make safer. Everywhere else an unanswered question
costs a few megabytes that could have been freed; here it means the rule
protected nothing at all. So on a machine where the question cannot be put --
Wayland does not let one program ask what another has in front of you, and
deliberately so -- the list carries **One rule did not run** and says why,
rather than reading like a list that had every rule applied to it.

**The window has a Processes screen.** `outlaw processes` existed only in the
terminal, which made it the one thing the window could not show. Same three
groups, same judgement -- it calls the same `Survey` rather than deciding
again, so the two cannot come to different conclusions about what "held back"
means. It says on the screen that it stops nothing, rather than leaving
somebody to hunt for the button.

**The model that gets asked is one that can answer.** Found by running the
live model test against a real machine rather than a fixture. Ollama lists what
it has alphabetically, the machine had `nomic-embed-text` installed alongside
three perfectly good chat models, and the tool took the first one -- so every
explanation came back as *"nomic-embed-text does not support chat"*. It
degraded honestly, which is right, and it degraded when it did not have to,
which is not. Models whose names mark them as turning text into vectors rather
than sentences are passed over. A name is a guess and not an identification,
so being wrong is kept cheap: it only ever skips one when there is another to
pick, an explicit choice in the settings is never overruled, and a server with
nothing but embedding models on it is still asked -- because "these names look
like embedders" is not the same claim as "you have no models", and only the
server can make the second one.

### Also

- **The setup program looks for the window after installing it**, instead of
  reporting success because the installer it ran exited without complaining.
  Whether there is a window on the machine afterwards is a question with an
  answer, and it is the same question `outlaw` asks when it tells you where to
  find one. It is also the check that would have caught the fault fixed in
  v0.12.0, where the bundle was downloaded, checked, put in the folder,
  announced, and never run. It now says where the window went, which is not
  the folder the rest of it went into.

- **The window's own code is tested now, not only type-checked.** It is a
  third of the product and nothing checked what it did. Two of the new tests
  read the Rust source and compare it against the TypeScript, because that
  join is the one place in this codebase no compiler can see across: a command
  is a string on one side and a function on the other, and a typo in either
  produces a screen that loads and then fails the moment somebody uses it.
  Another pair check that every tab actually renders its own screen -- a tab
  with no branch does not error, it quietly shows a different screen, which is
  worse. And how long a process has been running is formatted twice, once per
  language; both are now checked against one shared table, so whichever one
  moves is the one whose test fails.

---

## v0.12.0

**Four things you actually see, all of them wrong.**

**Console windows stopped appearing.** Almost everything this tool asks the
machine is answered by another program -- PowerShell for the registry and the
event log, `smartctl` for disk health, `ollama` for a model. From a terminal
that is invisible. From a window it is not: Windows gives each one its own
console, so every call flashed a black rectangle onto the screen and took the
focus with it. A scan makes dozens of them. The effect was an application that
appeared to be doing something furtive, and it was worst during an install --
the moment somebody is deciding whether to trust it at all. A test now reads
the source and fails if a new place to start a program forgets the flag; it
found three the first attempt had missed.

**The model install works.** A process gets its environment when it starts and
never sees a change to it, so the installer that had *just* installed Ollama
was the one process on the machine guaranteed not to find it on `PATH` -- and
asking for the model straight afterwards therefore failed every time. It is
looked for where each platform actually puts it. Ollama's own installer no
longer opens a window over ours, winget is no longer allowed to stop and ask
questions on a screen nobody is looking at, and "you already have it" is no
longer reported as a failure. The download reports as it goes, because several
gigabytes arriving behind a window that says nothing is indistinguishable from
a window that has stopped working.

**The setup program installs the window.** It used to download it, put it in
the folder, and say "run it to install the window" -- which left somebody who
had asked for the window with a folder containing an installer and no window
in it. Asking for a thing and being handed the means of getting the thing is
not installing it. It runs it now, and tidies the installer away afterwards.
The release page also says, at the top, which of its eleven files is the one to
download, which it never did.

**The window fits its own minimum size.** At 900 pixels wide -- the width it
will not let you go below -- the last three tabs were not merely cramped but
cut off, with no way to reach them. A tab you cannot click is a screen that
does not exist.

### Also

- **The setup program wears the project's colours** rather than being a grey
  box: the same near-black that leans violet, two crossing grids, a magenta
  bloom low and a cyan one high, the vignette, and the pulse that says it is
  still running during a download when nothing else on screen is changing. The
  values are read out of the window's own stylesheet by a test, so the two
  cannot drift into being nearly the same colour, which looks worse than being
  plainly different.

---

## v0.11.0

**`outlaw processes` -- what is running, and what a sweep would leave alone.**

Stage two of the process-control plan, and it still stops nothing. It shows
three groups: what could be stopped, heaviest first, with what each is holding;
what is held back and why; and what is never touched, counted by reason. The
list exists on its own, before anything can act on it, so that it can be read
on real machines first -- which is the only thing that makes the button
afterwards defensible.

The memory figure says what those programs are **holding**, never what stopping
them would free. Those are different numbers and the second is always smaller,
because memory shared between programs is counted against every one of them.
The honest version can only be produced by measuring afterwards.

### Also

- **Who owns a process is read from that process.** It was one setting for the
  whole machine, which meant one answer for all of it: either every service was
  held back or none was, and neither is a list anybody could act on. On this
  machine the difference is 44 candidates that included Windows services
  against 30 that are all genuinely yours. Where the owner cannot be read at
  all -- the ordinary answer for a service, asked by a program without
  administrator rights -- it is treated as not yours, because not knowing whose
  something is is not the same as knowing it is yours.
- **The window is called the same thing everywhere.** The published `.deb`
  installed `/usr/bin/ork-desktop` and its menu entry ran `ork-desktop`, while
  the documentation said to type `outlaw-repair-kit` and everything looking for
  the window looked for that -- so on a machine that had it installed, the
  documented command did not exist and nothing could find it. The name is now
  chosen explicitly rather than left to default differently on each platform,
  and a test reads it out of the window's own build configuration, so renaming
  it in one place and not the other fails the build.
- **The Linux menu entry has categories.** It shipped with `Categories=` empty,
  which is how an application ends up filed under nothing in an applications
  menu.
- **Programs can be pinned** in the configuration file under `[processes]`,
  matched without regard to capitalisation, and are then never offered.

---

## v0.10.0

**You can open it now.**

Three separate things all pointed the same way: somebody who installed this
tool had no way of opening it that worked.

`outlaw`, typed on its own, answered `error: a subcommand is required` and
exited with a failure. That is a reasonable default for a program whose users
are all already at a prompt, and this one's are not -- somebody has been told
to run a repair tool because their computer is misbehaving, and the first thing
it did was tell them they were wrong. It now says what it is, six things worth
typing, and where the window is.

The shortcut the installer made pointed at the command-line program. On Windows
that opens a console, prints, and closes it faster than anybody can read; on
Linux, in an entry marked `Terminal=false`, it did nothing visible at all. Both
look exactly like a broken program. A shortcut now knows whether it opens a
window or a terminal, and the terminal one goes through a small script that
stays open afterwards -- a file that sits beside the program, can be read, and
can be deleted. The install scripts add one too, which they never did.

And on Linux the installer put the program in `~/.local/share` while adding
`~/.local/bin` to your `PATH`, so `outlaw` answered "command not found" on a
machine where it had just installed correctly. It is linked into the directory
that goes on `PATH` now.

The documentation said there was no installer, which stopped being true three
releases ago, and never said how to open anything. Getting started, installing,
the command reference, troubleshooting and the front page all now say how to
open both halves of the tool, and what to do when a shortcut does nothing.

### Also

- **The Windows build has an icon and says who made it.** It was a generic
  console executable: no icon in a folder, none on a shortcut, and nothing in
  its Properties saying what it was. Embedding one cannot fail the build -- a
  machine without a resource compiler still gets a working program with a
  plain icon.
- **What may be stopped, and what may never be.** The first piece of process
  control: a classifier that stops nothing and answers one question about a
  running process. Protected means never, with a stated reason -- the operating
  system, security software, drivers and their control panels, the display,
  input and audio stack, networking, disk encryption, accessibility, and the
  tool itself. Held back means not by default, and says which restraint
  applied. Candidate means it is offered, with what it is holding, so the
  decision stays with a person. Nothing can stop a process yet.

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

- **Working with no model and no API key is now a tested guarantee**, not an
  observation. It has always been the intended way to run -- the checks find
  problems on their own and the runbook library explains them on their own --
  but nothing stopped that quietly breaking. Five tests now run the whole
  analysis with no model configured and no network reachable: a known problem
  is explained from the runbooks, an unknown one is reported as unexplained
  rather than invented, and a report holding both keeps both.

- The remaining `(s)` counts are gone from the scan, the queue, the machine
  list, and the model screen.

- **The install scripts now refuse a download they cannot check.** A checksum
  that was *wrong* has always been refused. A release publishing no checksum at
  all, or none for the file being fetched, printed a warning and installed it
  anyway -- putting an unverified binary on somebody's PATH on the strength of
  a line they had already scrolled past. The graphical installer had refused
  both cases from the start and the documentation described that behaviour, so
  the scripts were the odd ones out. `--allow-unverified` (`-AllowUnverified`
  on Windows) is there for anyone who has checked it themselves. Nothing turns
  off the refusal of a checksum that is wrong.

- The install scripts and the release workflow are now checked against each
  other. They have to agree on file names -- if they stop agreeing, every
  install that day gets a 404 and nothing in the tool itself is wrong, so
  nothing would have caught it except a stranger.

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
