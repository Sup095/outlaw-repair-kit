//! Working the triage queue from the window.
//!
//! The rules are the command line's rules, unchanged: a dry run unless changes
//! are explicitly allowed, one change at a time, a snapshot before each one, a
//! test afterwards, and a rollback if that test does not pass. None of that is
//! reimplemented here -- this module only carries the question "may I do this?"
//! out to the window and the answer back.
//!
//! Two things about that round trip are load-bearing:
//!
//! * **Only an explicit approval is consent.** A closed window, a dropped
//!   channel, an unrecognised answer, a cancelled run -- all of them decline.
//!   There is no path where silence means yes.
//! * **An answer names the question it answers.** Each prompt carries an id and
//!   an answer is ignored unless it matches, so a click that arrives late can
//!   never approve a change the user was never shown.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ork_ai::runbook::RunbookLibrary;
use ork_fix::action::FixAction;
use ork_fix::engine::{Approval, Approver, DryRun, FixEngine, ItemOutcome};
use ork_fix::plan::candidates_for;
use ork_fix::snapshot::detect_system_snapshot_support;
use ork_fix::store::TriageItem;
use ork_fix::verify::VerifierRegistry;
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tokio_util::sync::CancellationToken;

use crate::commands::{AppState, CmdResult, fail, open_store, runbook_dir, state_dir};

/// Progress, as the queue is worked. Mirrors what the terminal prints.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "event", rename_all = "kebab-case")]
pub enum FixEvent {
    Started {
        total: usize,
        /// How many of them can be tested after a change, and so could be
        /// fixed rather than merely explained.
        testable: usize,
        apply: bool,
        /// Absent when a system-level snapshot tool was found.
        snapshot_warning: Option<String>,
    },
    Item {
        index: usize,
        total: usize,
        occurrence_key: String,
        title: String,
        severity: String,
    },
    Outcome {
        occurrence_key: String,
        outcome: ItemOutcome,
    },
    Finished {
        resolved: usize,
        stopped: bool,
    },
}

/// A pending question, and where its answer should go.
struct Question {
    id: u64,
    answer: Sender<Approval>,
}

/// What is asked of the window when a change wants permission.
#[derive(Debug, Clone, Serialize)]
pub struct Ask {
    pub id: u64,
    /// The action in the engine's own words, not a paraphrase.
    pub action: String,
    pub title: String,
    pub occurrence_key: String,
}

#[derive(Default)]
pub struct FixState {
    running: Mutex<Option<CancellationToken>>,
    asking: Arc<Mutex<Option<Question>>>,
    next_id: AtomicU64,
}

/// Asks the window, and waits for as long as it takes.
///
/// There is deliberately no timeout. A question about changing someone's
/// machine that answers itself because they went to make a cup of tea is not a
/// question. The run is cancellable at any moment instead, which is the same
/// escape hatch without the guesswork.
struct AskTheWindow {
    /// Puts the question in front of someone, and reports whether it got
    /// there. Kept as a closure rather than a window handle so that the
    /// "nobody said yes" rules below can be tested without a window.
    announce: Box<dyn Fn(&Ask) -> bool + Send + Sync>,
    asking: Arc<Mutex<Option<Question>>>,
    cancel: CancellationToken,
    next_id: Arc<AtomicU64>,
}

impl AskTheWindow {
    fn park(&self, id: u64, receiver: Receiver<Approval>) -> Approval {
        loop {
            if self.cancel.is_cancelled() {
                return Approval::StopEverything;
            }
            match receiver.recv_timeout(Duration::from_millis(150)) {
                Ok(approval) => return approval,
                Err(RecvTimeoutError::Timeout) => continue,
                // The window went away mid-question. Nobody said yes.
                Err(RecvTimeoutError::Disconnected) => {
                    tracing::warn!(id, "the approval channel closed; declining");
                    return Approval::Decline;
                }
            }
        }
    }
}

