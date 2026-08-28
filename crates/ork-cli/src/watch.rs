//! `outlaw watch` -- keep looking, and speak up only when something changes.
//!
//! The whole of the behaviour is in [`ork_core::watch`]; this is presentation.
//! What is worth saying about the presentation is that a quiet round prints
//! nothing at all. Not a dot, not a timestamp, not "no changes". A watcher
//! that prints something every quarter of an hour fills a terminal with
//! evidence that nothing happened, and by the time something does happen it is
//! one line among four hundred.
//!
//! What it does print, once, is a header saying it has started and how often
//! it will look -- so that silence afterwards is understood as the watcher
//! working rather than as the watcher having died.

use std::io::IsTerminal;

use anyhow::Result;
use ork_core::tier::ScanTier;
use ork_core::watch::{Baseline, Change, WatchEvent, Watcher};
use tokio::sync::mpsc;

use crate::style::{bold, dim, severity_label};

/// An instant, written the way the audit log writes one.
///
/// Shared with the audit log rather than formatted here, because this screen
/// and that one had the same bug: a stored instant printed exactly as stored,
/// to seven decimal places, in UTC, at somebody trying to read what happened
/// on their computer this afternoon.
fn when(at: time::OffsetDateTime) -> String {
    ork_core::util::readable_time(
        &at.format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default(),
    )
}

/// Watch until Ctrl-C, or take a single look and stop.
pub async fn run(tier: ScanTier, every_minutes: u64, json: bool, once: bool) -> Result<()> {
    let interval = std::time::Duration::from_secs(every_minutes.saturating_mul(60));

    // One look and out is what a scheduled task wants: the system's own
    // scheduler decides when, and this decides what changed. It shares the
    // same memory as the running watcher, so moving between the two loses
    // nothing.
    if once {
        let watcher = Watcher::new().tier(tier);
        let look = watcher.look_once().await?;
        if json {
            println!("{}", serde_json::to_string_pretty(&look)?);
            return Ok(());
        }

        // A look that changed nothing prints nothing -- into a log. A
        // scheduled task writing "no change" every hour is a log nobody reads,
        // and by the time it says something nobody sees it.
        //
        // To a person, though, silence and failure are the same thing on
        // screen. When somebody is watching, a quiet look says so in one line;
        // when the output is going anywhere else, it stays quiet. Both halves
        // of that were asked for, and they are not in conflict -- they are
        // about different readers.
        let note = look.quiet().then(|| quiet_note(&look));
        show(&WatchEvent::Looked {
            look: Box::new(look),
        });
        if let Some(note) = note
            && std::io::stdout().is_terminal()
        {
            println!("{}", dim(&note));
        }
        return Ok(());
    }

    let (sender, mut events) = mpsc::unbounded_channel();
    let watcher = Watcher::new()
        .tier(tier)
        .interval(interval)
        .with_events(sender);

    let cancel = watcher.cancel_token();

    // Ctrl-C stops the watcher rather than killing the process, so the round
    // in progress ends cleanly and what has been learned gets written down.
    let interrupt = cancel.clone();
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            interrupt.cancel();
        }
    });

    let printer = tokio::spawn(async move {
        while let Some(event) = events.recv().await {
            if json {
                if let Ok(line) = serde_json::to_string(&event) {
                    println!("{line}");
                }
                continue;
            }
            show(&event);
        }
    });

    let outcome = watcher.run().await;
    // The watcher holds the only other end of the channel the printer is
    // reading, so it has to go before the printer will ever see the channel
    // close. Without this, Ctrl-C stopped the watcher cleanly and then left
    // the process sitting there, apparently ignoring it.
    drop(watcher);
    let _ = printer.await;
    outcome
}

/// What to tell a person after a single look that changed nothing.
///
/// Its own function because the wording has to be conditional and getting it
/// wrong would be a lie: a look where two checks could not run has not found
/// that nothing changed, it has found that nothing changed *among the checks
/// that ran*. Those are different claims, and only one of them is true.
fn quiet_note(look: &ork_core::watch::Look) -> String {
    if look.did_not_run.is_empty() {
        "Looked, and nothing has changed. `outlaw watching` says what it knows.".to_string()
    } else {
        format!(
            "Looked, and nothing changed among the checks that ran -- but {} did not run, so              nothing {} looks for was judged either way.",
            look.did_not_run.join(", "),
            if look.did_not_run.len() == 1 {
                "it"
            } else {
                "they"
            }
        )
    }
}

