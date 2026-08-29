# Proposal: CritterScript as the terminal's language

**Status: proposed, and the language has now been read.** Nothing here is built.
The reference implementation lives outside this repository, in the FieldKit
project (`core/critterscript.js`, `core/critterscript-net.js`,
`modules/critter-ref.js`, and the two test files beside them). It is at **v3**,
it is about 2,600 lines of JavaScript with roughly 270 checks against it, and
`modules/critter-ref.js` exists specifically so the specification can be lifted
out of that project and made real somewhere else. This is that somewhere else.

*In a subdirectory on purpose: `docs/` is the manual, compiled into the binary,
and a manual must only describe what the program actually does. A proposal is
not documentation.*

---

## What is being asked for

Replace the way the terminal is spoken to. Today it is flags and subcommands in
the ordinary style — `outlaw scan --tier full --explain`. Instead it would be
**CritterScript**.

Two reasons were given, and they are different in kind:

1. **The terminal becomes ours.** Not assembled out of somebody else's argument
   parser, with somebody else's ideas about what a command line is.
2. **It should be simpler** — less jargon, closer to saying what you want than
   to remembering a switch.

The second is the one that decides whether this was worth doing. The first is
achievable by definition, the moment the parser is ours.

An earlier draft of this page read "easier to learn" as "conversational" and
spent a section warning that a conversational front-end guesses what you meant,
which would undo a tool whose whole claim is that it does not guess. That
warning was aimed at the wrong target and is withdrawn. The ask is for **less
jargon**, and CritterScript is not a guessing language — its first rule exists
precisely to delete a category of ambiguity. That is covered below, because it
turns out to be the strongest argument for doing this at all.

## What CritterScript actually is

Three rules, and they are the whole language:

```
1.  $name is a variable. Everything else is a literal.
2.  |  sends the answer along.
3.  -> names the answer.
```

```
list 3 1 10 2 | sort | join ", "          # 1, 2, 3, 10
read notes | split " " | count -> $n
say "there are $n words"
```

Blocks (`if`/`elif`/`else`, `repeat`, `while`, `for`) close with `end`; there
are no braces and indentation is for the reader. `def ... end` makes a command
that behaves exactly like a built-in one, with its own variables that cannot see
or change yours. Brackets run a command and hand its answer back —
`say (upper hello)` — while a bare command prints. Six kinds of value: text,
number, yes-or-no, list, record, nothing. A record is deliberately the shape
`JSON.parse` produces, so parsed JSON needs no conversion step.

### Rule 1 is the reason this fits here

> **`$name` is a variable. Everything else is a literal.**

No quotes around ordinary words, ever. The dollar sign is the only thing that
means "look this up". There is no globbing, no word splitting of a value after
it has been substituted, no "when does this expand", and no shell-quoting rules
to memorise.

That is not merely convenient. This tool refuses to call a skipped check a pass
and refuses to say "you do not use this program" from four numbers. **A command
line that cannot silently reinterpret what you typed is the same commitment one
level down.** A path with a space in it, a Windows path with backslashes, a URL
— each survives being typed plainly, and the FieldKit test suite has probes on
exactly those cases because they are the ones a shell gets wrong.

So the earlier worry was backwards. Ordinary shell syntax is the guessing
surface; rule 1 is what removes it.

## What is actually being swapped

Three separable layers, and the request names one. Worth writing down so the
other two are decisions rather than drift:

| Layer | Today | In scope? |
| --- | --- | --- |
| **How a command is typed** | `clap` flags and subcommands | **Yes.** This is the request. |
| **What you can write in a file and run** | nothing; there is no script format | Yes, and it arrives free. See below. |
| **How runbooks are written** | TOML, in `crates/ork-ai/runbooks/` | Not yet, but it is the natural second home. |

The second is no longer a "probably". CritterScript is a *language*, not an
argument syntax: it has variables, blocks, functions and a `check()` that
validates a whole script before a line of it runs. Adopting it as the command
syntax means the file format exists the same day, because a file is just several
lines of the thing you already type. That is a real gain — the scheduled-task
instructions in `docs/watching.md` currently ask people to wire up a command
line in somebody else's scheduler.

The third is worth flagging early. A runbook is already close to prose — a known
problem, what it looks like, and what has been known to fix it — and the tool
already requires a runbook answer before a probe may ship. That is a large
second project and it should not be smuggled into the first, but the first
should not be designed in a way that forecloses it.