impl Approver for AskTheWindow {
    fn approve(&self, action: &FixAction, item: &TriageItem) -> Approval {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (sender, receiver) = channel();

        {
            let Ok(mut slot) = self.asking.lock() else {
                return Approval::Decline;
            };
            *slot = Some(Question { id, answer: sender });
        }

        let ask = Ask {
            id,
            action: action.describe(),
            title: item.title.clone(),
            occurrence_key: item.occurrence_key.clone(),
        };

        // A question that never reaches the window can never be answered, so
        // a failure to deliver it declines rather than waiting forever.
        if !(self.announce)(&ask) {
            if let Ok(mut slot) = self.asking.lock() {
                slot.take();
            }
            return Approval::Decline;
        }

        let approval = self.park(id, receiver);
        if let Ok(mut slot) = self.asking.lock() {
            slot.take();
        }
        approval
    }
}

/// Work the queue. Dry run unless `apply`, and even then every change is asked
/// about individually via `fix://ask`.
///
/// Progress arrives as `fix://event`. The call returns when the run finishes,
/// which may be a long time -- there is no limit on it, only cancellation.
#[tauri::command]
pub async fn fix_run(app: AppHandle, state: State<'_, AppState>, apply: bool) -> CmdResult<bool> {
    let cancel = CancellationToken::new();
    {
        let mut slot = state
            .fix
            .running
            .lock()
            .map_err(|_| "the fix lock was poisoned".to_string())?;
        if slot.is_some() {
            return Err("the queue is already being worked".to_string());
        }
        *slot = Some(cancel.clone());
    }

    let asking = state.fix.asking.clone();
    // Ids never restart, so an answer left over from a previous run cannot
    // collide with a question in this one.
    let next_id = Arc::new(AtomicU64::new(
        state.fix.next_id.fetch_add(1_000, Ordering::SeqCst),
    ));

    // The run happens on a thread of its own, for two reasons that happen to
    // point the same way: the queue's database handle is not shareable between
    // threads, and asking the window for permission parks the caller until an
    // answer comes back. Neither belongs on a runtime shared with the rest of
    // the window.
    let (done, wait) = tokio::sync::oneshot::channel();
    let worker = {
        let asking = asking.clone();
        let cancel = cancel.clone();
        std::thread::Builder::new()
            .name("outlaw-fix".to_string())
            .spawn(move || {
                let runtime = match tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(2)
                    .enable_all()
                    .build()
                {
                    Ok(runtime) => runtime,
                    Err(error) => {
                        let _ = done.send(Err(anyhow::Error::from(error)));
                        return;
                    }
                };
                let outcome = runtime.block_on(run_queue(app, apply, cancel, asking, next_id));
                let _ = done.send(outcome);
            })
    };

    let outcome = match worker {
        Ok(_) => wait
            .await
            .unwrap_or_else(|_| Err(anyhow::anyhow!("the fix run ended without reporting"))),
        Err(error) => Err(anyhow::Error::from(error)),
    };

    // Cleared however it ended, or the next run would find the door locked.
    if let Ok(mut slot) = state.fix.running.lock() {
        *slot = None;
    }
    if let Ok(mut slot) = asking.lock() {
        slot.take();
    }

    outcome.map_err(fail)
}

async fn run_queue(
    app: AppHandle,
    apply: bool,
    cancel: CancellationToken,
    asking: Arc<Mutex<Option<Question>>>,
    next_id: Arc<AtomicU64>,
) -> anyhow::Result<bool> {
    let store = open_store()?;
    let items = store.pending()?;
    let library = RunbookLibrary::load(runbook_dir().as_deref())?;
    let platform = ork_core::platform::detect()?.kind().to_string();
    // The engine is given the same token the window's Stop button holds, so
    // cancelling stops it after the current step rather than in the middle of
    // one -- and without depending on a task being scheduled to pass the
    // message along.
    let engine = FixEngine::new(open_store()?, state_dir()?.join("snapshots"))
        .with_cancel_token(cancel.clone());
    let verifiers = VerifierRegistry::standard();

    let emit = |event: FixEvent| {
        if let Err(error) = app.emit("fix://event", &event) {
            tracing::debug!(%error, "could not deliver a fix event");
        }
    };

    let support = detect_system_snapshot_support();
    emit(FixEvent::Started {
        total: items.len(),
        testable: verifiers.coverage(&items),
        apply,
        snapshot_warning: (!support.available).then(|| support.detail.clone()),
    });

    let approver: Box<dyn Approver> = if apply {
        let window = app.clone();
        Box::new(AskTheWindow {
            announce: Box::new(move |ask| match window.emit("fix://ask", ask) {
                Ok(()) => true,
                Err(error) => {
                    tracing::warn!(%error, "could not ask for permission; declining");
                    false
                }
            }),
            asking,
            cancel: cancel.clone(),
            next_id,
        })
    } else {
        // The same path, simply never given permission, which is what makes a
        // dry run real rather than a flag somebody has to remember to check.
        Box::new(DryRun)
    };

    let mut resolved = 0;
    let mut stopped = false;
    for (index, item) in items.iter().enumerate() {
        emit(FixEvent::Item {
            index: index + 1,
            total: items.len(),
            occurrence_key: item.occurrence_key.clone(),
            title: item.title.clone(),
            severity: item.severity.to_string(),
        });

        let outcome = engine
            .work_item(
                item,
                candidates_for(item, &library, &platform),
                verifiers.for_item(item),
                approver.as_ref(),
            )
            .await?;

        if matches!(outcome, ItemOutcome::Resolved { .. }) {
            resolved += 1;
        }
        stopped = matches!(outcome, ItemOutcome::Stopped);
        emit(FixEvent::Outcome {
            occurrence_key: item.occurrence_key.clone(),
            outcome,
        });
        if stopped {
            break;
        }
    }

    emit(FixEvent::Finished { resolved, stopped });
    Ok(stopped)
}

