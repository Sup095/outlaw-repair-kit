# Reporting a crash or an error

When the tool goes wrong on your computer, the person who could fix it is not
the person who saw it happen. This turns what happened into something you can
hand over.

```bash
outlaw report
```

In the window, it is the **Report a problem** screen.

## What it does

Errors and crashes are written to a small file on your machine as they happen —
nothing is sent anywhere, and nothing needs to be running for this to work. When
you ask for a report, the tool reads that file, takes the personal details out,
and shows you the result.

Then it gives you a link. The link opens GitHub's "new issue" form with the
report already filled in. You read it, change anything you like, and press the
button on that page.

## What it will never do

**It does not post anything.** It holds no credentials for your GitHub account
and never asks for any. It opens a form.

This is the design, not a step on the way to something more convenient. A bug
reporter that can publish for you is a thing that can publish your logs before
you have read them, and no amount of automatic redaction makes that a reasonable
capability for a diagnostic tool to have.

## What gets removed

Before anything is shown to you, the report goes through a redactor. It removes
two kinds of thing.

**Details of this machine**, looked up rather than guessed at:

| Removed | Replaced with |
| --- | --- |
| Your home directory, in any capitalisation | `<home>` |
| Your account name | `<user>` |
| This computer's name | `<machine>` |

**Anything that looks dangerous**, whether or not this machine has seen it
before:

| Removed | Replaced with |
| --- | --- |
| Home directory paths belonging to anyone — `/home/someone`, `C:\Users\Someone` | `<home>` |
| Email addresses | `<email>` |
| Network addresses, keeping the port | `<address>:11434` |
| Anything shaped like a key, token, or hash | `<redacted>` |

The path *inside* a home directory is kept on purpose:
`<home>/.steam/steam.pid` is the diagnostic part, and a report that says a file
failed without saying which file is not worth posting.

Loopback addresses survive too — `127.0.0.1:11434` identifies nobody, and it is
frequently the entire explanation for a local model that is not answering.

### It removes too much on purpose

The pattern rules over-reach. A commit hash, a long identifier, or an
unusually opaque version string will sometimes come out as `<redacted>`.

That is the intended trade. A report with a hash wrongly blanked is a nuisance;
a report carrying somebody's API key onto a public issue tracker cannot be taken
back. When the two conflict, this chooses the nuisance.

### Read it anyway

The tool always shows you the finished text before offering the link, and in
the window you can edit it. Do read it. No redactor is good enough to be
trusted unread, which is precisely why nothing is submitted for you.

## The commands

```bash
outlaw report                    # show what would be posted, and the link
outlaw report --open             # also open the form in a browser
outlaw report --save report.md   # also write it to a file
outlaw report --clear            # forget everything recorded so far
outlaw report --json             # the report as data
```

### When the report is too long for a link

A very long report will not fit in a URL. Rather than quietly cutting the end
off — which is usually the part that matters — the tool writes the report to a
file, tells you where, and gives you a plain link to the blank issue form.
Attach the file, or paste it in.

## You will be told if it crashed

The start-up self-test counts what has been recorded. A crash shows as a
warning there, which is the only notice you get if the tool fell over in the
window, where there is no terminal to have printed anything.

```text
[warn] recorded problems  1 crash(es) recorded -- `outlaw report` turns one into a bug report
```

It is a warning and nothing more — start-up continues normally. Handled errors
are counted and mentioned but never warned about; most of them are a network
hiccup, and warning about those every time would teach you to skip the line
that matters.

## What is recorded, and where

Two things:

- **Errors** — anything the tool logs at error level, anywhere inside it,
  including a command that fails outright. Nothing has to remember to report
  itself.
- **Crashes** — caught by a panic hook, which also still prints the crash to the
  terminal. Somebody watching a crash happen should not have to go looking in a
  file to see what it said.

Both land in `incidents.jsonl` beside your settings, newest last, capped at 200
entries. The oldest go first: the most recent failure is nearly always the one
being chased.

A backtrace is only captured if you asked for one:

```bash
RUST_BACKTRACE=1 outlaw scan
```

Without that, a crash is recorded with its message and location but no frames.
Capturing one on every crash regardless would slow every failure down and fill
the file with frames nobody switched on — but if you can reproduce a crash, a
run with backtraces on makes a much better report.

The record is yours. `outlaw report --clear` empties it, and deleting the file
does the same thing.
