//! The optional part: setting up a model on this machine.
//!
//! Optional, and offered rather than assumed. The tool runs every check and
//! explains every known problem with no model at all -- a model only helps
//! with problems that are not in the runbook library. Anyone who says no here
//! gets a working installation, and the screen says so before asking.
//!
//! Nothing here is installed silently. Ollama is another organisation's
//! software, several hundred megabytes of it, and the model itself is several
//! gigabytes more. Both are named, sized, and shown as the exact command that
//! would run, before anything runs.

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use ork_ai::router::{ModelPick, model_for_vram};
use ork_core::platform::{GpuInfo, run_capture, which};

/// What this machine looks like, as far as choosing a model goes.
#[derive(Debug, Clone)]
pub struct Hardware {
    pub gpus: Vec<GpuInfo>,
    /// The largest amount of video memory found on any one card.
    pub vram_bytes: Option<u64>,
    pub pick: ModelPick,
}

pub fn look() -> Hardware {
    // Through the platform layer rather than around it, so that this asks the
    // question exactly the way the tool itself asks it. An installer that
    // detects different hardware from the program it installs is an installer
    // nobody can trust about the program.
    let gpus = ork_core::platform::detect()
        .and_then(|platform| platform.gpus())
        .unwrap_or_default();
    let vram_bytes = gpus.iter().filter_map(|gpu| gpu.vram_total_bytes).max();
    Hardware {
        pick: model_for_vram(vram_bytes),
        gpus,
        vram_bytes,
    }
}

impl Hardware {
    /// One line describing what was found, for somebody who is about to be
    /// asked to download several gigabytes on the strength of it.
    pub fn summary(&self) -> String {
        match (self.gpus.first(), self.vram_bytes) {
            (Some(gpu), Some(bytes)) => {
                format!(
                    // GiB, because that is the division being done. See
                    // the note beside the bands in `ork_ai::router`.
                    "{} — {} GiB of video memory",
                    gpu.name,
                    bytes / (1024 * 1024 * 1024)
                )
            }
            (Some(gpu), None) => format!(
                "{} — how much video memory it has could not be established, so the \
                 smallest option was chosen",
                gpu.name
            ),
            (None, _) => "No graphics card was found, so a model small enough for the \
                          processor was chosen"
                .to_string(),
        }
    }
}

/// What is running models on this machine, if anything is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Runner {
    /// Ollama is installed and can be asked to fetch a model.
    Ollama,
    /// Something else that serves an OpenAI-compatible endpoint is already
    /// here. Nothing is installed in this case -- the tool will find it.
    Present { name: String },
    /// Nothing found.
    None,
}

/// Look for something already able to run a model.
///
/// LM Studio is checked for as well as Ollama, because somebody who already
/// has one should not be offered the other. The tool talks to any
/// OpenAI-compatible endpoint and does not care which of them it is.
pub fn runner() -> Runner {
    if ollama().is_some() {
        return Runner::Ollama;
    }
    if which("lms").is_some() {
        return Runner::Present {
            name: "LM Studio".to_string(),
        };
    }
    Runner::None
}

/// Where Ollama is, if it is anywhere this can find.
///
/// `PATH` is asked first and is not trusted to be the whole answer. A process
/// gets its environment when it starts and never sees a change to it, so the
/// installer that has *just* installed Ollama is the one process on the
/// machine guaranteed not to find it on `PATH` -- and asking for the model
/// immediately afterwards was therefore guaranteed to fail with "the system
/// cannot find the file specified". That is the whole of the bug this
/// function exists to fix.
pub fn ollama() -> Option<PathBuf> {
    if let Some(found) = which("ollama") {
        return Some(found);
    }

    ollama_places().into_iter().find(|place| place.is_file())
}

/// Where each platform's Ollama installer puts it.
fn ollama_places() -> Vec<PathBuf> {
    let mut places = Vec::new();
    if cfg!(windows) {
        if let Some(local) = dirs::data_local_dir() {
            places.push(local.join("Programs").join("Ollama").join("ollama.exe"));
        }
        for variable in ["ProgramFiles", "ProgramFiles(x86)"] {
            if let Some(dir) = std::env::var_os(variable) {
                places.push(PathBuf::from(dir).join("Ollama").join("ollama.exe"));
            }
        }
    } else {
        places.push(PathBuf::from("/usr/local/bin/ollama"));
        places.push(PathBuf::from("/usr/bin/ollama"));
        if let Some(home) = dirs::home_dir() {
            places.push(home.join(".local").join("bin").join("ollama"));
        }
    }
    places
}

