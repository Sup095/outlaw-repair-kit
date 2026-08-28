# Proposal: escalation mode

**Status: proposed, not built.** Reviewed for safety before it exists, not
bolted on afterwards. Nothing here is in the tool.

---

## What it is for

The ordinary tool is deliberately timid. It runs unprivileged, it refuses to
make a change it cannot test, it rolls back anything it cannot verify, and it
hands anything it is unsure about to a person. That is right for the common
case, which is a machine with an ordinary problem.

It is not enough for two situations:

- **Severe cases.** The machine will not boot properly, or something is
  actively wrong and the safe checks cannot see it because they are not allowed
  to look.
- **In-depth debugging.** Somebody is actually trying to find out what is
  happening, and needs the tool to gather far more than it normally would.

Escalation mode is the tool being told: *stop being careful about looking, and
be careful about acting instead.*

## The distinction that makes it safe

There are two separate things people mean by "escalate", and conflating them is
how this becomes dangerous:

| | Looking harder | Acting harder |
| --- | --- | --- |
| What it does | Reads more, at greater depth, with more rights | Makes changes the ordinary tool refuses to make |
| Worst case | A large file of information about your machine | A machine in a worse state than it started |
| Reversible | Nothing to reverse | Sometimes, at best |

**Proposal: build the first, and treat the second as a separate decision
requiring its own review.** Most of what "severe case" and "in-depth debugging"
actually need is the first one. A tool that gathers everything and explains it
clearly, while still refusing to act, is genuinely useful in a crisis and cannot
make the crisis worse.

## Level 1 — Deep gather

Reading, at a depth the ordinary scan does not go to. Changes nothing.

- Full event and system logs over a long window, not the recent slice.
- Complete driver and module inventory with versions and load order.
- Boot configuration, recent boot history, and how each one ended.
- Full disk health attributes rather than the drive's own summary verdict.
- Filesystem and volume state, including anything dirty or pending repair.
- Crash dumps present on the machine — **listed and described, never read into
  a report**, because a memory dump contains whatever was in memory.
- The complete process and service tree with their relationships.
- Scheduled work, all of it, not just the non-Microsoft entries.
- Network configuration and what holds each open connection.

**Requires elevation**, and says which parts it could not reach without it
rather than silently gathering less.

**The output is a bundle, and the bundle is the product.** Written to one file,
with the same redaction the problem reporter already applies — home directory
paths, account and machine names, addresses, anything shaped like a key. It is
shown before it goes anywhere, and it goes nowhere on its own.

This is the piece that would help most in a severe case, and it cannot damage
anything.

## Level 2 — Deep act

Changes the ordinary engine will not make. **This needs its own review; it is
described here so the boundary is written down, not because it is agreed.**

The candidates, and what makes each hard:

| Action | Why it is wanted | Why it is dangerous |
| --- | --- | --- |
| Reinstall or roll back a driver | The commonest cause of "it broke after an update" | A failed graphics driver install is a machine with no display |
| Repair system files | The deep scan already *finds* this and can only advise | The repair tools take a long time and can fail part-way |
| Reset a network stack | Fixes a real and common class of fault | Removes configuration somebody may have set deliberately |
| Repair boot configuration | The machine does not start; nothing else matters | Getting it wrong means the machine does not start at all |
| Clear a filesystem's pending-repair state | Unblocks a stuck volume | Filesystem repair on a failing disk finishes the disk off |

Rules if it is ever built:

- **A machine-level restore point first**, not just the tool's own file
  snapshot, and the action is refused outright if one cannot be made.
- **One action, then stop.** Not a sequence. The person decides whether to
  continue after seeing the result.
- **Never on a drive that reports itself failing.** The disk-health check
  already knows. Repairing a dying disk is the single most reliable way to
  finish it off, and the tool already says so in its own runbook.
- **Never without the exact command shown first**, in full, before it runs.
- Every one of these is a typed operation in the closed set. There is no
  variant that means "run this string", and escalation mode does not add one.

## How it is entered

Deliberately, per run, and it announces itself.

- A flag, `--escalate`, and a switch in the window. Never a setting that stays
  on: a mode you can forget you are in is a mode that surprises you.
- It states what changes and what still applies, before anything runs.
- The whole session is marked in the audit log, at the beginning and the end,
  so the record shows exactly which work happened under it.
- No time limit, because nothing here has one. It ends when the run ends.

## What never changes, in any mode

This is the important list. Escalation raises what the tool may look at and
argues about what it may do. It does not touch any of this:

- **No arbitrary commands.** Not from a runbook, not from a model, not from a
  person via a text box. The closed set of typed operations is the reason every
  other safety rule is a guarantee rather than a hope.
- **Nothing is sent anywhere.** A deep-gather bundle is written to a file and
  shown to you. It is not uploaded, and there is no mode in which it is.
- **A model never gains the ability to act.** It explains. Escalation gives it
  more to read and no more to do.
- **Confirmation is still per action.** Escalation mode is not blanket consent;
  it is consent to be *asked* about a wider set of things.
- **The audit log is still never pruned.**
- **Nothing destructive.** No formatting, no partitioning, no deleting user
  data, ever, under any flag.

## Where it would live

- A field on `ProbeContext`, so a probe can gather more when told to and still
  declares what it needs. Probes stay the unit.
- `ork-core/src/gather/` for the bundle, reusing the redaction the problem
  reporter already has rather than growing a second one that drifts from it.
- New typed actions in the existing closed set for level 2, if it is ever
  agreed. No new execution path.
- `outlaw gather` for level 1 — usable on its own, without the rest of
  escalation mode — and `--escalate` on `scan` and `fix`.

## Order of work

1. **`outlaw gather`**, level 1, no elevation. Useful immediately, cannot hurt.
2. **Elevated gather**, once the elevation broker exists, saying plainly what it
   could not reach without it.
3. **The bundle's redaction, tested hard.** It contains far more than a problem
   report, so the thing that takes personal details out of it matters more.
4. **Level 2, one action at a time**, each with its own review. Driver rollback
   is the obvious first, because it is the commonest severe cause and Windows
   already provides a supported way to do it.

Stage 1 alone would cover most of what "severe case" and "in-depth debugging"
actually need.

## Still open

- **Does level 2 belong in this tool at all?** A defensible answer is no: be
  the best possible instrument, and let a person act on what it shows. Worth
  deciding deliberately rather than drifting into.
- **Elevation is a prerequisite** for most of level 1 and all of level 2. The
  broker is separate work and comes first.
- **How much is too much in a bundle?** A deep gather on a busy machine could be
  very large. It should say how big it is and let you leave parts out.
