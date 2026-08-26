# Troubleshooting

## Seeing what is happening

Almost every question here is answered faster with logging on:

```bash
ORK_LOG=debug outlaw scan
ORK_LOG=ork_ai=debug outlaw models      # just the model layer
ORK_LOG=ork_fix=debug outlaw fix        # just the fix layer
```

Logs go to standard error, so they will not corrupt `--json` output.

---

## The scan says checks did not run

That is the tool being honest rather than a fault. Each skipped check names its
reason:

| Reason | What to do |
| --- | --- |
| `` `x` is not installed `` | Install it if you want that check. On Linux, `smartmontools` provides `smartctl`. |
| `needs administrator rights` | Re-run elevated if you want that check. Most do not need it. |
| `not supported on <platform>` | Nothing to do; that check does not apply here. |
| `only runs in a full scan` | Use `--tier full` or `--tier deep`. |

## The scan found nothing

Also a real result. `outlaw probes` shows what was actually looked at. If you
have a specific problem the tool did not spot, that is worth
[opening an issue](https://github.com/Sup095/outlaw-repair-kit/issues) --
missing detection is the most useful kind of bug report.

## The log check finds nothing on Linux

The tool prefers `journalctl` and falls back to `dmesg`. If neither works:

```bash
journalctl --priority=4 --since -72h    # does this work as your user?
```

If it says you lack permission, add yourself to the `systemd-journal` group and
log out and back in. If `dmesg` is the fallback in use, it only covers the
current boot -- so a crash from yesterday will not appear. `dmesg` may also be
restricted to root by `kernel.dmesg_restrict`.

## The log check is slow on Windows

It queries the Event Log through PowerShell, which takes a second or two to
start. Normal.

---

## No model is being used

```bash
outlaw models
```

This tells you precisely why. Common answers:

**"turned off in settings"** -- that tier is disabled. Remote and cloud are off
by default; see [Setting up a model](ai-setup.md).

**"did not answer"** -- the address was tried and nothing responded. Check the
server is running and listening where you think:

```bash
curl http://127.0.0.1:11434/v1/models
```

**"no model loaded"** -- the server answered but has nothing loaded. In LM
Studio, load a model. With Ollama, `ollama pull` something first.

**"no credential"** -- the cloud tier is on but no API key is stored. Run
`outlaw set-key cloud`.

**"skipped because a different tier is pinned"** -- you set `mode` to something
other than `auto`. That is working as intended: pinning stops fallback, so your
diagnostics cannot end up somewhere you did not choose.

## The other machine cannot be reached

Work through it from the machine that is failing to connect:

1. **Is the server listening beyond localhost?** By default most are not. See
   [Using another machine](remote-machine.md), step 1.
2. **Can you reach it at all?** `curl http://ADDRESS:PORT/v1/models` from the
   machine running the scan. If this fails, the problem is networking, not this
   tool.
3. **Firewall.** On Linux, `sudo ufw allow 11434/tcp` or the firewalld
   equivalent. On Windows, allow the port for the network profile you are on --
   a mesh network adapter is often classified as Public, where most things are
   blocked by default.
4. **Right address?** On a mesh network, use the address that network assigned,
   not the LAN one.
5. **Timeout too short?** The default reachability check is 2 seconds. Over a
   slow link, raise it:

   ```toml
   [ai]
   reachability_timeout_ms = 5000
   ```

## The model gives poor explanations

Usually the model is too small for the job. `outlaw models` recommends a size
for your hardware. Under about 7B parameters, expect vague answers and
occasional failures to return usable structured output.

If the model returns nothing usable, the tool keeps the runbook answers and
says the model could not be consulted -- you lose the explanation, not the
scan.

## The model answer looks wrong

Report it. Also worth knowing: model answers never overwrite runbook answers,
so if a problem has a known answer you are seeing the reviewed one. Anything
labelled "reasoned by ..." came from the model and should be read as a
suggestion, not a diagnosis.

You can override any built-in answer with your own -- see
[Writing runbooks](runbooks.md).

---

## `outlaw fix` says everything needs a person

Expected, currently. The tool only applies a change automatically when it can
*test* whether the change worked, and it can only carry out a small set of
typed operations. Everything else is presented as instructions.

[Fixing problems safely](fixing.md) explains why the list is short.

## A fix failed with a permissions error

The tool runs as you, not as an administrator. Restarting a system service
generally needs more rights. Run it elevated yourself if you have decided that
specific action is warranted:

```bash
sudo outlaw fix --apply          # Linux
```

On Windows, run the terminal as Administrator first.

## Undoing something the tool did

Every change is preceded by a backup, and a failed change is rolled back
automatically. If you want to undo a change that succeeded:

```bash
outlaw audit --limit 100
```

Find the entry, then look in the snapshots directory (`outlaw config` prints
the path). Each snapshot directory is named for the attempt it belongs to and
contains the original files.

## The installer window is blank

It should not be. If it happens, the installer can be told to draw a different
way:

```bash
ORK_SETUP_RENDERER=gl outlaw-setup
```

on Linux, or in a Windows terminal:

```bash
set ORK_SETUP_RENDERER=gl && outlaw-setup.exe
```

`wgpu` forces the other direction. The installer normally draws through
Direct3D or Vulkan and falls back to OpenGL on its own if neither is there, so
this should never be necessary -- but a blank window is a dead end, and one
environment variable is a better answer than none.

The reason it carries two ways of drawing at all: the OpenGL path was found
rendering a blank white window on an ordinary Windows desktop with a current
graphics card. OpenGL started up without complaint and every frame was drawn
correctly -- none of them reached the screen, because overlay software of the
kind that ships with graphics cards hooks the point where a frame is handed
over. If neither renderer works, please
[report it](reporting.md); that is worth knowing about.

If you cannot get the window to draw at all, the shell installers in `install/`
do the same work with no window involved.

## Where is the state kept?

```bash
outlaw config
```

Prints every path. Deleting `state.db` clears the queue, the history, and the
audit log; nothing else is affected.

---

## Reporting a bug

```bash
outlaw report --open
```

That builds a report from the errors and crashes recorded on your machine,
shows you exactly what it would post with personal details already removed, and
opens GitHub's issue form with it filled in. Nothing is sent until you press the
button on that page yourself. In the window, it is the **Report a problem**
screen.

If you can make the problem happen again, do it with backtraces on first -- a
crash with frames in it is worth several without:

```bash
RUST_BACKTRACE=1 outlaw scan
outlaw report --open
```

For something the tool did not notice as an error -- a wrong answer, a missing
check, a confusing message -- `outlaw report` still gives you a form with the
version and machine details filled in, and you can describe the rest.

These are worth attaching to almost any report:

```bash
outlaw host --json
outlaw probes --json
```

A full debug log is more than a report normally needs, but if one is asked for:

```bash
ORK_LOG=debug outlaw scan 2> scan-log.txt
```

Read `scan-log.txt` before attaching it. Unlike `outlaw report`, **it is not
redacted** -- it contains hostnames, drive layouts, and log excerpts from your
machine exactly as they were.

See [Reporting a problem](reporting.md) for what the redactor removes and what
it deliberately leaves alone.

Issues: <https://github.com/Sup095/outlaw-repair-kit/issues>
