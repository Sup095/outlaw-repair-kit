//! `outlaw processes` -- what is running, and what would happen to each.
//!
//! Read-only, and says so on the screen. This is stage two of
//! `docs/proposals/process-control.md`: the list exists before anything can
//! act on it, so that it can be looked at on real machines first.
//!
//! Two things it must never do, both of which it would be easy to do by
//! accident:
//!
//! * **Call held memory "freed".** Adding up working sets always overstates
//!   what stopping things returns to the machine, because shared pages are
//!   counted against everyone sharing them. The word here is *holding*.
//! * **Hide what it left alone.** The interesting half of this list is what
//!   the tool refuses to touch and why, and a summary that omitted it would be
//!   asking to be trusted rather than showing its reasoning.

use anyhow::Result;
use ork_core::processes::{Row, Survey};
use ork_core::util::{counted_as, format_bytes};

use crate::style::{bold, dim};

/// How many rows to print before saying how many were left out.
const ENOUGH: usize = 20;

fn how_long(seconds: u64) -> String {
    let days = seconds / 86_400;
    let hours = (seconds % 86_400) / 3_600;
    let minutes = (seconds % 3_600) / 60;
    if days > 0 {
        format!("{days}d {hours}h")
    } else if hours > 0 {
        format!("{hours}h {minutes}m")
    } else {
        format!("{minutes}m")
    }
}

/// One reason and how many it applied to.
///
/// The count is padded away from the reason rather than the reason padded into
/// a fixed column, because these strings are written for people and the longest
/// of them is longer than any column worth having. A fixed width ran the two
/// together -- `...audio stack3 processes` -- which is exactly the sort of
/// thing that makes a careful tool look careless on the screen where somebody
/// is deciding whether to trust it.
fn reason_line(reason: &str, count: usize) -> String {
    format!(
        "  {reason:<42} {}",
        dim(&counted_as(count, "process", "processes"))
    )
}

fn row_line(row: &Row) -> String {
    format!(
        "  {:<34}{:>10}   {}",
        crate::render::ellipsise(&row.name, 33),
        format_bytes(row.memory_bytes),
        dim(&format!("running {}", how_long(row.run_time_secs)))
    )
}

/// Print the rows, then say plainly how many were not printed.
///
/// Silently showing the top twenty of ninety is how a person comes away with
/// a wrong idea of their own machine, so the number left out is always said.
fn some_of(rows: &[&Row], all: bool) {
    let shown = if all {
        rows.len()
    } else {
        rows.len().min(ENOUGH)
    };
    for row in rows.iter().take(shown) {
        println!("{}", row_line(row));
    }
    if shown < rows.len() {
        println!(
            "  {}",
            dim(&format!(
                "and {} more -- `outlaw processes --all` for the rest",
                rows.len() - shown
            ))
        );
    }
}