## The decision this whole thing turns on

FieldKit's CritterScript has a rule, written into `critterscript-net.js` and
enforced by a test that fails if it is ever broken:

> **Nothing in that file can change anything.** No restart, no action runner, no
> config write, no delete. The reasoning is not about permissions: a script is
> something you paste from a chat window and run to see what it does, and that
> has to stay a safe thing to do. No confirmation prompt fixes it once the
> command exists to be typed.

**That rule cannot survive the port unchanged, and it is the most important
thing on this page.** Outlaw Repair Kit's terminal exists to change things. It
applies fixes, writes settings, stores credentials, and edits boot
configuration. A language whose safety story is "no command here can do harm"
becomes, here, a language that can.

And the sentence that justifies the FieldKit rule is *more* true here, not less:
a script is a thing somebody pastes from a chat window to see what it does, and
the thing it does on this machine might be to edit a bootloader.

Three ways to answer it, and this needs deciding before any grammar work:

1. **Two registries, and the boundary is a type.** Reading commands and changing
   commands are separate sets, exactly as FieldKit separates them by file, with
   a test on the boundary. A script may be all-reading, and an all-reading
   script is safe to paste — which keeps the property that makes sharing a
   script worth doing.
2. **A changing command may not be piped into or out of.** The safety rails say
   one change at a time, snapshot first, confirm. A pipeline is a sentence that
   composes steps, and a change halfway along a sentence is a change that
   happened while somebody was reading forward. Making `fix` a statement and
   never a pipe stage costs nothing and keeps the rail visible.
3. **Confirmation is part of the language, not part of the command.** `fix`
   asking for a yes is a property of *this* command today; a marker on the
   registration — the way `guestSafe` already is — makes it a property of the
   category, and then a new changing command cannot be added without one.

**Recommended: all three.** They are cheap, they compose, and each of them
protects a rail that already exists in this tool and is currently protected by
nothing but the shape of `clap`.

`guestSafe` is the precedent and it ports directly. FieldKit already carries a
per-command flag, checked in one place (`invoke`) that both the statement form
and the bracket form go through — the comment there says two copies of that
check is how the expression form ends up without it, which is exactly the bug
that turns a read-only mode into a suggestion. This tool has read-only remote
viewing and an elevation broker, and both want the same flag.

## What the terminal is today

Seventeen commands, one flag set, and a plain enum in the middle:

```
scan  watch  watching  stress  models  queue  fix  audit  report
config  set-key  link  boot  probes  processes  host  docs
```

Three flags apply everywhere: `--json`, `--log`, `--no-boot`.

### The good news: the seam already exists

`clap` appears in exactly two places in `ork-cli` — the `derive` on the `Cli`
and `Command` types, and the tests that check the manual against the real
command list. Everything downstream matches on a plain Rust enum:

```rust
async fn dispatch(mut cli: Cli) -> Result<()> {
    let Some(command) = cli.command.take() else { ... };
    match command { ... }
}
```

That enum is the boundary. **Anything that can produce a `Command` value can
drive this tool**, and nothing behind that point knows or cares how the value
was arrived at. So the swap is not "rewrite the terminal" — it is "write a
second producer of an existing type, and then delete the first".

### The bad news: what `clap` was doing for us

Naming this honestly, because "entirely ours" also means "entirely ours to
build". Each of these was checked against the reference implementation rather
than assumed:

- **`--help` for every command**, generated from the same declarations that
  parse, so it cannot drift. CritterScript has the better answer here and it is
  already built: `modules/critter-ref.js` reads the command table **from the
  live registry at render time**, with a selfcheck that fails if a registered
  command is missing from the exported document. That is stronger than what
  `clap` gives us, and it is the same discipline as this repository's existing
  rule that a hand-maintained list next to its own assertion cannot fail.
- **Error messages that suggest.** `outlaw scna` today says *did you mean
  `scan`?* **CritterScript does not have this.** Its error is `no command called
  'scna'. Type help to see the list.` — checked, not assumed. That is a real
  regression and it lands squarely on the second stated goal, since "did you
  mean" is most of what makes a command line usable by somebody who
  half-remembers. It is also not hard: the registry is right there, and edit
  distance over a few dozen names is a short function.