/// Answer the question the window is currently showing.
///
/// Anything other than a plain "approve" is a refusal. An answer whose id does
/// not match the open question is discarded, so a stale click cannot approve
/// something the user never saw.
#[tauri::command]
pub fn fix_answer(state: State<'_, AppState>, id: u64, answer: String) -> CmdResult<bool> {
    let asking = state
        .fix
        .asking
        .lock()
        .map_err(|_| "the fix lock was poisoned".to_string())?;
    Ok(deliver(asking, id, approval_from(&answer)))
}

/// What an answer from the window means.
///
/// Only the exact word "approve" is consent. Everything else -- a typo, a
/// value from a future version of the window, an empty string -- declines,
/// because the failure mode of guessing generously is changing someone's
/// machine without being asked to.
fn approval_from(answer: &str) -> Approval {
    match answer {
        "approve" => Approval::Approve,
        "stop" => Approval::StopEverything,
        _ => Approval::Decline,
    }
}

/// Hand an answer to the question it names, if that question is still open.
///
/// Returns whether it was delivered. An id that does not match is dropped: a
/// click that arrives after the question moved on must never be able to
/// approve a change nobody was shown.
fn deliver(
    mut asking: std::sync::MutexGuard<'_, Option<Question>>,
    id: u64,
    approval: Approval,
) -> bool {
    match asking.as_ref() {
        Some(question) if question.id == id => {
            let question = asking.take().expect("the question was just matched");
            question.answer.send(approval).is_ok()
        }
        Some(_) => {
            tracing::warn!(id, "an answer arrived for a question no longer open");
            false
        }
        None => false,
    }
}

