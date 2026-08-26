# Command reference

Every command accepts these:

| Option | Meaning |
| --- | --- |
| `--json` | Machine-readable output instead of human-readable |
| `--log <level>` | `error`, `warn`, `info`, `debug`, `trace` (default `warn`) |

The `ORK_LOG` environment variable overrides `--log` and takes per-module
filters, e.g. `ORK_LOG=ork_ai=debug`. Logs go to standard error, so `--json`
output on standard out stays clean for piping.

---

## `outlaw scan`

Look for problems.

```bash
outlaw scan
outlaw scan --tier full
outlaw scan --explain
outlaw scan --json
```

| Option | Meaning |
| --- | --- |
| `--tier`, `-t` | `quick` (default), `full`, or `deep` |
| `--explain` | Also explain the findings -- see [Setting up a model](ai-setup.md) |

No tier has a time limit. Press Ctrl-C to stop; the scan finishes the current
check and reports what it has.

Findings needing investigation are added to the [triage queue](fixing.md)
automatically.

**Exit codes:** `0` nothing serious, `2` at least one high or critical finding,
`1` the scan itself failed. Useful in scheduled tasks.

> `full` adds two things. **Disk health** asks every drive whether it considers
> itself healthy -- cheap, but not something a quick look-around should be
> telling you in passing. And the **application launch test** starts catalogued
> applications such as Steam and closes them again, which is the reason it is
> not part of a quick scan.
>
> On Linux, disk health needs `smartmontools` installed and root, because
> talking to a drive means talking to the device node. Without both, the check
> is skipped **with the reason shown** rather than quietly returning nothing --
> a scan that could not look at your disks must not read as a clean bill of
> health.
>
> `deep` adds the system file check: it verifies that the operating system's own
> files still match what installed them. That means reading and hashing most of
> what is installed, so it takes minutes to an hour, and there is no time limit
> on it -- press Ctrl-C to stop. On Windows it needs administrator rights, and
> is skipped with that reason shown when it does not have them. The stress and
> burn-in tests that tier is also meant for are not built yet.

---

## `outlaw probes`

List every check this build knows how to run, what it needs, and which
platforms it supports.

---

## `outlaw host`

Show what the tool detected about this machine: operating system, processor,
memory, and every volume with its free space.

---

## `outlaw docs [page]`

Read the manual. Every page is compiled into the program, so this works on a
machine that cannot reach the internet -- which is a machine this tool expects
to be run on.

```bash
outlaw docs               # list the pages
outlaw docs commands      # print one
outlaw docs fixing | less
```

Printed as the Markdown it is written in, deliberately: that is the form that
survives being piped, grepped, or pasted into an issue. The window shows the
same pages, rendered, on its **Info** screen.

---

## `outlaw models`

Show which model would be used and **why each tier was or was not chosen** --
including whether an endpoint answered, and whether a credential is stored.

Also reports graphics hardware, a recommended model size for it, and how many
runbook entries are loaded.

See [Setting up a model](ai-setup.md).

---

## `outlaw config`

Show where settings live, what they currently say, and which credentials are
stored. Values of credentials are never printed.

Everything has a working default, so the file may not exist yet -- that is
normal.

---

## `outlaw set-key <which>`

Store a credential in the operating system's credential store.

```bash
outlaw set-key cloud
outlaw set-key remote
outlaw set-key cloud --remove
```

| Argument | What it is |
| --- | --- |
| `cloud` | API key for the hosted model provider |
| `remote` | Bearer token for a remote endpoint that requires one |

The value is read from standard input, not taken as an argument, so it does not
land in your shell history or the process list.

---

## `outlaw queue`

Show problems waiting to be worked, worst first, with how many attempts each
has had and what state it is in.

---

## `outlaw fix`

Work through the triage queue.

```bash
outlaw fix           # dry run -- changes nothing
outlaw fix --apply   # allow changes, confirming each one
```

| Option | Meaning |
| --- | --- |
| `--apply` | Permit changes. Each one is still confirmed individually. |

Without `--apply` this is a **dry run**: it reports what it would do and
changes nothing.

Read [Fixing problems safely](fixing.md) before using `--apply`. It explains
what the tool will and will not do, and why the list is short.

Press Ctrl-C to stop after the current step.

---

## `outlaw link`

Lend a model to another machine, or borrow one. See
[linking.md](linking.md) for the whole picture.

```bash
outlaw link                    # what this machine is linked to
outlaw link host               # lend this machine's model
outlaw link join               # pair with a machine showing a code
outlaw link find               # who on this network is lending
outlaw link check              # ask each link whether it still answers
outlaw link view [<name>]      # what is wrong with a linked machine
outlaw link remove <name>      # cut a link and forget its token
```

| Option | Applies to | What it does |
| --- | --- | --- |
| `--port <n>` | `host`, `join`, `find` | Use a port other than 7341 |
| `--model-url <url>` | `host` | Lend a specific model instead of the first configured one |
| `--no-discovery` | `host` | Do not answer discovery on the local network |
| `--at <address>` | `join` | Skip discovery and pair with that address |

A linked machine can be asked to think, and to say what its last scan found.
Nothing in the link can change the machine at the other end.

## `outlaw boot`

Run the start-up screen on its own: six self-checks and an update check. Exits
non-zero if a check failed.

```bash
outlaw boot
outlaw boot --json
```

Start-up runs automatically before `scan` and `fix`. Skip it with `--no-boot`,
which is also implied by `--json`.

The update check reports and never installs. See [desktop.md](desktop.md) for
what each check covers.

## `outlaw audit`

Everything the tool has checked, found, attempted, and changed. Newest first,
never pruned.

```bash
outlaw audit
outlaw audit --limit 200
outlaw audit --json
```

---

## `outlaw report`

Turn a crash or an error into a bug report you can post.

```bash
outlaw report                    # show what would be posted, and the link
outlaw report --open             # also open the form in a browser
outlaw report --save report.md   # also write it to a file
outlaw report --clear            # forget everything recorded so far
outlaw report --json
```

Shows the finished report first, with personal details already removed, and
then gives you a link that opens GitHub's issue form with it filled in. **It
never posts anything** — you press the button on that page yourself. See
[Reporting a problem](reporting.md) for exactly what is removed and why.

---

## Scripting

Every command has a `--json` form, because nothing in the interface is allowed
to do something that is not reachable programmatically.

```bash
# Anything high or critical?
outlaw scan --json | jq '[.outcomes[].findings[]
  | select(.severity == "high" or .severity == "critical")] | length'

# What did not run, and why?
outlaw scan --json | jq -r '.outcomes[]
  | select(.status.status == "skipped")
  | "\(.name): \(.status.reason)"'

# Exit code in a scheduled task
outlaw scan --json > /var/log/outlaw-$(date +%F).json || echo "problems found"
```
