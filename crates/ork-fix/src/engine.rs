//! The fix-attempt loop.
//!
//! For each queued problem, working through candidates least invasive first:
//! snapshot, apply one change, test whether it worked, roll back if it did
//! not, then try the next candidate. It keeps going until something works or
//! the list is exhausted. Nothing here has a deadline.
//!
//! Two rules shape the design more than anything else.
//!
//! **A change is only applied if its result can be tested.** "One change at a
//! time, always testable and reversible" is not satisfied by a change whose
//! effect nobody can measure. So an action with no verifier is never applied
//! automatically -- it is presented as advice for a person to carry out and
//! judge. This is restrictive on purpose, and it is why driver installs and
//! package changes arrive as instructions rather than actions.
//!
//! **"I could not tell" is not "it worked."** A verifier that cannot determine
//! the outcome causes a rollback, not a shrug. Keeping an unverified change
//! would mean the tool had modified a machine and could not say whether it
//! helped, which is exactly the state this loop exists to avoid.

use std::path::PathBuf;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::Result;
use crate::action::FixAction;
use crate::snapshot::Snapshot;
use crate::store::{AttemptOutcome, FixStore, ItemState, TriageItem};

/// Whether a fix worked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Tested, and the problem is gone.
    Fixed,
    /// Tested, and the problem is still there.
    StillBroken { detail: String },
    /// The test could not be carried out, so nothing is known.
    ///
    /// Treated as failure, because an unverified change to someone's machine
    /// is not an improvement.
    CannotTell { reason: String },
}

/// Something that can tell whether a problem is still present.
#[async_trait]
pub trait Verifier: Send + Sync {
    /// Re-test the specific problem this item describes.
    async fn verify(&self, item: &TriageItem) -> Verdict;
}

/// What the user said about applying a change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Approval {
    /// Go ahead with this one.
    Approve,
    /// Skip this one, try the next candidate.
    Decline,
    /// Stop working the queue entirely.
    StopEverything,
}

/// Asks permission before anything is changed.
///
/// A trait so the same engine serves a terminal prompt, a desktop dialog, and
/// a dry run -- and so that "ask first" is structurally impossible to skip.
pub trait Approver: Send + Sync {
    fn approve(&self, action: &FixAction, item: &TriageItem) -> Approval;
}

/// An approver that never approves anything.
///
/// This is what makes `--dry-run` real rather than a flag that code has to
/// remember to check: the engine takes the same path and simply never gets
/// permission, so it reports exactly what it would have done.
pub struct DryRun;

impl Approver for DryRun {
    fn approve(&self, _action: &FixAction, _item: &TriageItem) -> Approval {
        Approval::Decline
    }
}

/// How working one item turned out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "kebab-case")]
pub enum ItemOutcome {
    /// Something worked, and the test confirmed it.
    Resolved { action: String },
    /// Every candidate was tried and none worked.
    Exhausted { tried: usize },
    /// Nothing could be attempted automatically; what to do is described.
    NeedsAPerson { instructions: Vec<String> },
    /// The user stopped it.
    Stopped,
    /// There was nothing to try.
    NoCandidates,
}

/// Applies actions and works the queue.
pub struct FixEngine {
    store: FixStore,
    snapshot_root: PathBuf,
    cancel: CancellationToken,
}

impl FixEngine {
    pub fn new(store: FixStore, snapshot_root: PathBuf) -> Self {
        Self {
            store,
            snapshot_root,
            cancel: CancellationToken::new(),
        }
    }

    pub fn store(&self) -> &FixStore {
        &self.store
    }

    pub fn cancel_token(&self) -> CancellationToken {
        self.cancel.clone()
    }