/// Stop the run. Always available, never automatic.
#[tauri::command]
pub fn fix_cancel(state: State<'_, AppState>) -> CmdResult<bool> {
    let slot = state
        .fix
        .running
        .lock()
        .map_err(|_| "the fix lock was poisoned".to_string())?;
    match slot.as_ref() {
        Some(cancel) => {
            cancel.cancel();
            // An open question is released too, so a run waiting on an answer
            // stops now rather than when someone finally clicks.
            if let Ok(mut asking) = state.fix.asking.lock() {
                if let Some(question) = asking.take() {
                    let _ = question.answer.send(Approval::StopEverything);
                }
            }
            Ok(true)
        }
        None => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open_question(id: u64) -> (Mutex<Option<Question>>, Receiver<Approval>) {
        let (answer, receiver) = channel();
        (Mutex::new(Some(Question { id, answer })), receiver)
    }

    #[test]
    fn only_the_word_approve_is_consent() {
        assert!(matches!(approval_from("approve"), Approval::Approve));
        assert!(matches!(approval_from("stop"), Approval::StopEverything));
        for answer in ["decline", "Approve", "yes", "y", "", "true", "1"] {
            assert!(
                matches!(approval_from(answer), Approval::Decline),
                "{answer:?} must not be read as permission"
            );
        }
    }

    #[test]
    fn an_answer_reaches_the_question_it_names() {
        let (asking, receiver) = open_question(7);
        assert!(deliver(asking.lock().unwrap(), 7, Approval::Approve));
        assert!(matches!(receiver.try_recv(), Ok(Approval::Approve)));
        assert!(asking.lock().unwrap().is_none(), "the question is closed");
    }

    #[test]
    fn an_answer_for_a_different_question_is_discarded() {
        // The dangerous version of this bug approves the change now on screen
        // using a click meant for the previous one.
        let (asking, receiver) = open_question(7);
        assert!(!deliver(asking.lock().unwrap(), 6, Approval::Approve));
        assert!(receiver.try_recv().is_err(), "nothing was delivered");
        assert!(
            asking.lock().unwrap().is_some(),
            "the real question is still waiting"
        );
    }

    #[test]
    fn a_question_can_only_be_answered_once() {
        let (asking, receiver) = open_question(7);
        assert!(deliver(asking.lock().unwrap(), 7, Approval::Decline));
        assert!(!deliver(asking.lock().unwrap(), 7, Approval::Approve));
        assert!(matches!(receiver.try_recv(), Ok(Approval::Decline)));
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn an_answer_with_nothing_waiting_goes_nowhere() {
        let asking: Mutex<Option<Question>> = Mutex::new(None);
        assert!(!deliver(asking.lock().unwrap(), 1, Approval::Approve));
    }

    #[test]
    fn asking_waits_for_the_answer_and_then_returns_it() {
        // The whole round trip: the approver parks, an answer arrives from
        // elsewhere, and the run continues with what the person actually said.
        let asking = Arc::new(Mutex::new(None));
        let approver = AskTheWindow {
            announce: Box::new(|_| true),
            asking: asking.clone(),
            cancel: CancellationToken::new(),
            next_id: Arc::new(AtomicU64::new(0)),
        };

        let answerer = {
            let asking = asking.clone();
            std::thread::spawn(move || {
                loop {
                    let guard = asking.lock().unwrap();
                    if let Some(question) = guard.as_ref() {
                        let id = question.id;
                        assert!(deliver(guard, id, Approval::Approve));
                        return;
                    }
                    drop(guard);
                    std::thread::sleep(Duration::from_millis(5));
                }
            })
        };

        let action = FixAction::Manual {
            instruction: "an example".to_string(),
        };
        let item = waiting_item();
        assert!(matches!(
            approver.approve(&action, &item),
            Approval::Approve
        ));
        answerer.join().unwrap();
        assert!(
            asking.lock().unwrap().is_none(),
            "the question is cleared once answered"
        );
    }

    fn waiting_item() -> TriageItem {
        TriageItem {
            id: 1,
            occurrence_key: "example".to_string(),
            finding_id: "example.problem".to_string(),
            subject: None,
            severity: ork_core::Severity::Low,
            title: "An example problem".to_string(),
            finding: ork_core::Finding::builder("example", "example.problem")
                .severity(ork_core::Severity::Low)
                .category(ork_core::finding::Category::Configuration)
                .title("An example problem")
                .detail("for the test")
                .build(),
            state: ork_fix::store::ItemState::Pending,
            attempts: 0,
        }
    }

    #[test]
    fn a_closed_window_declines_rather_than_waiting() {
        // park() sees the sender dropped and must not spin for ever.
        let (sender, receiver) = channel::<Approval>();
        drop(sender);
        let asking = Arc::new(Mutex::new(None));
        let approver = AskTheWindow {
            announce: Box::new(|_| true),
            asking,
            cancel: CancellationToken::new(),
            next_id: Arc::new(AtomicU64::new(0)),
        };
        assert!(matches!(approver.park(1, receiver), Approval::Decline));
    }

    #[test]
    fn a_cancelled_run_stops_a_question_that_is_still_waiting() {
        let (_sender, receiver) = channel::<Approval>();
        let cancel = CancellationToken::new();
        cancel.cancel();
        let approver = AskTheWindow {
            announce: Box::new(|_| true),
            asking: Arc::new(Mutex::new(None)),
            cancel,
            next_id: Arc::new(AtomicU64::new(0)),
        };
        assert!(matches!(
            approver.park(1, receiver),
            Approval::StopEverything
        ));
    }
}
