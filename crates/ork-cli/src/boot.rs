//! The terminal boot screen.
//!
//! Deliberately retro: amber phosphor, block-drawn progress, a log pane that
//! scrolls the last few lines. It is decoration over the top of
//! [`ork_boot::boot`], which is where the actual work happens -- nothing here
//! decides anything, so a piped or scripted run loses the paint and keeps the
//! behaviour.
//!
//! On the terminal palette: the foreground and background are set with OSC 10
//! and 11 while the screen is up and reset afterwards. The font is not
//! touched, because no portable escape sequence changes it -- terminals treat
//! that as the user's setting, not the program's. The colour scheme does the
//! work instead.

use std::io::{IsTerminal, Write};

use ork_boot::{BootEvent, BootReport, CheckState, UpdateStatus};

use crate::style;

/// How many recent lines the log pane shows.
const LOG_LINES: usize = 3;

/// Amber, in the phosphor sense.
const AMBER: &str = "38;5;214";
const AMBER_DIM: &str = "38;5;136";

const BANNER: &str = r#"  ___  _   _ _____ _        _    __        __
 / _ \| | | |_   _| |      / \   \ \      / /
| | | | | | | | | | |     / _ \   \ \ /\ / /
| |_| | |_| | | | | |___ / ___ \   \ V  V /
 \___/ \___/  |_| |_____/_/   \_\   \_/\_/"#;

/// Restores the terminal palette however this function is left, including on
/// a panic or a Ctrl-C that unwinds. Leaving someone's terminal amber would be
/// a rude way to end.
struct Palette {
    changed: bool,
}

impl Palette {
    fn apply() -> Self {
        // Opt-out for anyone whose terminal handles OSC badly, or who simply
        // does not want their colours touched.
        let wanted = style::colour_enabled() && std::env::var_os("ORK_NO_BOOT_THEME").is_none();
        if wanted {
            print!("\u{1b}]10;#ffb000\u{7}\u{1b}]11;#120c04\u{7}");
            let _ = std::io::stdout().flush();
        }
        Self { changed: wanted }
    }
}

impl Drop for Palette {
    fn drop(&mut self) {
        if self.changed {
            // OSC 110/111: back to whatever the user had.
            print!("\u{1b}]110\u{7}\u{1b}]111\u{7}");
            let _ = std::io::stdout().flush();
        }
    }
}

fn amber(text: &str) -> String {
    style::paint(AMBER, text)
}

fn amber_dim(text: &str) -> String {
    style::paint(AMBER_DIM, text)
}

fn state_mark(state: CheckState) -> String {
    match state {
        CheckState::Pass => style::paint("38;5;214", "[ ok ]"),
        CheckState::Warn => style::paint("1;38;5;220", "[warn]"),
        CheckState::Fail => style::paint("1;38;5;196", "[FAIL]"),
    }
}

/// Draws the progress bar and the rolling log in place, one frame per event.
struct Screen {
    /// How many lines the last frame occupied, so the next one can overwrite it.
    drawn: usize,
    log: Vec<String>,
    animated: bool,
}

impl Screen {
    fn new() -> Self {
        Self {
            drawn: 0,
            log: Vec::new(),
            animated: std::io::stdout().is_terminal(),
        }
    }

    fn push(&mut self, line: String) {
        self.log.push(line);
        if self.log.len() > LOG_LINES {
            self.log.remove(0);
        }
    }

    fn frame(&mut self, progress: f32) {
        // Without a terminal there is no cursor to move, so each line is simply
        // printed once, in order. Piping the boot screen still reads correctly.
        if !self.animated {
            if let Some(line) = self.log.last() {
                println!("{line}");
            }
            return;
        }

        if self.drawn > 0 {
            print!("\u{1b}[{}A", self.drawn);
        }

        let mut lines = 0;
        for line in &self.log {
            // Clear to end of line: frames differ in width and stale tails
            // would otherwise survive underneath a shorter line.
            println!("\u{1b}[2K{line}");
            lines += 1;
        }
        for _ in self.log.len()..LOG_LINES {
            println!("\u{1b}[2K");
            lines += 1;
        }
        println!("\u{1b}[2K{}", bar(progress));
        lines += 1;

        self.drawn = lines;
        let _ = std::io::stdout().flush();
    }
}