    /// Carry out one action, having already been given permission.
    ///
    /// The snapshot is taken here rather than by the caller, so there is no
    /// path through this code that changes a file without first copying it.
    fn apply(&self, action: &FixAction, snapshot: &mut Snapshot) -> Result<()> {
        action.ensure_permitted()?;

        match action {
            FixAction::RemoveStaleFile { path, .. } => {
                snapshot.capture(path)?;
                match std::fs::remove_file(path) {
                    Ok(()) => Ok(()),
                    // Already gone is the desired end state.
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                    Err(error) => Err(error.into()),
                }
            }
            FixAction::RestartService { service } => {
                let (program, args) = if cfg!(windows) {
                    (
                        "powershell",
                        vec![
                            "-NoProfile".to_string(),
                            "-NonInteractive".to_string(),
                            "-Command".to_string(),
                            format!("Restart-Service -Name '{service}' -ErrorAction Stop"),
                        ],
                    )
                } else {
                    ("systemctl", vec!["restart".to_string(), service.clone()])
                };

                let output = std::process::Command::new(program)
                    .args(&args)
                    .output()
                    .map_err(|error| anyhow::anyhow!("could not run {program}: {error}"))?;

                if output.status.success() {
                    Ok(())
                } else {
                    // Almost always a permissions problem. Saying so is more
                    // useful than a bare exit code, because the answer is to
                    // re-run with the rights rather than to try something else.
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    anyhow::bail!(
                        "could not restart `{service}`: {}. This usually needs administrator \
                         or root rights, which this process does not have.",
                        stderr.trim()
                    )
                }
            }
            // Neither of these changes anything, so neither reaches here.
            FixAction::Inspect { .. } | FixAction::Manual { .. } => {
                anyhow::bail!("this action does not change anything and should not be applied")
            }
        }
    }

    /// Order candidates: what has worked before first, then least invasive,
    /// with anything already tried and failed dropped.
    fn plan(&self, item: &TriageItem, candidates: Vec<FixAction>) -> Result<Vec<FixAction>> {
        let already_failed = self.store.already_failed(&item.occurrence_key)?;
        let known_good = self.store.known_good(&item.finding_id)?;

        let mut plan: Vec<FixAction> = candidates
            .into_iter()
            .filter(|action| !already_failed.contains(&action.describe()))
            .collect();

        plan.sort_by_key(|action| {
            // Something that has fixed this before on this machine goes first,
            // so a repeat problem is resolved on the first try rather than by
            // working down the whole list again.
            let proven = if known_good.contains(&action.describe()) {
                0
            } else {
                1
            };
            (proven, action.risk())
        });

        Ok(plan)
    }
}