/// The exact command that would install Ollama on this platform.
///
/// Shown before it is run, always. This installer will not run somebody
/// else's installer on somebody's machine without showing them what it is
/// about to do.
pub fn install_command() -> Option<(&'static str, Vec<&'static str>)> {
    #[cfg(windows)]
    {
        if which("winget").is_some() {
            return Some((
                "winget",
                vec![
                    "install",
                    "--id",
                    "Ollama.Ollama",
                    "-e",
                    "--accept-package-agreements",
                    "--accept-source-agreements",
                    // Ollama's own installer otherwise puts a window of its
                    // own on the screen, on top of this one, which looks like
                    // something has gone wrong at exactly the moment somebody
                    // is deciding whether to trust this program.
                    "--silent",
                    // And winget itself otherwise stops to ask questions on a
                    // screen nobody is looking at, which reads as a hang.
                    "--disable-interactivity",
                ],
            ));
        }
        None
    }
    #[cfg(not(windows))]
    {
        // Deliberately absent. The published way to install Ollama on Linux is
        // to pipe a script from the internet into a shell, and this program
        // will not do that on somebody's behalf -- running an unread script as
        // a side effect of clicking "yes" in an installer is the exact thing
        // this project tells people not to do. The screen links to the
        // download page instead, and finds Ollama on the next run.
        None
    }
}

/// Render a command the way somebody would type it.
pub fn as_typed(program: &str, args: &[&str]) -> String {
    let mut line = program.to_string();
    for arg in args {
        line.push(' ');
        line.push_str(arg);
    }
    line
}

/// Install Ollama, having been told to.
pub fn install_ollama() -> Result<String> {
    let Some((program, args)) = install_command() else {
        bail!("there is no way to install Ollama automatically on this machine");
    };
    let output = run_capture(program, &args).context("could not run the installer")?;
    if !output.success {
        // Already installed is not a failure. winget reports "no applicable
        // update found" as an error, and treating that as one would tell
        // somebody their install broke when what actually happened is that
        // they already had it.
        let said = format!("{}{}", output.stdout, output.stderr).to_ascii_lowercase();
        let already = said.contains("no applicable update")
            || said.contains("already installed")
            || said.contains("no newer package");
        if !(already && ollama().is_some()) {
            bail!(
                "{} did not succeed: {}",
                program,
                first_meaningful_line(&output.stderr)
                    .or_else(|| first_meaningful_line(&output.stdout))
                    .unwrap_or("no reason given")
            );
        }
    }
    Ok(as_typed(program, &args))
}

/// Ask Ollama to fetch a model, saying how it is going.
///
/// This can take a very long time on a slow connection and is not given a
/// deadline, which is the same rule the rest of this project runs on: work is
/// supervised, never timed out. It reports as it goes, because several
/// gigabytes arriving behind a window that says nothing is indistinguishable
/// from a window that has stopped working.
///
/// Called with the full path to Ollama rather than the name, for the reason
/// set out on [`ollama`].
pub fn pull(tag: &str, say: &mut dyn FnMut(String)) -> Result<()> {
    use std::io::{BufRead, BufReader};
    use std::process::{Command, Stdio};

    use ork_core::unseen::Unseen;

    let program = ollama().context(
        "Ollama is installed but could not be found. Open a new terminal and run \
         `ollama pull` yourself -- everything else is installed.",
    )?;

    let mut child = Command::new(&program)
        .args(["pull", tag])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .unseen()
        .spawn()
        .with_context(|| format!("could not start {}", program.display()))?;

    // Ollama writes its progress to standard error, redrawing one line over
    // and over. Only the lines that say something different are passed on --
    // otherwise this reports a hundred times a second and says nothing.
    let mut last = String::new();
    if let Some(stream) = child.stderr.take() {
        for line in BufReader::new(stream)
            .lines()
            .map_while(std::io::Result::ok)
        {
            let Some(said) = progress_worth_saying(&line) else {
                continue;
            };
            if said != last {
                say(said.clone());
                last = said;
            }
        }
    }

    let status = child
        .wait()
        .with_context(|| format!("could not wait for {}", program.display()))?;
    if !status.success() {
        bail!(
            "Ollama could not fetch {tag}: {}",
            if last.is_empty() {
                "no reason given".to_string()
            } else {
                last
            }
        );
    }
    Ok(())
}

