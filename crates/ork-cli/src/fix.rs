//! CLI surface for the triage queue and the fix engine.

use anyhow::Result;
use ork_ai::runbook::RunbookLibrary;
use ork_core::Config;
use ork_core::finding::Triage;
use ork_core::scan::ScanReport;
use ork_fix::action::FixAction;
use ork_fix::engine::{Approval, Approver, DryRun, FixEngine, ItemOutcome};
use ork_fix::plan::candidates_for;
use ork_fix::snapshot::detect_system_snapshot_support;
use ork_fix::store::{FixStore, ItemState, TriageItem};
use ork_fix::verify::VerifierRegistry;

use crate::render::wrap;
use crate::style::{bold, dim, severity_label};

/// Where the queue, history, and audit log are kept.
fn state_dir() -> Result<std::path::PathBuf> {
    let path = Config::default_path()?;
    Ok(path
        .parent()
        .map(|dir| dir.to_path_buf())
        .unwrap_or_else(|| std::path::PathBuf::from(".")))
}

pub fn open_store() -> Result<FixStore> {
    FixStore::open(&state_dir()?.join("state.db"))
}

/// Put a scan's complex findings on the queue.
///
/// Only findings the probes marked for triage. Simple deterministic problems
/// are not queued -- queueing everything would bury the things that actually
/// need working through.
pub fn enqueue_from_scan(report: &ScanReport) -> Result<usize> {
    let store = open_store()?;
    let mut added = 0;
    for finding in report.findings() {
        if finding.triage == Triage::Queue && store.enqueue(finding)? {
            added += 1;
        }
    }
    Ok(added)
}

/// `outlaw queue` -- what is waiting to be worked.
pub fn show_queue(json: bool) -> Result<()> {
    let store = open_store()?;
    let items = store.all()?;

    if json {
        println!("{}", serde_json::to_string_pretty(&items)?);
        return Ok(());
    }

    if items.is_empty() {
        println!("The triage queue is empty.");
        println!("{}", dim("Run `outlaw scan` to look for problems."));
        return Ok(());
    }

    let pending = items
        .iter()
        .filter(|item| item.state == ItemState::Pending)
        .count();
    println!(
        "{}",
        bold(&format!("{} item(s), {pending} still to work", items.len()))
    );
    println!();

    for item in &items {
        let state = match item.state {
            ItemState::Pending => "waiting".to_string(),
            ItemState::Resolved => "fixed".to_string(),
            ItemState::Exhausted => "no fix found".to_string(),
            ItemState::Dismissed => "dismissed".to_string(),
        };
        println!("{}  {}", severity_label(item.severity), bold(&item.title));
        println!(
            "            {}",
            dim(&format!(
                "{state} -- {} attempt(s) so far -- {}",
                item.attempts, item.occurrence_key
            ))
        );
    }
    println!();
    println!(
        "{}",
        dim("`outlaw fix` works through them. `outlaw fix --apply` allows changes.")
    );
    Ok(())
}

/// `outlaw audit` -- everything the tool has done.
pub fn show_audit(limit: usize, json: bool) -> Result<()> {
    let store = open_store()?;
    let entries = store.audit_log(limit)?;

    if json {
        let value: Vec<_> = entries
            .iter()
            .map(|(at, kind, message)| serde_json::json!({"at": at, "kind": kind, "message": message}))
            .collect();
        println!("{}", serde_json::to_string_pretty(&value)?);
        return Ok(());
    }

    if entries.is_empty() {
        println!("Nothing has been recorded yet.");
        return Ok(());
    }

    println!(
        "{}",
        bold(&format!("Last {} entries, newest first", entries.len()))
    );
    println!();
    for (at, kind, message) in entries {
        println!("  {} {:<14} {message}", dim(&at), kind);
    }
    Ok(())
}

/// Asks in the terminal before anything is changed.
struct AskInTerminal;

impl Approver for AskInTerminal {
    fn approve(&self, action: &FixAction, item: &TriageItem) -> Approval {
        use std::io::Write;

        println!();
        println!("{}", bold("This would change your system:"));
        println!("  {}", action.describe());
        println!("  {}", dim(&format!("to address: {}", item.title)));
        print!("Allow it? [y]es / [n]o / [s]top: ");
        let _ = std::io::stdout().flush();

        let mut answer = String::new();
        if std::io::stdin().read_line(&mut answer).is_err() {
            return Approval::Decline;
        }
        match answer.trim().to_ascii_lowercase().as_str() {
            "y" | "yes" => Approval::Approve,
            "s" | "stop" => Approval::StopEverything,
            // Anything unrecognised means no. A misread keystroke must never
            // be taken as consent to change someone's machine.
            _ => Approval::Decline,
        }
    }
}