impl FixEngine {
    /// Work one problem through its candidate fixes.
    ///
    /// There is no time limit. The loop ends when a fix is confirmed to have
    /// worked, when the candidates run out, or when the user stops it.
    pub async fn work_item(
        &self,
        item: &TriageItem,
        candidates: Vec<FixAction>,
        verifier: Option<&dyn Verifier>,
        approver: &dyn Approver,
    ) -> Result<ItemOutcome> {
        let plan = self.plan(item, candidates)?;
        if plan.is_empty() {
            return Ok(ItemOutcome::NoCandidates);
        }

        let mut manual = Vec::new();
        let mut attempted = 0usize;

        for action in plan {
            if self.cancel.is_cancelled() {
                return Ok(ItemOutcome::Stopped);
            }

            // The safety rules are checked again here, not only when the
            // candidate was built. Whatever produced this action -- a reviewed
            // runbook or a model -- it does not get to bypass validation.
            if let Err(refusal) = action.validate() {
                tracing::warn!(action = action.describe(), %refusal, "refused");
                self.store.record_attempt(
                    &item.occurrence_key,
                    &item.finding_id,
                    &action,
                    AttemptOutcome::Refused,
                    Some(&refusal.to_string()),
                    None,
                )?;
                continue;
            }

            match &action {
                // Advice. The tool describes it and moves on; it cannot carry
                // it out and must not pretend to have.
                FixAction::Manual { instruction } => {
                    manual.push(instruction.clone());
                    self.store.record_attempt(
                        &item.occurrence_key,
                        &item.finding_id,
                        &action,
                        AttemptOutcome::NeedsAPerson,
                        None,
                        None,
                    )?;
                    continue;
                }
                // Gathers information. Recorded, but it cannot resolve
                // anything, so the loop continues either way.
                FixAction::Inspect { .. } => {
                    self.store.record_attempt(
                        &item.occurrence_key,
                        &item.finding_id,
                        &action,
                        AttemptOutcome::NeedsAPerson,
                        Some("inspection only"),
                        None,
                    )?;
                    manual.push(action.describe());
                    continue;
                }
                _ => {}
            }

            // A change with no way to test it is not applied. "Always
            // testable" is a requirement, not an aspiration.
            let Some(verifier) = verifier else {
                tracing::debug!(
                    action = action.describe(),
                    "no verifier for this problem; offering as advice instead of applying"
                );
                manual.push(action.describe());
                self.store.record_attempt(
                    &item.occurrence_key,
                    &item.finding_id,
                    &action,
                    AttemptOutcome::NeedsAPerson,
                    Some("no way to test the result, so it was not applied automatically"),
                    None,
                )?;
                continue;
            };

            match approver.approve(&action, item) {
                Approval::Decline => {
                    self.store.record_attempt(
                        &item.occurrence_key,
                        &item.finding_id,
                        &action,
                        AttemptOutcome::NeedsAPerson,
                        Some("not approved"),
                        None,
                    )?;
                    continue;
                }
                Approval::StopEverything => return Ok(ItemOutcome::Stopped),
                Approval::Approve => {}
            }

            attempted += 1;
            let snapshot_id = format!("{}-{}", item.id, attempted);
            let mut snapshot =
                Snapshot::create(&self.snapshot_root, &snapshot_id, &action.describe())?;

            if let Err(error) = self.apply(&action, &mut snapshot) {
                tracing::warn!(action = action.describe(), %error, "could not apply");
                // The change may have partly happened, so put things back
                // before trying anything else.
                let _ = snapshot.restore();
                self.store.record_attempt(
                    &item.occurrence_key,
                    &item.finding_id,
                    &action,
                    AttemptOutcome::Failed,
                    Some(&format!("{error:#}")),
                    Some(&snapshot_id),
                )?;
                continue;
            }

            match verifier.verify(item).await {
                Verdict::Fixed => {
                    self.store.record_attempt(
                        &item.occurrence_key,
                        &item.finding_id,
                        &action,
                        AttemptOutcome::Succeeded,
                        None,
                        Some(&snapshot_id),
                    )?;
                    self.store
                        .set_state(&item.occurrence_key, ItemState::Resolved)?;
                    return Ok(ItemOutcome::Resolved {
                        action: action.describe(),
                    });
                }
                Verdict::StillBroken { detail } => {
                    snapshot.restore()?;
                    self.store.record_attempt(
                        &item.occurrence_key,
                        &item.finding_id,
                        &action,
                        AttemptOutcome::RolledBack,
                        Some(&detail),
                        Some(&snapshot_id),
                    )?;
                }
                Verdict::CannotTell { reason } => {
                    // Not knowing is not success. Rolling back keeps the
                    // machine in a state the tool can describe honestly.
                    snapshot.restore()?;
                    self.store.record_attempt(
                        &item.occurrence_key,
                        &item.finding_id,
                        &action,
                        AttemptOutcome::RolledBack,
                        Some(&format!("could not tell whether this worked: {reason}")),
                        Some(&snapshot_id),
                    )?;
                }
            }
        }

        if attempted == 0 && !manual.is_empty() {
            return Ok(ItemOutcome::NeedsAPerson {
                instructions: manual,
            });
        }

        self.store
            .set_state(&item.occurrence_key, ItemState::Exhausted)?;
        Ok(ItemOutcome::Exhausted { tried: attempted })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ork_core::Finding;
    use ork_core::finding::Severity;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A verifier that returns a scripted sequence of verdicts.
    struct ScriptedVerifier {
        verdicts: Mutex<Vec<Verdict>>,
        calls: AtomicUsize,
    }

    impl ScriptedVerifier {
        fn new(verdicts: Vec<Verdict>) -> Self {
            Self {
                verdicts: Mutex::new(verdicts),
                calls: AtomicUsize::new(0),
            }
        }
    }

    #[async_trait]
    impl Verifier for ScriptedVerifier {
        async fn verify(&self, _item: &TriageItem) -> Verdict {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let mut verdicts = self.verdicts.lock().unwrap();
            if verdicts.is_empty() {
                Verdict::StillBroken {
                    detail: "no more verdicts".to_string(),
                }
            } else {
                verdicts.remove(0)
            }
        }
    }

    struct AlwaysApprove;
    impl Approver for AlwaysApprove {
        fn approve(&self, _action: &FixAction, _item: &TriageItem) -> Approval {
            Approval::Approve
        }
    }

    struct Harness {
        dir: PathBuf,
        engine: FixEngine,
        item: TriageItem,
        lock_path: PathBuf,
    }

    fn harness(name: &str) -> Harness {
        let dir = std::env::temp_dir().join(format!("ork-engine-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let lock_path = dir.join("steam.lock");
        std::fs::write(&lock_path, "stale lock").unwrap();

        let store = FixStore::in_memory().unwrap();
        let finding = Finding::builder("apps.launch-check", "app.launch-hung")
            .subject("steam")
            .severity(Severity::High)
            .title("Steam hangs instead of starting")
            .detail("it never finishes starting")
            .build();
        store.enqueue(&finding).unwrap();
        let item = store.pending().unwrap().remove(0);

        let engine = FixEngine::new(store, dir.join("snapshots"));
        Harness {
            dir,
            engine,
            item,
            lock_path,
        }
    }

    fn remove_lock(path: &std::path::Path) -> FixAction {
        FixAction::RemoveStaleFile {
            path: path.to_path_buf(),
            reason: "left behind by a crash".to_string(),
        }
    }

    #[tokio::test]
    async fn a_fix_that_works_is_kept_and_the_item_is_resolved() {
        let h = harness("success");
        let verifier = ScriptedVerifier::new(vec![Verdict::Fixed]);

        let outcome = h
            .engine
            .work_item(
                &h.item,
                vec![remove_lock(&h.lock_path)],
                Some(&verifier),
                &AlwaysApprove,
            )
            .await
            .unwrap();

        assert!(matches!(outcome, ItemOutcome::Resolved { .. }));
        assert!(!h.lock_path.exists(), "the fix should have taken effect");
        assert!(
            h.engine.store().pending().unwrap().is_empty(),
            "the item should be resolved"
        );

        let _ = std::fs::remove_dir_all(&h.dir);
    }

    #[tokio::test]
    async fn a_fix_that_does_not_work_is_rolled_back_completely() {
        // The file must come back. This is the whole promise of the loop.
        let h = harness("rollback");
        let verifier = ScriptedVerifier::new(vec![Verdict::StillBroken {
            detail: "same error".to_string(),
        }]);

        let outcome = h
            .engine
            .work_item(
                &h.item,
                vec![remove_lock(&h.lock_path)],
                Some(&verifier),
                &AlwaysApprove,
            )
            .await
            .unwrap();

        assert!(matches!(outcome, ItemOutcome::Exhausted { .. }));
        assert!(h.lock_path.exists(), "the removed file must be restored");
        assert_eq!(std::fs::read_to_string(&h.lock_path).unwrap(), "stale lock");

        let _ = std::fs::remove_dir_all(&h.dir);
    }

    #[tokio::test]
    async fn an_unverifiable_change_is_rolled_back_rather_than_kept() {
        // Not knowing whether a change helped is not the same as it having
        // helped. The machine goes back to a state the tool can describe.
        let h = harness("cannot-tell");
        let verifier = ScriptedVerifier::new(vec![Verdict::CannotTell {
            reason: "the test could not run".to_string(),
        }]);

        h.engine
            .work_item(
                &h.item,
                vec![remove_lock(&h.lock_path)],
                Some(&verifier),
                &AlwaysApprove,
            )
            .await
            .unwrap();

        assert!(
            h.lock_path.exists(),
            "an unverified change must not be left in place"
        );
        let attempts = h
            .engine
            .store()
            .attempts_for(&h.item.occurrence_key)
            .unwrap();
        assert_eq!(attempts[0].outcome, AttemptOutcome::RolledBack);
        assert!(
            attempts[0]
                .detail
                .as_ref()
                .unwrap()
                .contains("could not tell")
        );

        let _ = std::fs::remove_dir_all(&h.dir);
    }

    #[tokio::test]
    async fn a_dry_run_changes_nothing_at_all() {
        let h = harness("dry-run");
        let verifier = ScriptedVerifier::new(vec![Verdict::Fixed]);

        h.engine
            .work_item(
                &h.item,
                vec![remove_lock(&h.lock_path)],
                Some(&verifier),
                &DryRun,
            )
            .await
            .unwrap();

        assert!(h.lock_path.exists(), "a dry run must not touch anything");
        assert_eq!(
            verifier.calls.load(Ordering::SeqCst),
            0,
            "nothing should have been tested"
        );

        let _ = std::fs::remove_dir_all(&h.dir);
    }

    #[tokio::test]
    async fn nothing_is_applied_when_there_is_no_way_to_test_the_result() {
        // "One change at a time, always testable" is a requirement. A change
        // whose effect cannot be measured is offered as advice instead.
        let h = harness("no-verifier");

        let outcome = h
            .engine
            .work_item(
                &h.item,
                vec![remove_lock(&h.lock_path)],
                None,
                &AlwaysApprove,
            )
            .await
            .unwrap();

        assert!(matches!(outcome, ItemOutcome::NeedsAPerson { .. }));
        assert!(h.lock_path.exists(), "nothing should have been changed");

        let _ = std::fs::remove_dir_all(&h.dir);
    }

    #[tokio::test]
    async fn an_unsafe_candidate_is_refused_even_though_it_reached_the_engine() {
        // Validation runs again here. Whatever produced the action -- a
        // reviewed runbook or a model -- it does not get to bypass the rules.
        let h = harness("refusal");
        let hostile = FixAction::RemoveStaleFile {
            path: PathBuf::from("/etc/passwd.lock"),
            reason: "definitely stale".to_string(),
        };
        let verifier = ScriptedVerifier::new(vec![Verdict::Fixed]);

        let outcome = h
            .engine
            .work_item(&h.item, vec![hostile], Some(&verifier), &AlwaysApprove)
            .await
            .unwrap();

        assert!(matches!(outcome, ItemOutcome::Exhausted { tried: 0 }));
        let attempts = h
            .engine
            .store()
            .attempts_for(&h.item.occurrence_key)
            .unwrap();
        assert_eq!(attempts[0].outcome, AttemptOutcome::Refused);

        let _ = std::fs::remove_dir_all(&h.dir);
    }

    #[tokio::test]
    async fn candidates_are_tried_in_turn_until_one_works() {
        let h = harness("sequence");
        let second_lock = h.dir.join("other.lock");
        std::fs::write(&second_lock, "second").unwrap();

        // The first is rolled back, the second succeeds.
        let verifier = ScriptedVerifier::new(vec![
            Verdict::StillBroken {
                detail: "still hanging".to_string(),
            },
            Verdict::Fixed,
        ]);

        let outcome = h
            .engine
            .work_item(
                &h.item,
                vec![remove_lock(&h.lock_path), remove_lock(&second_lock)],
                Some(&verifier),
                &AlwaysApprove,
            )
            .await
            .unwrap();

        assert!(matches!(outcome, ItemOutcome::Resolved { .. }));
        assert!(
            h.lock_path.exists(),
            "the failed candidate must have been undone"
        );
        assert!(
            !second_lock.exists(),
            "the successful candidate should stand"
        );
        assert_eq!(verifier.calls.load(Ordering::SeqCst), 2);

        let _ = std::fs::remove_dir_all(&h.dir);
    }

    #[tokio::test]
    async fn a_candidate_that_already_failed_is_not_tried_again() {
        let h = harness("no-repeat");
        let action = remove_lock(&h.lock_path);
        h.engine
            .store()
            .record_attempt(
                &h.item.occurrence_key,
                &h.item.finding_id,
                &action,
                AttemptOutcome::RolledBack,
                None,
                None,
            )
            .unwrap();

        let verifier = ScriptedVerifier::new(vec![Verdict::Fixed]);
        let outcome = h
            .engine
            .work_item(&h.item, vec![action], Some(&verifier), &AlwaysApprove)
            .await
            .unwrap();

        assert_eq!(outcome, ItemOutcome::NoCandidates);
        assert_eq!(verifier.calls.load(Ordering::SeqCst), 0);

        let _ = std::fs::remove_dir_all(&h.dir);
    }

    #[tokio::test]
    async fn stopping_ends_the_loop_immediately() {
        let h = harness("stop");
        struct StopNow;
        impl Approver for StopNow {
            fn approve(&self, _: &FixAction, _: &TriageItem) -> Approval {
                Approval::StopEverything
            }
        }

        let verifier = ScriptedVerifier::new(vec![Verdict::Fixed]);
        let outcome = h
            .engine
            .work_item(
                &h.item,
                vec![remove_lock(&h.lock_path)],
                Some(&verifier),
                &StopNow,
            )
            .await
            .unwrap();

        assert_eq!(outcome, ItemOutcome::Stopped);
        assert!(h.lock_path.exists());

        let _ = std::fs::remove_dir_all(&h.dir);
    }
}