fn show(event: &WatchEvent) {
    match event {
        WatchEvent::Started {
            interval_secs,
            known,
        } => {
            let minutes = interval_secs / 60;
            println!(
                "{} looking every {minutes} minute{}. Ctrl-C to stop.",
                bold("Watching."),
                if minutes == 1 { "" } else { "s" }
            );
            if *known > 0 {
                println!(
                    "{}",
                    dim(&format!(
                        "  {known} problem{} already known about; you will hear about changes, not about {}.",
                        if *known == 1 { "" } else { "s" },
                        if *known == 1 { "it" } else { "them" }
                    ))
                );
            }
            println!(
                "{}",
                dim("  Nothing further will be printed unless something changes.")
            );
            println!();
        }
        // Deliberately silent. Saying "looking..." every quarter of an hour is
        // how a watcher teaches somebody to stop reading it.
        WatchEvent::Looking => {}
        WatchEvent::Looked { look } => {
            if look.established_baseline {
                println!(
                    "{}",
                    dim(&format!(
                        "Recorded how this machine is right now: {} thing{} to keep an eye on. Watching for changes from here.",
                        look.recorded,
                        if look.recorded == 1 { "" } else { "s" }
                    ))
                );
                println!();
                return;
            }

            if look.changes.is_empty() {
                return;
            }

            println!("{}", bold(&when(look.at)));
            for change in &look.changes {
                println!("  {}", line(change));
            }
            // Only mentioned when there is something else being said anyway.
            // A quiet round stays quiet even if a check could not run, or the
            // watcher would announce a skipped check every quarter of an hour.
            if !look.did_not_run.is_empty() {
                println!(
                    "{}",
                    dim(&format!(
                        "  ({} did not run this time, so nothing it looks for was judged either way)",
                        look.did_not_run.join(", ")
                    ))
                );
            }
            println!();
        }
        WatchEvent::Trouble { error } => {
            eprintln!("{} {error}", bold("Could not look this time:"));
            eprintln!("{}", dim("  Still watching."));
        }
        WatchEvent::Stopped => println!("{}", dim("Stopped watching.")),
    }
}

fn line(change: &Change) -> String {
    let severity = match change {
        Change::Cleared { .. } => dim("gone   "),
        _ => severity_label(change.severity()),
    };
    format!("{severity}  {}", change.headline())
}

/// Show what the watcher currently remembers, without watching.
pub fn status(json: bool) -> Result<()> {
    let path = Baseline::default_path()?;
    let baseline = Baseline::load(&path);

    if json {
        println!("{}", serde_json::to_string_pretty(&baseline)?);
        return Ok(());
    }

    if !baseline.established {
        println!("The watcher has not looked at this machine yet.");
        println!("{}", dim("  outlaw watch    start looking"));
        return Ok(());
    }

    println!("{}", bold("What the watcher remembers"));
    println!("{}", dim(&format!("  {}", path.display())));
    println!();

    let mut present: Vec<_> = baseline.seen.values().filter(|seen| seen.present).collect();
    present.sort_by_key(|seen| std::cmp::Reverse(seen.severity));

    if present.is_empty() {
        println!("  Nothing wrong, as of the last look.");
    } else {
        println!("{}", bold("  Present now"));
        for seen in present {
            println!(
                "    {}  {}",
                severity_label(seen.severity),
                bold(&seen.title)
            );
            println!(
                "{}",
                dim(&format!(
                    "             first seen {}, last changed {}",
                    when(seen.first_seen),
                    when(seen.last_change)
                ))
            );
        }
    }

    let gone = baseline.seen.values().filter(|seen| !seen.present).count();
    if gone > 0 {
        println!();
        println!(
            "{}",
            dim(&format!(
                "  {gone} problem{} seen before and not there now, remembered so that {} coming back is recognised as coming back.",
                if gone == 1 { "" } else { "s" },
                if gone == 1 { "it" } else { "them" }
            ))
        );
    }

    // Listed, always. A watcher with a private list of things it has decided
    // not to mention is not a watcher anybody should trust.
    if !baseline.muted.is_empty() {
        println!();
        println!("{}", bold("  Held quiet"));
        for muted in &baseline.muted {
            println!("    {}", bold(&muted.title));
            println!("{}", dim(&format!("             {}", muted.reason)));
        }
        println!(
            "{}",
            dim("  Delete the file above to start over and hear about these again.")
        );
    }

    Ok(())
}

