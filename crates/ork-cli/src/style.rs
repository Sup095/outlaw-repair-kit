//! Terminal styling, with no dependency and no surprises.
//!
//! Colour is disabled when output is redirected, when `NO_COLOR` is set, and
//! when `TERM=dumb`, which covers the cases where escape codes would end up in
//! a log file or a pipe.

use std::io::IsTerminal;
use std::sync::OnceLock;

use ork_core::Severity;

fn colour_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        if std::env::var_os("NO_COLOR").is_some() {
            return false;
        }
        if std::env::var("TERM").is_ok_and(|term| term == "dumb") {
            return false;
        }
        std::io::stdout().is_terminal()
    })
}

fn paint(code: &str, text: &str) -> String {
    if colour_enabled() {
        format!("\u{1b}[{code}m{text}\u{1b}[0m")
    } else {
        text.to_string()
    }
}

pub fn bold(text: &str) -> String {
    paint("1", text)
}

pub fn dim(text: &str) -> String {
    paint("2", text)
}

/// The severity label as it appears at the start of a finding line.
pub fn severity_label(severity: Severity) -> String {
    let (code, label) = match severity {
        Severity::Critical => ("1;97;41", "CRITICAL"),
        Severity::High => ("1;31", "HIGH    "),
        Severity::Medium => ("1;33", "MEDIUM  "),
        Severity::Low => ("36", "LOW     "),
        Severity::Info => ("2", "INFO    "),
    };
    paint(code, label)
}