- **Type checking of values** — that `--tier` is one of three words. In
  CritterScript this becomes the command's own job. `minArgs` and `usage` exist
  in the registration; anything narrower is written in `run`.
- **`--version`**, and the conventions that go with it.
- **Shell completion**, generated today and maintained by nobody. This one is
  genuinely lost and would have to be written against the registry.

None of these are hard. All of them are work, and all of them are the sort of
work that is invisible until it is missing.

## Answers to the questions this page used to ask

The previous draft listed eight things the language file needed to tell me. It
has now been read, so they are answered here rather than asked.

1. **Command syntax, script format, or both?** Both, and they are the same
   thing. `run(src, io)` takes a whole source text; one line and a hundred lines
   take the same path. There is no separate interactive grammar, and no session
   state in the language itself — variables live in an environment the host
   owns, so a persistent prompt is a host decision rather than a language
   change.
2. **The grammar.** Line-oriented and block-structured. A tokenizer taking one
   line at a time, an operator table matched longest-first (`<=` before `<`,
   `->` before `-`, `||` before `|`), string literals in either quote with
   interpolation, `#` comments to end of line, and blocks closed by `end`. Small
   enough to port with confidence.
3. **How it says no.** `check(src)` validates structure **before anything
   executes** — the FieldKit terminal refuses to save a script that will not
   parse. Runtime errors carry a line number attached exactly once, not once per
   stack frame. There is no notion of "I understood some of this": it parses or
   it does not.
4. **Is it typed?** Not declared, but values have kinds and `kind` reports them.
   Commands declare `minArgs` and a `usage` string; anything stricter is the
   command's own check. So "did you mean" for *command names* is possible today
   from the registry; "did you mean" for *values* is per-command work.
5. **Values with units.** Arguments are one word each — the most commonly hit
   gotcha, and documented as such: `f $n - 1` passes three arguments, not a
   subtraction; `f ($n - 1)` passes one. Quotes are needed only for a value
   containing a space or an interpolation. **And one that matters a great deal
   here:** a bare token beginning with a digit is a number, so `2026-08-21`
   reads as arithmetic. Dates, versions, device identifiers and sizes want
   quotes. This tool prints all four constantly, and every example in the manual
   will have to be right about it.
6. **Variables, conditions, repetition?** All three, plus `def`/`return` with
   their own scope, `break`/`continue`, and recursion. This is an interpreter,
   not a parser. That is a larger port than an argument grammar — and it is also
   the thing that makes the script format arrive for free.
7. **What a file is called.** Not settled in the reference; FieldKit stores
   scripts in its own vault rather than on a filesystem. **Ours to decide**, and
   the first genuinely open question the port has to answer rather than inherit.
8. **What is already settled.** v3 is settled and exported deliberately. The
   three rules, the pipe's semantics, `end`-closed blocks, the value kinds, the
   registry shape (`name`, `group`, `usage`, `help`, `minArgs`, `guestSafe`,
   `run`) and the budget model are all decided. What is not settled is anything
   about a filesystem, a process, or a machine — because FieldKit has none of
   those, and this tool is almost entirely about them.

## What must not change

These hold whatever else is decided.

- **Nothing the window can do may be unreachable from a script.** The window
  calls the same crates; the terminal must keep reaching all of it.
- **`--json`, or its CritterScript equivalent, stays.** Something reads it.
  Simpler input does not imply less machine-readable output, and machine-
  readable output is a contract.
- **The safety rails.** Snapshot before change, dry run, explicit confirmation,
  one change at a time, no time limits, the audit log. A new way of asking must
  not become a new way of skipping the asking. See the section above; this is
  where the port is most dangerous.
- **No time limits.** CritterScript's budgets are step, output, loop and
  call-depth ceilings — counts, not clocks — so they do not conflict with the
  rule directly. But a step ceiling would be wrong for a script driving a long
  stress test or a watch, and FieldKit's ceilings exist because it runs on a UI
  thread, which is not our situation. **The budgets should be reconsidered from
  first principles rather than ported as numbers**, and the manual cancel has to
  reach a running script.
- **`outlaw` alone still explains itself.** The orientation page is the first
  thing a newcomer sees and it matters more, not less, if the syntax is
  unfamiliar.
- **No check may become harder to reach.**

## The compatibility question

