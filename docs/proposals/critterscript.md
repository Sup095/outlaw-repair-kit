# Proposal: CritterScript as the terminal's language

**Status: proposed, and waiting on the language itself.** Nothing here is
built. The CritterScript definition has not been written into this repository
yet; this is the plan for when it is, and the list of things the plan cannot
decide without it.

*In a subdirectory on purpose: `docs/` is the manual, compiled into the binary,
and a manual must only describe what the program actually does. A proposal is
not documentation.*

---

## What is being asked for

Replace the way the terminal is spoken to. Today it is flags and subcommands in
the ordinary style — `outlaw scan --tier full --explain`. Instead it would be
**CritterScript**, a language started for another project (`fieldkit`) and
brought here.

Two reasons were given, and they are different in kind:

1. **The terminal becomes ours.** Not assembled out of somebody else's
   argument parser, with somebody else's ideas about what a command line is.
2. **It should be easier to learn** — closer to saying what you want than to
   remembering a switch.

The second is the one that decides whether this was worth doing. The first is
achievable by definition, the moment the parser is ours; the second has to be
true for somebody who has never used the tool, and that is a question about the
language rather than about this codebase.

## The tension to name first

This tool's whole claim is that it does not guess. It says what it checked and
what it could not; it refuses to call a skipped check a pass; it will not say
"you do not use this program" from four numbers. A command surface that reads
like a conversation is, historically, a command surface that **guesses what you
meant** — and a guessing front-end on a non-guessing tool would undo the thing
that makes it worth having.

So the rule for CritterScript here, whatever its grammar turns out to be:

> It may be **forgiving to read** and it may not be **ambiguous to execute**.
> Where a phrase could mean two things, it says so and asks. It never picks the
> likelier one and proceeds.

A conversation, after all, includes "sorry, which did you mean?". That is not a
failure of a conversational interface — it is what makes it one, rather than a
program silently deciding on your behalf.

## What is actually being swapped

There are three separable layers and the request names one of them. Worth
writing down so that the other two are decisions rather than drift:

| Layer | Today | In scope? |
| --- | --- | --- |
| **How a command is typed** | `clap` flags and subcommands | **Yes.** This is the request. |
| **What you can write in a file and run** | nothing; there is no script format | Probably. See below. |
| **How runbooks are written** | TOML, in `crates/ork-ai/runbooks/` | Not yet, but it is the natural second home. |

The third is worth flagging early. A runbook is already close to prose — a
known problem, what it looks like, and what has been known to fix it. If
CritterScript reads the way it is meant to, runbooks written in it would be
readable by the people who most need to write them, and the tool already
requires a runbook answer before a probe may ship. That is a large second
project and it should not be smuggled into the first, but the first should not
be designed in a way that forecloses it.

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

That is a much smaller and much safer piece of work than it sounds, and it is
worth keeping true. The preparation section below is mostly about not
accidentally losing it.

### The bad news: what `clap` was doing for us

Naming this honestly, because "entirely ours" also means "entirely ours to
build":

- **`--help`, for every command and subcommand**, generated from the same
  declarations that parse the arguments, so it cannot drift out of date.
- **Error messages that suggest.** `outlaw scna` today says *did you mean
  `scan`?* That is not decoration; it is most of what makes a command line
  usable by somebody who half-remembers.
- **Type checking of values** — that `--tier` is one of three words, that
  `--every` is a number — with the error naming the option and the accepted
  values.
- **`--version`**, and the conventions that go with it.
- **Shell completion**, which is generated today and which nobody has to
  maintain.

None of these are hard. All of them are work, and all of them are the sort of
work that is invisible until it is missing. A CritterScript terminal that
cannot say *"did you mean"* is a worse terminal than the current one, whoever
owns the parser, and the second stated goal is precisely about being easier for
a newcomer.

## The compatibility question

The hard one, and it should be decided deliberately rather than discovered.

**Everything scripted against the current syntax breaks.** That includes the
install scripts in this repository, the scheduled-task instructions in
`docs/watching.md`, anything in the manual with a fenced command in it, and
whatever anybody has written for themselves since the first public release.

Three honest answers:

1. **Break it, once, before 1.0 and say so loudly.** The versioning note in the
   changelog already says 1.0 will mean the interfaces have stopped moving —
   which is a promise that they *are still moving* until then. This is the
   cheapest moment this will ever be.
2. **Keep both, with the old one deprecated.** Kinder, and it means two
   surfaces to keep working and to document, for as long as the deprecation
   lasts. The manual doubles.
3. **Keep both forever**, CritterScript as the front door and flags as the
   scripting interface. Defensible, and it quietly concedes the second goal:
   the conversational one becomes the toy and the flags stay the real one.

**Recommended: (1), with a stated version and a printed notice.** Do it before
1.0, announce it in the release before it happens, and have the old syntax
answer for one version with *"that is the old way of asking; here is the new
one"* rather than with an error. A tool that tells you what you should have
typed is not a broken tool; a tool that just fails is.