/// Turn one of Ollama's redrawn progress lines into something worth showing.
///
/// It writes a carriage-returned bar with escape codes in it, which is meant
/// for a terminal and is unreadable anywhere else. What is wanted is the last
/// segment and the percentage in it, without the drawing.
fn progress_worth_saying(line: &str) -> Option<String> {
    let flattened = flatten_redraws(line);
    let last = flattened
        .rsplit('\r')
        .map(str::trim)
        .find(|part| !part.is_empty())?;

    // The spinner is the last thing on a "still working" line, and means
    // nothing at all once the line is not being redrawn in place.
    let said = last
        .trim_end_matches(|c: char| ('\u{2800}'..='\u{28ff}').contains(&c) || c.is_whitespace());
    if said.is_empty() {
        return None;
    }
    Some(said.to_string())
}

/// Turn one line of terminal drawing into plain text, keeping the boundaries
/// between successive redraws of it.
///
/// Ollama draws its progress by erasing the line and moving the cursor back to
/// the start, over and over, and sends a newline only when it moves on to the
/// next thing. So one "line" read here is really twenty redraws of the same
/// one. Removing the escape sequences without marking where they were glues
/// all twenty together -- `pulling manifest x pulling manifest x pulling
/// manifest ...` -- which is exactly how the first version reported it.
///
/// A sequence that erases or moves becomes a separator; one that only sets a
/// colour disappears, because a colour change does not start the line again.
fn flatten_redraws(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '\r' {
            out.push('\r');
            continue;
        }
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        let mut ended = ' ';
        for c in chars.by_ref() {
            if c.is_ascii_alphabetic() {
                ended = c;
                break;
            }
        }
        // `m` sets a colour and leaves the cursor where it was. Everything
        // else used here moves or erases, and both mean the line is being
        // drawn again from its beginning.
        if ended != 'm' {
            out.push('\r');
        }
    }
    out
}

