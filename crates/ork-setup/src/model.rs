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
                    "{} — {} GB of video memory",
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
    if which("ollama").is_some() {
        return Runner::Ollama;
    }
    if which("lms").is_some() {
        return Runner::Present {
            name: "LM Studio".to_string(),
        };
    }
    Runner::None
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
        bail!(
            "{} did not succeed: {}",
            program,
            first_meaningful_line(&output.stderr).unwrap_or("no reason given")
        );
    }
    Ok(as_typed(program, &args))
}

/// Ask Ollama to fetch a model.
///
/// This can take a very long time on a slow connection and is not given a
/// deadline, which is the same rule the rest of this project runs on: work is
/// supervised, never timed out.
pub fn pull(tag: &str) -> Result<()> {
    let output = run_capture("ollama", &["pull", tag])
        .with_context(|| format!("could not ask Ollama for {tag}"))?;
    if !output.success {
        bail!(
            "Ollama could not fetch {tag}: {}",
            first_meaningful_line(&output.stderr).unwrap_or("no reason given")
        );
    }
    Ok(())
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
        assert!(summary.contains("24 GB"), "{summary}");
        assert_eq!(hardware.pick.tag, "qwen3:32b");
    }

    #[test]
    fn no_card_says_so_rather_than_reporting_zero() {
        // "0 GB of video memory" reads like a broken card. There isn't one.
        let hardware = Hardware {
            gpus: Vec::new(),
            vram_bytes: None,
            pick: model_for_vram(None),
        };
        let summary = hardware.summary();
        assert!(!summary.contains("0 GB"), "{summary}");
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
    fn a_failure_with_nothing_to_say_admits_that_too() {
        assert_eq!(first_meaningful_line("\n\n   \n"), None);
        assert_eq!(
            first_meaningful_line("\n  something went wrong\nmore"),
            Some("something went wrong")
        );
    }
}