/// `outlaw fix` -- work the queue.
///
/// Dry run unless `--apply` is given. Even with `--apply`, every change is
/// confirmed individually.
pub async fn work_queue(apply: bool, json: bool) -> Result<()> {
    let store = open_store()?;
    let items = store.pending()?;

    if items.is_empty() {
        println!("Nothing waiting. Run `outlaw scan` first.");
        return Ok(());
    }

    let library =
        RunbookLibrary::load(state_dir().ok().map(|dir| dir.join("runbooks")).as_deref())?;
    let platform = ork_core::platform::detect()?.kind().to_string();
    let engine = FixEngine::new(open_store()?, state_dir()?.join("snapshots"));
    let verifiers = VerifierRegistry::standard();

    // Ctrl-C stops after the current step rather than mid-change.
    let cancel = engine.cancel_token();
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            eprintln!("\nStopping after the current step...");
            cancel.cancel();
        }
    });

    if !json {
        // Said up front, because it is the difference between "this tool will
        // fix things" and "this tool will tell me about things", and people
        // deserve to know which one they are getting before they start.
        let testable = verifiers.coverage(&items);
        println!(
            "{}",
            dim(&format!(
                "{testable} of {} can be tested after a change, so only those can be fixed automatically. The rest are explained instead.",
                items.len()
            ))
        );
        println!();
    }

    if !apply && !json {
        println!("{}", bold("Dry run -- nothing will be changed."));
        println!(
            "{}",
            dim("Add --apply to allow changes. You will still be asked each time.")
        );
        println!();

        let support = detect_system_snapshot_support();
        if !support.available {
            for line in wrap(&support.detail, 74) {
                println!("{}", dim(&line));
            }
            println!();
        }
    }

    let approver: Box<dyn Approver> = if apply {
        Box::new(AskInTerminal)
    } else {
        Box::new(DryRun)
    };

    let mut results = Vec::new();
    for item in &items {
        let candidates = candidates_for(item, &library, &platform);

        if !json {
            println!("{}  {}", severity_label(item.severity), bold(&item.title));
        }

        // A problem with no verifier gets advice rather than a change. That
        // restriction lives in the engine, not here: asking the registry for
        // `None` is how this front-end says "there is no way to test this",
        // and the engine is what refuses to touch the machine on that basis.
        let outcome = engine
            .work_item(
                item,
                candidates,
                verifiers.for_item(item),
                approver.as_ref(),
            )
            .await?;

        if !json {
            match &outcome {
                ItemOutcome::Resolved { action } => {
                    println!("  {}", bold("fixed"));
                    println!("  {}", dim(action));
                }
                ItemOutcome::NeedsAPerson { instructions } => {
                    println!("  {}", dim("This one needs you. Least disruptive first:"));
                    for (index, instruction) in instructions.iter().enumerate() {
                        for (line_number, line) in wrap(instruction, 70).into_iter().enumerate() {
                            if line_number == 0 {
                                println!("   {}. {line}", index + 1);
                            } else {
                                println!("      {line}");
                            }
                        }
                    }
                }
                ItemOutcome::NoCandidates => {
                    println!(
                        "  {}",
                        dim(
                            "No known fix. `outlaw scan --explain` may be able to reason about it."
                        )
                    );
                }
                ItemOutcome::Exhausted { tried } => {
                    println!(
                        "  {}",
                        dim(&format!("tried {tried} candidate(s); none worked"))
                    );
                }
                ItemOutcome::Stopped => {
                    println!("  {}", dim("stopped"));
                }
            }
            println!();
        }

        let stopped = matches!(outcome, ItemOutcome::Stopped);
        results.push(serde_json::json!({
            "occurrence_key": item.occurrence_key,
            "title": item.title,
            "outcome": outcome,
        }));
        if stopped {
            break;
        }
    }

    if json {
        println!("{}", serde_json::to_string_pretty(&results)?);
    } else {
        println!(
            "{}",
            dim("Everything attempted is recorded. See `outlaw audit`.")
        );
    }
    Ok(())
}