**Everything scripted against the current syntax breaks.** That includes the
install scripts in this repository, the scheduled-task instructions in
`docs/watching.md`, every fenced command in the manual, and whatever anybody has
written for themselves since the first public release.

Three honest answers:

1. **Break it, once, before 1.0, and say so loudly.** The versioning note in the
   changelog already says 1.0 will mean the interfaces have stopped moving —
   which is a promise that they *are still moving* until then. This is the
   cheapest moment this will ever be.
2. **Keep both, with the old one deprecated.** Kinder, and it means two surfaces
   to keep working and to document for as long as the deprecation lasts. The
   manual doubles.
3. **Keep both forever**, CritterScript as the front door and flags as the
   scripting interface. Defensible, and it quietly concedes the second goal: the
   simpler one becomes the toy and the flags stay the real one.

**Recommended: (1), with a stated version and a printed notice.** Announce it in
the release before it happens, and have the old syntax answer for one version
with *"that is the old way of asking; here is the new one"* rather than with an
error. The old parser already exists, so keeping it for one version as an error
message generator costs almost nothing and turns the worst moment of the change
into a lesson.

## Preparation, before any of this starts

All of it is useful whether or not CritterScript lands, which is the test of
whether preparation is real or speculative.

1. ~~**Keep the seam clean, and prove it.**~~ Done — `ork-cli` now has four
   tests that read its own source and require `clap` to appear exactly once,
   in the import that declares the commands, and nowhere else outside test
   code. `dispatch` is checked for still taking a plain `Cli`, and `ArgMatches`
   is banned outright. The first time somebody reaches for `clap::Error` in the
   middle of a command, the build says so.
2. **Give every command a home that is not `main.rs`.** Several are implemented
   inline. A command whose behaviour lives in its own function, taking plain
   values, is a command a second parser can call.
3. **Write down what each command *means*, once.** The descriptions currently
   live in doc comments that `clap` reads. A second front-end needs the same
   text, and two copies would drift. CritterScript's registry has a place for it
   (`help`, `usage`, `group`), and `critter-ref.js` proves the pattern of
   rendering documentation from the registry rather than beside it.
4. ~~**The manual's fenced commands should be checkable.**~~ Done — every fenced
   `outlaw` line in `docs/` is now parsed by the program itself. On the day the
   syntax changes, all of them are wrong at once, and this says so line by line.
5. **Decide where the parser lives.** `ork-cli` is the obvious answer and
   probably the wrong one: if CritterScript ever describes runbooks, `ork-ai`
   needs it too. An `ork-critter` crate that depends on nothing else in the
   workspace keeps that open and keeps the language testable on its own.

## Order of work

1. The rest of the preparation above.
2. Decide the changing-commands question. Nothing else is safe to build first.
3. `ork-critter`: the grammar, a parser, an interpreter, and its refusals. No
   tool behaviour at all — it turns text into a value or into a complaint, and
   it is tested on its own. **Port the FieldKit test suite alongside it**: about
   270 checks exist against the reference implementation, and they are a
   conformance suite for free. A Rust implementation that passes the JavaScript
   one's tests is the same language rather than a similar one.
4. The command registry, and a translation from a parsed call to the existing
   `Command` enum. At this point both front-ends work and neither has been
   removed.
5. The manual, rewritten. Every fenced example, every table.
6. The old syntax becomes a one-version teacher: it recognises the old form and
   prints the new one.
7. Remove `clap`.

Steps 3 and 4 are where this either works or does not, and they are reversible
right up until step 7.

## Still open

- **The changing-commands boundary.** Named above. Decide first.
- **What happens to `--json`.** A flag on a language with no flags is a wart; a
  second way of saying it is a second thing to learn. Possibly the output form
  is part of the sentence — `scan | as json`, which is a pipe stage and needs no
  new concept at all.
- **What a script file is called**, and where one may live. Question 7 above;
  the reference does not answer it because FieldKit has no filesystem.
- **The budgets.** Ported numbers would be wrong; the model may still be right.
- **Whether there is a session.** The language does not decide this — variables
  live in a host-owned environment, so a persistent prompt is available whenever
  it is wanted.
- **Whether the window's screens should say the CritterScript for what they just
  did.** The rule is that nothing the window can do is unreachable from a
  script; showing the sentence makes that concrete and teaches the language to
  the people least likely to read a manual. Attractive, and not free.
- **Runbooks.** Named above. Decide separately, after the terminal.