That last part is the whole of the migration, and it is cheap: the old parser
already exists, so keeping it for one version as an *error message generator*
costs almost nothing and turns the worst moment of the change into a lesson.

## What must not change

These hold whatever the language turns out to look like.

- **Nothing the window can do may be unreachable from a script.** The window
  calls the same crates; the terminal must keep being able to reach all of it.
- **`--json`, or its CritterScript equivalent, stays.** Something reads it.
  Conversational input does not imply conversational output, and machine-
  readable output is a contract.
- **The safety rails.** Snapshot before change, dry run, explicit confirmation,
  one change at a time, no time limits, the audit log. A new way of asking must
  not become a new way of skipping the asking.
- **`outlaw` alone still explains itself.** The orientation page is the first
  thing a newcomer sees and it matters more, not less, if the syntax is
  unfamiliar.
- **No check may become harder to reach.** If a probe can be run today, it can
  be run after.

## What the language file needs to tell me

Written now so the answer can be looked for rather than guessed at.

1. **Is it a command syntax, a script format, or both?** "More like having a
   conversation" suits a session — a conversation has memory, and *"now explain
   the third one"* only means something in one. A one-shot shell invocation has
   no memory. If it is both, the same sentence should mean the same thing typed
   at a prompt, written in a file, and passed as an argument.
2. **The grammar, exactly.** Enough to write a parser that either accepts a
   sentence or refuses it, with no third case.
3. **How it says no.** What a syntax error looks like, and whether the language
   has a notion of "I understood some of this".
4. **Whether it is typed.** Does `quickly` know it is a thoroughness, or is it
   a word the tool interprets? This decides whether *"did you mean"* is
   possible.
5. **How values with units are written** — minutes, shares, counts, paths, and
   paths with spaces in them.
6. **Whether it has variables, conditions, or repetition**, which is the line
   between a command syntax and a programming language, and which decides how
   much of this is a parser and how much is an interpreter.
7. **What it is called in a file** — the extension, and whether a file is a
   sequence of commands or something with structure.
8. **What of it is already settled** and what is still open, so that this does
   not propose changes to decisions already made.

## Preparation, before the file arrives

All of this is useful whether or not CritterScript ever lands, which is the
test of whether preparation is real or speculative.

1. **Keep the seam clean, and prove it.** A test that `ork-cli` uses `clap`
   only in the declaration and in tests — never in dispatch, never in a
   command's implementation. Then the parser can be replaced without touching
   anything behind it. Cheap to write, and it fails loudly the first time
   somebody reaches for `clap::Error` in the middle of a command.
2. **Give every command a home that is not `main.rs`.** Several are
   implemented inline. A command whose behaviour lives in its own function,
   taking plain values, is a command a second parser can call.
3. **Write down what each command *means*, once.** The descriptions currently
   live in doc comments that `clap` reads. A second front-end needs the same
   text, and two copies would drift. This is the same problem already solved
   for the "how long has it been running" format: one table, read by both.
4. **The manual's fenced commands should be checkable.** There are dozens of
   `outlaw ...` examples across `docs/`. When the syntax changes, every one of
   them is wrong, and there is currently nothing that would say so. A test that
   every fenced `outlaw` line in the manual actually parses would catch the lot
   — and it is worth having *today*, before any of this.
5. **Decide where the parser lives.** `ork-cli` is the obvious answer and
   probably the wrong one: if CritterScript ever describes runbooks, `ork-ai`
   needs it too. A `ork-critter` crate that depends on nothing else in the
   workspace keeps that open and keeps the language testable on its own.

Items 1 and 4 are worth doing regardless and are the natural next pieces of
work.

## Order of work

1. The preparation above, particularly the seam test and the manual check.
2. Read the language. Answer the questions in this document, in this document.
3. `ork-critter`: the grammar, a parser, and its refusals. No tool behaviour at
   all — it turns text into a value or into a complaint, and it is tested on
   its own.
4. A translation from that value to the existing `Command` enum. At this point
   both front-ends work and neither has been removed.
5. The manual, rewritten. Every fenced example, every table.
6. The old syntax becomes a one-version teacher: it recognises the old form and
   prints the new one.
7. Remove `clap`.

Steps 3 and 4 are where this either works or does not, and they are reversible
right up until step 7.

## Still open

- **What happens to `--json`.** A flag on a language with no flags is a wart;
  a second way of saying it is a second thing to learn. Possibly the output
  form is part of the sentence.
- **Whether there is a session.** See question 1. A prompt is a large addition
  and might be the thing that makes the second goal true.
- **Whether the window's screens should say the CritterScript for what they
  just did.** The rule is that nothing the window can do is unreachable from a
  script; showing the sentence would make that concrete, and would teach the
  language to the people least likely to read a manual. Attractive, and not
  free.
- **Runbooks.** Named above. Decide separately, after the terminal.
