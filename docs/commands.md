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

> `full` and `deep` currently run the same checks as `quick`. The additional
> checks for those tiers are not built yet; the tiers exist so that probes can
> declare which one they belong to.

---

## `outlaw probes`

List every check this build knows how to run, what it needs, and which
platforms it supports.

---

## `outlaw host`

Show what the tool detected about this machine: operating system, processor,
memory, and every volume with its free space.

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

## `outlaw audit`

Everything the tool has checked, found, attempted, and changed. Newest first,
never pruned.

```bash
outlaw audit
outlaw audit --limit 200
outlaw audit --json
```

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
