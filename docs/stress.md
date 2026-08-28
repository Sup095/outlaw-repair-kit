# Stress and burn-in

Everything else in this tool watches your computer. This is the part that makes
it work.

```bash
outlaw stress                        # ten minutes, all cores, most of the free memory
outlaw stress --minutes 60           # a proper burn-in
outlaw stress --no-cpu               # memory only
outlaw stress --no-memory --minutes 30
```

In the window it is the **Stress** tab. Everything below is true of both.

## What it is for

There is a class of fault that watching cannot find, because the machine never
reports it. Nothing appears in the log. Nothing fails. The computer just gets
*unreliable*:

- A file that was fine yesterday will not open today.
- A game crashes about once a week, never in the same place.
- A build fails, and building again works.
- A photograph has a band of noise through it that was not there when it was
  taken.
- The machine is fast for the first minute and slow after that.

Every one of those is usually blamed on software, and none of them are. The
first four are what memory that corrupts one bit an hour does, or a processor
core that computes wrongly when it is hot. The last one is a cooling system
full of dust. People reinstall the operating system over these, twice, and then
buy a new computer.

This is the test that tells you which it is.

## What it actually does

**The processor.** Every core is given a block of arithmetic with a known
correct answer and asked to do it over and over. The load is not the point --
anything can load a processor. The point is that a core which quietly returns
the wrong number is caught, and caught with the core number attached.

The correct answer is worked out at the start of the run, on the same core that
will then be checked against it. That is deliberate. We are not asking whether
your processor agrees with some reference machine; we are asking whether it
agrees **with itself** — whether the same instructions on the same data give
the same answer this second as last second. A processor that does not is
broken, and no reference is needed to say so.

**The memory.** A share of the free memory is filled with a known pattern and
read back to see whether it is still there. Five patterns are used in turn, and
that matters more than it sounds, because each catches a different physical
fault:

| Pattern | Catches |
| --- | --- |
| All zeros | Cells stuck high |
| All ones | Cells stuck low, and power delivery that cannot hold up under load |
| Checkerboard | Cells disturbed by what is written next to them — the usual failure in modern memory |
| Own address | Faults in the *addressing*: two addresses landing on one physical cell, which every other pattern reads back as perfectly correct |
| Noise | What a regular pattern cannot, because a fault that happens to agree with the pattern is invisible to it |

**The temperature**, the whole time, on every sensor the machine has.

## The rails

- **It stops itself if the machine gets too hot.** Every sensor is read every
  couple of seconds and compared against the temperature *that machine* says is
  critical for that part, with a margin. Where the machine states nothing, a
  conservative 95 °C is assumed.
- **If nothing can be read, it says so** — before it starts, and again in the
  result. A machine with no usable sensors has nothing watching it, and an
  empty temperature list must never be mistaken for a machine that stayed cool.
  On Windows this reading comes from the firmware through WMI and needs
  administrator rights, so running elevated may produce one where an ordinary
  run does not; many desktop motherboards never publish it at all.
- **A gigabyte of memory is always left alone**, however large a share you ask
  for. Taking everything would push the machine into swap, which tests the disk
  instead of the memory, makes the computer unusable while it runs, and on
  Linux invites the kernel to kill something.
- **Nothing is changed and nothing is written.** No settings, no files, no
  registry, no disk. The machine is exactly as it was afterwards, hotter.
- **Stopping is instant.** Ctrl-C, or the button, at any moment. The work is cut
  into pieces a few milliseconds long precisely so that nobody has to wait to
  stop something that is heating their computer.

## It is never part of a scan

`outlaw scan --tier deep` does not run this, and neither does anything else.
You ask for it, every time, on its own.

Choosing "check my computer carefully" is not consent to have that computer
pinned at full load and heated for ten minutes. A tool that treated it as
consent would be doing something to your machine that you did not ask for.

## On the duration

Nothing in this tool is ever cut off for taking too long, and that has not
changed here. The number of minutes is not a deadline on work that would
otherwise continue — it **is** the work. "Load this machine for ten minutes" is
the request. Work already in progress is always allowed to finish, and the
result says how long it actually ran rather than how long it was asked for.

Ten minutes is the default because it is long enough to get a machine properly
hot, which is when marginal hardware misbehaves, and short enough that people
will actually sit through it. An hour is a better test. Overnight is a better
test still, and is what to do when you have a fault that appears once a week.

## Reading the result

**A fault of any kind is a hardware fault.** The machine was asked to repeat
work it had already done and gave a different answer. That is not something
software does and not something this tool can fix. If you see one, stop using
that machine for anything you would mind losing.

**Stopped because it got too hot** is a real result, not a failed test. A
machine that cannot be worked hard without overheating will throttle itself
under any real load — which is what "fast for a minute, then slow" is — and
running hot shortens the life of the parts getting hot. The causes are
physical: dust in the cooling path, a fan that has stopped, thermal paste that
has dried out. Check that the fans are turning and the vents are clear before
anything else.

**Nothing went wrong** means less than you want it to, and the result says so
in as many words. It means: for as long as it ran, every core agreed with
itself and the memory it could reach held what was written to it. It does not
clear the hardware. Faults of this kind are intermittent by nature, and only
the memory the operating system was willing to hand this program could be
tested — anything already in use, including the operating system's own memory,
was out of reach. A clean result narrows where a problem can be hiding. It does
not prove there is not one.

A clean run of under two minutes says even less, and the result says that too.

## When to reach for it

- The computer is unreliable and nothing in a scan explains why.
- You have just built a machine, or changed the memory, or changed a setting in
  the firmware that affects speed or voltage.
- You bought it second-hand and want to know what you bought.
- It gets loud and slow under load and you want to know whether that is normal.
- A shop told you the hardware is fine.

## Options

| Option | What it does |
| --- | --- |
| `--minutes N` | How long to run. Ten by default. |
| `--no-cpu` | Leave the processor alone; test the memory only. |
| `--no-memory` | Leave the memory alone; work the processor only. |
| `--memory-share S` | Share of free memory to test, `0.05` to `0.95`. The reserve is kept whatever this says. |
| `--threads N` | How many cores to work. All of them by default. |
| `--yes` | Start without asking. For scripts and scheduled tasks. |
| `--json` | Machine-readable events and result, one JSON object per line. Needs `--yes` as well: asking for machine-readable output is not the same as agreeing to have the machine heated, and a prompt would break the output anyway. |

## Related

- [Watching for changes](watching.md) — the opposite approach: quiet, repeated,
  and never touching anything.
- [Command reference](commands.md)
- [Troubleshooting](troubleshooting.md)