/// Whether a change is one this front-end colours as bad news.
///
/// Kept here rather than inlined so the test below can state the rule.
#[cfg(test)]
fn reads_as_bad_news(change: &Change) -> bool {
    use ork_core::finding::Severity;
    !matches!(change, Change::Cleared { .. }) && change.severity() >= Severity::Low
}

#[cfg(test)]
mod tests {
    use super::*;
    use ork_core::finding::{Category, Finding, Severity, Triage};
    use ork_core::watch::Look;

    fn look(did_not_run: Vec<String>) -> Look {
        Look {
            at: time::OffsetDateTime::now_utc(),
            changes: Vec::new(),
            established_baseline: false,
            recorded: 0,
            did_not_run,
        }
    }

    #[test]
    fn a_quiet_look_that_checked_everything_says_so_plainly() {
        let note = quiet_note(&look(Vec::new()));
        assert!(note.contains("nothing has changed"), "{note}");
    }

    #[test]
    fn a_quiet_look_with_a_check_that_did_not_run_does_not_claim_nothing_changed() {
        // The distinction the whole watcher is built on: a check that did not
        // run cleared nothing and found nothing, and a one-line summary must
        // not quietly widen its claim to cover it.
        let note = quiet_note(&look(vec!["Disk health".to_string()]));
        assert!(note.contains("Disk health"), "{note}");
        assert!(note.contains("among the checks that ran"), "{note}");
        assert!(
            !note.contains("nothing has changed"),
            "claimed more than it knows: {note}"
        );
    }

    #[test]
    fn more_than_one_skipped_check_reads_as_more_than_one() {
        let note = quiet_note(&look(vec!["A".to_string(), "B".to_string()]));
        assert!(note.contains("A, B"), "{note}");
        assert!(note.contains("they look"), "{note}");
    }

    #[test]
    fn a_look_that_established_a_baseline_is_not_quiet() {
        // It has something to say -- how many things it recorded -- and
        // `show` says it. A second line underneath would be the tool
        // answering itself.
        let mut first = look(Vec::new());
        first.established_baseline = true;
        first.recorded = 6;
        assert!(!first.quiet());
    }

    fn finding(severity: Severity) -> Box<Finding> {
        Box::new(
            Finding::builder("storage", "storage.full")
                .subject("C:")
                .severity(severity)
                .category(Category::Storage)
                .title("The system drive is nearly full")
                .detail("detail")
                .triage(Triage::None)
                .build(),
        )
    }

    #[test]
    fn a_cleared_problem_is_never_shown_as_bad_news() {
        // It is the only good news this command ever prints, and printing it
        // in the same red as everything else would make somebody's heart sink
        // at being told a problem went away.
        let cleared = Change::Cleared {
            id: "storage.full".into(),
            subject: Some("C:".into()),
            title: "The system drive is nearly full".into(),
            was: Severity::High,
        };
        assert!(!reads_as_bad_news(&cleared));
        assert!(line(&cleared).contains("cleared:"));

        assert!(reads_as_bad_news(&Change::Appeared {
            finding: finding(Severity::High)
        }));
    }

    #[test]
    fn every_kind_of_change_prints_a_line_that_says_what_happened() {
        // A line that renders as an empty severity and a blank title is worse
        // than no line: it looks like the tool noticed something and could not
        // say what.
        let changes = [
            Change::Appeared {
                finding: finding(Severity::High),
            },
            Change::Worsened {
                finding: finding(Severity::Critical),
                was: Severity::Low,
            },
            Change::Eased {
                finding: finding(Severity::Low),
                was: Severity::Critical,
            },
            Change::Cleared {
                id: "storage.full".into(),
                subject: Some("C:".into()),
                title: "The system drive is nearly full".into(),
                was: Severity::High,
            },
            Change::Flapping {
                finding: finding(Severity::Medium),
                appearances: 3,
            },
        ];

        for change in &changes {
            let rendered = line(change);
            assert!(
                rendered.contains("drive is nearly full"),
                "a change printed without naming what it was about: {rendered:?}"
            );
            assert!(rendered.len() > 20, "{rendered:?}");
        }
    }

    #[test]
    fn a_worsening_says_both_ends_of_the_move() {
        // "It got worse" is not actionable. "It went from low to critical" is.
        let rendered = line(&Change::Worsened {
            finding: finding(Severity::Critical),
            was: Severity::Low,
        });
        assert!(rendered.contains("low"), "{rendered:?}");
        assert!(rendered.contains("critical"), "{rendered:?}");
    }
}
