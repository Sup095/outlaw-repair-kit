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
//! * **Let a rail that did not run look like one that did.** "Anything with a
//!   window in front of you" is not a rule the tool can always apply -- on
//!   Wayland nothing can. Where it could not, the list says so, in the same
//!   place and the same voice as a scan reporting a check that did not run.

use anyhow::Result;
use ork_core::processes::{Program, Row, Survey, Sweep};
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

/// One program, its total, and how much of it a sweep would offer.
///
/// The last column is the whole reason this section exists. A program with
/// fewer offered than running is a program that would still be on screen
/// afterwards, and somebody who was not told that reads the leftover window as
/// the tool having failed.
fn program_line(program: &Program) -> String {
    format!(
        "  {:<30}{:>10}   {:<14} {}",
        crate::render::ellipsise(&program.name, 29),
        format_bytes(program.memory_bytes),
        counted_as(program.processes(), "process", "processes"),
        // Worded in `ork-core`, so the window's column cannot say something
        // else about the same program.
        dim(&program.sweep().briefly())
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

/// `outlaw processes`, including the two options that change a setting.
///
/// Pinning lives here rather than under `config` because the moment somebody
/// wants a program left alone is the moment they are looking at it in this
/// list. It is also the answer to a gap that stood for as long as the setting
/// did: `[processes] pinned` could only be used by finding a TOML file and
/// editing it by hand, which is exactly what this tool is supposed to save
/// people from.
pub fn run(all: bool, pin: Option<String>, unpin: Option<String>, json: bool) -> Result<()> {
    match (pin, unpin) {
        (Some(name), _) => set_pinned(&name, true, json),
        (_, Some(name)) => set_pinned(&name, false, json),
        _ => show(all, json),
    }
}

/// Add a program to the leave-alone list, or take it off it.
///
/// By name rather than by process id, deliberately. A browser is forty
/// processes and pinning one of them would leave the other thirty-nine
/// offered, which is not what anybody means by it -- and process identifiers
/// are reused, so a pin against one would eventually apply to something else
/// entirely.
fn set_pinned(name: &str, pinned: bool, json: bool) -> Result<()> {
    let path = ork_core::Config::default_path()?;
    let mut config = ork_core::Config::load_or_default(&path)?;
    let changed = if pinned {
        config.processes.pin(name)
    } else {
        config.processes.unpin(name)
    };
    // Nothing to write is not a failure. Writing anyway would rewrite a file
    // somebody may have laid out by hand, for no reason at all.
    if changed {
        config.save(&path)?;
    }

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "name": name,
                "pinned": config.processes.is_pinned(name),
                "changed": changed,
                "path": path.display().to_string(),
                "pinned_programs": config.processes.pinned,
            }))?
        );
        return Ok(());
    }

    println!();
    if changed && pinned {
        println!("  {name} will be left alone. It will not be offered for stopping.");
    } else if changed {
        println!("  {name} is no longer pinned. It will be judged like anything else.");
    } else if pinned {
        println!("  {name} was already pinned. Nothing changed.");
    } else {
        println!("  {name} was not pinned. Nothing changed.");
    }
    println!("  {}", dim(&format!("settings: {}", path.display())));
    println!();
    Ok(())
}

pub fn show(all: bool, json: bool) -> Result<()> {
    let config = crate::ai::load_config()?;
    let survey = Survey::of_this_machine(&config.processes.pinned)?;

    let candidates: Vec<&Row> = survey.candidates().collect();
    let held = survey.held_back().collect::<Vec<_>>();
    let programs = survey.by_program();

    if json {
        // Built in `ork-core`, not here. The window publishes the same object
        // from the same function, so there is one answer rather than two
        // hand-written copies of it.
        println!("{}", serde_json::to_string_pretty(&survey.as_report())?);
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
        bold("By program"),
        dim("several processes of one name are one program to you")
    );
    if programs.is_empty() {
        println!("  {}", dim("nothing"));
    } else {
        let shown = if all {
            programs.len()
        } else {
            programs.len().min(ENOUGH)
        };
        for program in programs.iter().take(shown) {
            println!("{}", program_line(program));
        }
        if shown < programs.len() {
            println!(
                "  {}",
                dim(&format!(
                    "and {} more -- `outlaw processes --all` for the rest",
                    programs.len() - shown
                ))
            );
        }
        // Only said when it is true of something on the screen. A caveat
        // printed under a list it does not apply to teaches people to skip
        // the caveats.
        if programs
            .iter()
            .take(shown)
            .any(|program| matches!(program.sweep(), Sweep::PartOfIt { .. }))
        {
            for line in crate::render::wrap(
                "Where fewer are offered than are running, stopping the offered ones                  leaves the program running with fewer processes. It does not close it.",
                72,
            ) {
                println!("  {}", dim(&line));
            }
        }
    }
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

    if let Some(why) = survey.in_front.unanswered() {
        println!("{}", bold("One rule did not run"));
        for line in crate::render::wrap(
            &format!(
                "Nothing with a window in front of you is offered for stopping, and on                  this machine that could not be checked: {why}. Everything else below                  still applies. It means the list may include what you are looking at.",
            ),
            72,
        ) {
            println!("  {}", dim(&line));
        }
        println!();
    }

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

    /// Every case in `tests/shared/duration-cases.json`, which the window's
    /// own test suite reads as well.
    ///
    /// The window formats this in TypeScript rather than asking the back end,
    /// because a round trip for every row of a two-hundred-row list is not
    /// worth paying to avoid six duplicated lines. Six duplicated lines drift.
    /// Both being checked against one table means whichever moves is the one
    /// whose test fails, rather than the two quietly describing the same
    /// running process two different ways on two screens.
    #[test]
    fn the_window_and_the_terminal_agree_on_every_shared_case() {
        let table = include_str!("../../../tests/shared/duration-cases.json");
        let parsed: serde_json::Value =
            serde_json::from_str(table).expect("the shared table is readable");
        let cases = parsed["cases"].as_array().expect("the table has cases");
        // A path that stopped resolving, or a table emptied by accident, would
        // otherwise make this pass by checking nothing.
        assert!(cases.len() > 8, "only {} cases loaded", cases.len());
        for case in cases {
            let seconds = case["seconds"].as_u64().expect("seconds");
            let wanted = case["expect"].as_str().expect("expected string");
            assert_eq!(
                how_long(seconds),
                wanted,
                "{seconds} seconds: the terminal and the window have drifted"
            );
        }
    }

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