/// A block-drawn progress bar, `[████████░░░░░░░░]  50%`.
fn bar(progress: f32) -> String {
    const WIDTH: usize = 34;
    let clamped = progress.clamp(0.0, 1.0);
    let filled = (clamped * WIDTH as f32).round() as usize;
    let track = format!(
        "{}{}",
        "\u{2588}".repeat(filled),
        "\u{2591}".repeat(WIDTH.saturating_sub(filled)),
    );
    format!(
        "  {}{}{}  {:>3}%",
        amber_dim("["),
        amber(&track),
        amber_dim("]"),
        (clamped * 100.0).round()
    )
}

fn print_banner() {
    println!();
    for line in BANNER.lines() {
        println!("  {}", amber(line));
    }
    println!("  {}", amber_dim("\u{2500}".repeat(45).as_str()));
    println!(
        "  {}   {}",
        style::bold(&amber("R E P A I R   K I T")),
        amber_dim(&format!("v{}", ork_boot::CURRENT_VERSION)),
    );
    println!("  {}", amber_dim("by Outlaw Systems"));
    println!();
}

/// Run the start-up sequence, showing the boot screen.
///
/// Returns the report so the caller can decide what a failure means; this
/// function only reports, it never exits the process.
pub async fn run() -> BootReport {
    show_and_run().await
}

/// The same sequence with nothing printed.
///
/// For `--json`, which needs the self-test to have actually happened and
/// cannot have a banner drawn across its output. The two used to be the same
/// choice: `--json` skipped the boot entirely rather than quieten it, which
/// meant `--json fix --apply` changed the machine without the self-test that
/// is supposed to stop it doing so when the snapshot area cannot be vouched
/// for. Quiet is a presentation decision; whether the check runs is not.
pub async fn run_quietly() -> BootReport {
    ork_boot::boot(|_| {}).await
}

async fn show_and_run() -> BootReport {
    let _palette = Palette::apply();
    print_banner();

    let mut screen = Screen::new();
    let report = ork_boot::boot(|event| {
        let line = match &event {
            BootEvent::Started { .. } => amber_dim("running self-test\u{2026}"),
            BootEvent::Check { result, .. } => format!(
                "{} {} {}",
                state_mark(result.state),
                amber(&result.name),
                amber_dim(&result.detail),
            ),
            BootEvent::Update { status, .. } => {
                format!("{} {}", state_mark(event.state()), amber(&status.summary()))
            }
            BootEvent::Finished { line, .. } => {
                format!(
                    "{} {}",
                    state_mark(event.state()),
                    style::bold(&amber(line))
                )
            }
        };
        screen.push(line);
        screen.frame(event.progress());
    })
    .await;

    println!();
    if !report.ready() {
        // Failures scrolled out of a three-line log pane, so they are repeated
        // here in full. This is the one thing on the screen that must not be
        // missed.
        for failure in report.selftest.failures() {
            println!(
                "  {} {}: {}",
                state_mark(CheckState::Fail),
                failure.name,
                failure.detail
            );
        }
        println!();
    }
    if let UpdateStatus::Available { latest, url, .. } = &report.update {
        // Reported, never installed: replacing the binary is the user's call.
        println!("  {}", amber(&format!("Version {latest} is available.")));
        println!("  {}", amber_dim(&format!("Download it from {url}")));
        println!();
    }

    report
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bar_fills_from_empty_to_full() {
        assert!(bar(0.0).contains("  0%"));
        assert!(bar(0.5).contains(" 50%"));
        assert!(bar(1.0).contains("100%"));
        // Out-of-range progress must not panic or produce a ragged bar.
        assert!(bar(-1.0).contains("  0%"));
        assert!(bar(2.0).contains("100%"));
    }

    #[test]
    fn the_bar_is_always_the_same_width() {
        let width = |text: &str| {
            text.chars()
                .filter(|c| *c == '\u{2588}' || *c == '\u{2591}')
                .count()
        };
        for step in 0..=10 {
            assert_eq!(width(&bar(step as f32 / 10.0)), 34);
        }
    }

    #[test]
    fn the_log_pane_keeps_only_the_most_recent_lines() {
        let mut screen = Screen::new();
        screen.animated = false;
        for index in 0..10 {
            screen.push(index.to_string());
        }
        assert_eq!(
            screen.log,
            vec!["7".to_string(), "8".to_string(), "9".to_string()]
        );
    }

    #[test]
    fn the_banner_names_the_tool_and_who_made_it() {
        // The ASCII spells OUTLAW; the rest is plain text below it.
        assert!(BANNER.lines().count() >= 5);
        assert!(
            BANNER.lines().all(|line| line.len() <= 60),
            "the banner must fit a narrow terminal"
        );
    }
}