/// The first line of output that says anything.
///
/// Progress bars and blank lines are not an explanation, and handing one back
/// as the reason something failed is worse than admitting there was none.
fn first_meaningful_line(text: &str) -> Option<&str> {
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('\u{1b}'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gpu(name: &str, vram: Option<u64>) -> GpuInfo {
        GpuInfo {
            name: name.to_string(),
            vram_total_bytes: vram,
            vram_used_bytes: None,
            driver_version: None,
        }
    }

    const GIB: u64 = 1024 * 1024 * 1024;

    #[test]
    fn a_card_is_described_with_the_memory_that_decided_the_choice() {
        let hardware = Hardware {
            gpus: vec![gpu("NVIDIA GeForce RTX 3090", Some(24 * GIB))],
            vram_bytes: Some(24 * GIB),
            pick: model_for_vram(Some(24 * GIB)),
        };
        let summary = hardware.summary();
        assert!(summary.contains("3090"), "{summary}");
        // GiB, matching the division, and matching what `outlaw models` says
        // about the same card. The two used to disagree a line apart.
        assert!(summary.contains("24 GiB"), "{summary}");
        assert!(
            !summary.replace("GiB", "").contains("GB"),
            "the card is described in two units at once: {summary}"
        );
        assert_eq!(hardware.pick.tag, "qwen3:32b");
    }

    #[test]
    fn no_card_says_so_rather_than_reporting_zero() {
        // "0 GiB of video memory" reads like a broken card. There isn't one.
        let hardware = Hardware {
            gpus: Vec::new(),
            vram_bytes: None,
            pick: model_for_vram(None),
        };
        let summary = hardware.summary();
        assert!(!summary.contains("0 GiB"), "{summary}");
        assert!(summary.contains("processor"), "{summary}");
    }

    #[test]
    fn a_card_whose_memory_is_unknown_admits_it() {
        // An unbadged card with no vendor tools is a real case, and it is not
        // the same as having no card.
        let hardware = Hardware {
            gpus: vec![gpu("Standard Display Adapter", None)],
            vram_bytes: None,
            pick: model_for_vram(None),
        };
        let summary = hardware.summary();
        assert!(summary.contains("could not be established"), "{summary}");
        assert!(summary.contains("Standard Display Adapter"), "{summary}");
    }

    #[test]
    fn the_command_shown_is_the_command_that_would_run() {
        // The screen prints this string and then runs those exact arguments.
        // If they ever drift apart, the installer is showing people one thing
        // and doing another.
        if let Some((program, args)) = install_command() {
            let typed = as_typed(program, &args);
            assert!(typed.starts_with(program), "{typed}");
            for arg in &args {
                assert!(typed.contains(arg), "{arg} missing from {typed}");
            }
        }
    }

    #[test]
    fn there_is_somewhere_to_look_for_ollama_besides_the_path() {
        // The whole point. A process cannot see a PATH change made after it
        // started, so the installer that has just installed Ollama is the one
        // process guaranteed not to find it there.
        assert!(
            !ollama_places().is_empty(),
            "nowhere to look on this platform"
        );
        for place in ollama_places() {
            assert!(
                place
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().starts_with("ollama")),
                "{} does not end in the program's name",
                place.display()
            );
        }
    }

    #[test]
    fn a_drawn_progress_bar_is_reduced_to_what_it_says() {
        // Ollama redraws one line with escape codes in it. Passed through as
        // it stands, it is unreadable anywhere that is not a terminal.
        let drawn = "old text\rpulling manifest";
        assert_eq!(
            progress_worth_saying(drawn).as_deref(),
            Some("pulling manifest")
        );

        let coloured = "\u{1b}[2K\u{1b}[1Gpulling 8934d96d3f08:  47%";
        assert_eq!(
            progress_worth_saying(coloured).as_deref(),
            Some("pulling 8934d96d3f08:  47%")
        );
    }

    #[test]
    fn many_redraws_of_one_line_report_as_one_line() {
        // Exactly what Ollama sends, and exactly what the first version got
        // wrong: it removed the escape sequences without noticing that they
        // were the boundaries, and reported all of them run together.
        let drawn = concat!(
            "\u{1b}[?25l",
            "\u{1b}[2K\u{1b}[1Gpulling manifest \u{280b} ",
            "\u{1b}[2K\u{1b}[1Gpulling manifest \u{2819} ",
            "\u{1b}[2K\u{1b}[1Gpulling manifest \u{2839} ",
        );
        assert_eq!(
            progress_worth_saying(drawn).as_deref(),
            Some("pulling manifest")
        );
    }

    #[test]
    fn a_colour_change_does_not_start_the_line_again() {
        // Colour is the one sequence here that leaves the cursor where it was.
        // Treating it as a boundary would throw away everything before the
        // colour changed, which on a progress line is most of it.
        let coloured = "pulling \u{1b}[32m8934d96d3f08\u{1b}[0m:  47%";
        assert_eq!(
            progress_worth_saying(coloured).as_deref(),
            Some("pulling 8934d96d3f08:  47%")
        );
    }

    #[test]
    fn the_spinner_is_not_part_of_what_is_said() {
        assert_eq!(
            progress_worth_saying("verifying sha256 digest \u{2838}").as_deref(),
            Some("verifying sha256 digest")
        );
    }

    #[test]
    fn a_line_that_says_nothing_is_not_reported() {
        assert_eq!(progress_worth_saying(""), None);
        assert_eq!(progress_worth_saying("\u{1b}[2K"), None);
        assert_eq!(progress_worth_saying("   \r  "), None);
    }

    #[test]
    fn the_install_command_does_not_put_another_window_on_the_screen() {
        // Both of these were missing, and both produce the same symptom: a
        // window appearing over the installer part-way through, which reads as
        // something having gone wrong.
        if let Some((_, args)) = install_command() {
            assert!(args.contains(&"--silent"), "{args:?}");
            assert!(args.contains(&"--disable-interactivity"), "{args:?}");
        }
    }

    #[test]
    fn a_failure_with_nothing_to_say_admits_that_too() {
        assert_eq!(first_meaningful_line("\n\n   \n"), None);
        assert_eq!(
            first_meaningful_line("\n  something went wrong\nmore"),
            Some("something went wrong")
        );
    }
}
