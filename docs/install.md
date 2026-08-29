# Installing

**If you want one answer: download the setup program and run it.** It installs
everything else, and nothing on this page is needed afterwards.

| Your system | The file |
| --- | --- |
| Windows | `outlaw-setup.exe` |
| Linux | `outlaw-setup` (mark it executable: `chmod +x outlaw-setup`) |

The rest of this page is the other ways in, which exist for people who want
them. They all end up with the same program.

> **The setup program is not the same file as the window's own installer.**
> A release also publishes `outlaw-repair-kit-<version>-x64-setup.exe`, which
> installs *only* the window and nothing else. If you are not sure which you
> have, the one you want is the one called `outlaw-setup`.

## The setup program

The one to use if you would rather not open a terminal at all. Download it from
the [latest release](https://github.com/Sup095/outlaw-repair-kit/releases/latest)
and run it:

| Your system | The file |
| --- | --- |
| Windows | `outlaw-setup.exe` |
| Linux | `outlaw-setup` (mark it executable: `chmod +x outlaw-setup`) |

It is a small window and a quick download. It carries no copy of the tool
inside it — it asks GitHub what has been released, you pick a version, and it
fetches that. So it stays small however large the thing it installs becomes.

**It installs everything you tick, including the window.** The window ships as
its own installer, and this downloads it, checks it, runs it, and then tidies
the installer away. You do not have to run anything afterwards.

Before it does anything it shows you a list of exactly what it is about to do
to your computer. Afterwards it shows you what it did, and writes the same list
to `install-receipt.json` beside the installed files, so removing this later is
reading a list rather than guessing.

It puts `outlaw` on your PATH, adds **Outlaw Repair Kit (terminal)** to your
Start menu or applications list, and — on Linux, where an AppImage has no
installer of its own — adds **Outlaw Repair Kit** for the window too. All of
that is listed on the page before it starts and in the receipt afterwards.

It **refuses** — not warns — to install any file whose checksum does not match
the one published with the release, or any file the release published no
checksum for. It never asks for administrator rights. It offers to set up a
model sized for whatever graphics card it finds, tells you how many gigabytes
that means before you agree, and shows the exact command it would run to
install anything that is not its own business.

> It needs a release from **v0.6.0** onwards. Earlier releases packaged the
> program inside an archive and did not publish it on its own, and this
> installer downloads exactly one file and checks it against exactly one
> published checksum rather than carrying an archive unpacker around. Told to
> use an older release, it says so and says which to pick instead.

## The install script

The same work, in a terminal, for anyone who prefers it.

**Windows** — in PowerShell:

```powershell
irm https://raw.githubusercontent.com/Sup095/outlaw-repair-kit/main/install/install.ps1 | iex
```

**Linux** — in a terminal:

```sh
curl -fsSL https://raw.githubusercontent.com/Sup095/outlaw-repair-kit/main/install/install.sh | sh
```

The installer:

- works out which published build fits your machine,
- downloads it and **refuses to install it unless the checksum matches** the
  one published with the release -- a mismatch is refused outright, and so is
  being unable to check at all, because not knowing is not the same as knowing
  it is fine and this is the step that decides what ends up on your PATH,
- puts `outlaw` somewhere on your PATH that needs no administrator rights
  (`%LOCALAPPDATA%\Programs\OutlawRepairKit` on Windows, `~/.local/bin` on
  Linux),
- adds **Outlaw Repair Kit (terminal)** to your Start menu or applications
  list, unless you say not to,
- asks whether you also want a model running on this machine, and
- tells you exactly what it is about to run before running anything that is not
  its own business.

It never installs anything else silently, and it never needs administrator
rights unless you ask it for the desktop app.

### Options

| Windows | Linux | What it does |
| --- | --- | --- |
| `-Version v0.4.0` | `--version v0.4.0` | Install a specific release |
| `-Dir <path>` | `--dir <path>` | Install somewhere else |
| `-Desktop` | `--desktop` | Also install the desktop app |
| `-LocalModel` | `--local-model` | Set up a local model without asking |
| `-NoLocalModel` | `--no-local-model` | Skip the local-model question |
| `-Yes` | `--yes` | Do not ask anything; take the safe default each time |
| `-NoShortcut` | `--no-shortcut` | Do not add anything to the Start menu or applications list |
| `-AllowUnverified` | `--allow-unverified` | Install even when the download cannot be checked against a published checksum. Refused without this. Nothing turns off the refusal of a checksum that is *wrong* |

To pass options to the one-line install, download the script first and run it:

```powershell
irm https://raw.githubusercontent.com/Sup095/outlaw-repair-kit/main/install/install.ps1 -OutFile install.ps1
.\install.ps1 -Desktop -LocalModel
```

## The window on its own

Only needed if you did *not* use the setup program, or if you want the window
on a machine that already has the command-line tool.


Download the installer for your system from the
[releases page](https://github.com/Sup095/outlaw-repair-kit/releases):

| System | File |
| --- | --- |
| Windows | `outlaw-repair-kit-<version>-x64-setup.exe`, or the `.msi` |
| Linux | `outlaw-repair-kit-<version>-amd64.AppImage`, or the `.deb` |

The window includes everything the command line does.

The install *scripts* can fetch it for you with `-Desktop` on Windows or
`--desktop` on Linux. On Linux that installs the AppImage into the same
user-owned directory as the program, so it still needs no root — run it with
`outlaw-repair-kit`. An AppImage needs FUSE; the script says so if it cannot
find it.

## Opening it once it is installed

| | The window | A terminal |
| --- | --- | --- |
| **Windows** | Start menu → **Outlaw Repair Kit** | Start menu → **Outlaw Repair Kit (terminal)**, or type `outlaw` |
| **Linux** | applications list → **Outlaw Repair Kit**, or `outlaw-repair-kit` | applications list → **Outlaw Repair Kit (terminal)**, or type `outlaw` |

Typing `outlaw` on its own is not an error. It says what the tool is and the
handful of commands worth knowing.

**Do not click `outlaw.exe` itself.** It is a command-line program: clicking it
opens a console, prints, and closes it again faster than anybody can read,
which looks exactly like something crashing. That is why there is a shortcut
rather than a link straight to the program. The shortcut runs a small script,
`outlaw-terminal`, that sits beside the program, opens it at a prompt, and
stays there. You can read that file, and deleting it removes nothing but the
convenience.

On Windows the desktop app's own installer adds its Start menu entry, so this
installer does not add a second one pointing at the same thing.

## From source

You need [Rust](https://rustup.rs) 1.85 or newer.

```bash
git clone https://github.com/Sup095/outlaw-repair-kit
cd outlaw-repair-kit
cargo build --release
```

The program lands in `target/release/outlaw`. To build the desktop app as well
you also need Node 20+, and on Linux the webview development packages:

```bash
sudo apt install libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf
cd apps/desktop && npm install && npm run tauri build
```

## About the optional local model

**The tool does not need a model.** Every deterministic check runs without one,
and the runbook library explains problems people have already written down. A
model only helps with the problems nobody has written down yet.

If you say yes, the installer sets up [Ollama](https://ollama.com) and offers a
model sized to the graphics card it finds:

| Video memory | Model offered |
| --- | --- |
| 22GB or more | `qwen3:32b` |
| 14–21GB | `qwen3:14b` |
| 10–13GB | `qwen3:8b` |
| 6–9GB | `qwen3:4b` |
| less, or none | `qwen3:1.7b` |

These are sized to leave room for the model's context rather than to be the
largest thing that technically loads. You can use any other model, or
[LM Studio](https://lmstudio.ai) instead — see [ai-setup.md](ai-setup.md).

If you would rather use a cloud model, or a model on another machine, skip this
and see [ai-setup.md](ai-setup.md) and [remote-machine.md](remote-machine.md).

## Uninstalling

Delete the program, and if you want, the folder it kept its settings in:

| | Program | Settings, queue, and audit log |
| --- | --- | --- |
| Windows | `%LOCALAPPDATA%\Programs\OutlawRepairKit` | `%APPDATA%\outlaw-repair-kit` |
| Linux | `~/.local/share/outlaw-repair-kit` and `~/.local/bin/outlaw` | `~/.config/outlaw-repair-kit` |

And the shortcuts, if any were made:

| | |
| --- | --- |
| Windows | `%APPDATA%\Microsoft\Windows\Start Menu\Programs\Outlaw Repair Kit (terminal).lnk` |
| Linux | `~/.local/share/applications/systems.outlaw.repairkit*.desktop` |

Stored keys live in the operating system's credential store; remove them with
`outlaw set-key cloud --remove` before deleting the program, or from the
Settings screen in the desktop app.

If you used the setup program, `install-receipt.json` in the program folder
lists everything it did, including anything it added to your PATH and any
shortcut it created. There is no uninstaller: the list is short enough to read,
and a program that can remove things from your machine is a bigger thing to
trust than one that tells you what it put there.