pub fn show(all: bool, json: bool) -> Result<()> {
    let config = crate::ai::load_config()?;
    let survey = Survey::of_this_machine(&config.processes.pinned)?;

    let candidates: Vec<&Row> = survey.candidates().collect();
    let held = survey.held_back().collect::<Vec<_>>();

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "platform": survey.platform.as_str(),
                "running": survey.rows.len(),
                "protected": survey.protected().count(),
                "held_back": held.len(),
                "candidates": candidates.len(),
                // Named as it is measured, in the machine-readable output as
                // much as on screen. Something reading this must not be able
                // to print it as "will free".
                "memory_held_by_candidates": survey.memory_held_by_candidates(),
                "why_protected": survey.why_protected().iter().map(|(reason, count)| {
                    serde_json::json!({ "reason": reason.describe(), "count": count })
                }).collect::<Vec<_>>(),
                "why_held_back": survey.why_held_back().iter().map(|(reason, count)| {
                    serde_json::json!({ "reason": reason.describe(), "count": count })
                }).collect::<Vec<_>>(),
                "rows": survey.rows,
            }))?
        );
        return Ok(());
    }

    println!();
    println!("{}", bold("What is running"));
    println!(
        "  {} running. {} never touched, {} held back, {} could be stopped.",
        survey.rows.len(),
        survey.protected().count(),
        held.len(),
        candidates.len()
    );
    println!();

    println!(
        "{}  {}",
        bold("Could be stopped"),
        dim(&format!(
            "holding {} between them",
            format_bytes(survey.memory_held_by_candidates())
        ))
    );
    if candidates.is_empty() {
        println!("  {}", dim("nothing"));
    } else {
        some_of(&candidates, all);
        for line in crate::render::wrap(
            "\"Holding\" is what they have now, not what stopping them would give back. \
             The second number is always smaller, and it is only knowable by measuring \
             afterwards.",
            72,
        ) {
            println!("  {}", dim(&line));
        }
    }
    println!();

    println!("{}", bold("Held back, and why"));
    if held.is_empty() {
        println!("  {}", dim("nothing"));
    } else {
        for (reason, count) in survey.why_held_back() {
            println!("{}", reason_line(reason.describe(), count));
        }
        if all {
            println!();
            some_of(&held, all);
        }
    }
    println!();

    println!("{}", bold("Never touched"));
    for (reason, count) in survey.why_protected() {
        println!("{}", reason_line(reason.describe(), count));
    }
    println!();

    println!(
        "  {}",
        dim("Nothing here stops anything. This command only looks.")
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_length_of_time_reads_as_words_rather_than_seconds() {
        assert_eq!(how_long(0), "0m");
        assert_eq!(how_long(90), "1m");
        assert_eq!(how_long(3 * 3600 + 25 * 60), "3h 25m");
        assert_eq!(how_long(50 * 3600), "2d 2h");
    }

    #[test]
    fn one_of_something_is_not_called_one_processs() {
        // It said "1 processs" and "43 processs". Small, and exactly the kind
        // of thing that makes somebody doubt the numbers next to it on the
        // screen where they are deciding whether to trust this tool.
        assert!(reason_line("pinned", 1).contains("1 process"));
        assert!(!reason_line("pinned", 1).contains("processs"));
        assert!(reason_line("pinned", 43).contains("43 processes"));
    }

    #[test]
    fn a_long_reason_does_not_run_into_the_number_beside_it() {
        // The longest reason in the tool is longer than any sensible fixed
        // column, and a fixed column produced `...audio stack3 processes`.
        let longest = ork_core::processes::Protection::DisplayInputAudio.describe();
        let line = reason_line(longest, 3);
        assert!(
            line.contains(&format!("{longest} ")),
            "the reason and the count ran together: {line}"
        );
    }

    #[test]
    fn every_reason_the_tool_can_give_fits_without_running_together() {
        // Checked against the reasons themselves rather than a copy of them,
        // so adding a longer one fails here instead of on somebody's screen.
        use ork_core::processes::{Protection, Restraint};
        let reasons: Vec<&str> = [
            Protection::OperatingSystem.describe(),
            Protection::Security.describe(),
            Protection::DriverOrControlPanel.describe(),
            Protection::DisplayInputAudio.describe(),
            Protection::Networking.describe(),
            Protection::DiskEncryption.describe(),
            Protection::Accessibility.describe(),
            Protection::TheToolItself.describe(),
            Restraint::RunsAsAnotherAccount.describe(),
            Restraint::InFrontOfYou.describe(),
            Restraint::JustStarted.describe(),
            Restraint::MayHoldUnsavedWork.describe(),
            Restraint::CannotBeRestarted.describe(),
            Restraint::MayBeSyncingFiles.describe(),
            Restraint::BelongsToAnotherProgram.describe(),
            Restraint::HowYouWouldRecover.describe(),
            Restraint::Pinned.describe(),
        ]
        .into_iter()
        .collect();

        for reason in reasons {
            let line = reason_line(reason, 2);
            assert!(
                line.contains(&format!("{reason} ")),
                "`{reason}` runs into its count: {line}"
            );
        }
    }

    #[test]
    fn a_very_long_time_does_not_turn_into_a_wrong_short_one() {
        // A machine left on for a month is ordinary, and a number that wrapped
        // round would make an old process look like a new one -- which is
        // exactly the judgement somebody is using this list to make.
        assert_eq!(how_long(400 * 86_400), "400d 0h");
    }
}
