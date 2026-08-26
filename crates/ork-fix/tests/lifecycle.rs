//! The whole path a problem takes: found by a scan, queued, worked, recorded.
//!
//! The unit tests cover each stage. This covers the joins between them, which
//! is where wiring mistakes live.

use std::path::PathBuf;

use async_trait::async_trait;
use ork_core::Finding;
use ork_core::finding::{Category, Severity, Triage};
use ork_fix::action::FixAction;
use ork_fix::engine::{Approval, Approver, FixEngine, ItemOutcome, Verdict, Verifier};
use ork_fix::store::{AttemptOutcome, FixStore, ItemState};

struct AlwaysApprove;
impl Approver for AlwaysApprove {
    fn approve(&self, _: &FixAction, _: &ork_fix::store::TriageItem) -> Approval {
        Approval::Approve
    }
}

/// Reports the problem as fixed once the lock file is gone -- which is what a
/// real verifier for this problem would check.
struct LockFileGone {
    path: PathBuf,
}

#[async_trait]
impl Verifier for LockFileGone {
    async fn verify(&self, _item: &ork_fix::store::TriageItem) -> Verdict {
        if self.path.exists() {
            Verdict::StillBroken {
                detail: "the lock file is still there".to_string(),
            }
        } else {
            Verdict::Fixed
        }
    }
}

fn finding(id: &str, subject: &str, severity: Severity, triage: Triage) -> Finding {
    Finding::builder("test.probe", id)
        .subject(subject)
        .severity(severity)
        .category(Category::Application)
        .title(format!("{id} affecting {subject}"))
        .detail("something is wrong")
        .triage(triage)
        .build()
}

#[tokio::test]
async fn a_problem_is_queued_worked_fixed_and_recorded() {
    let dir = std::env::temp_dir().join(format!("ork-lifecycle-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let lock = dir.join("steam.lock");
    std::fs::write(&lock, "stale").unwrap();

    let store = FixStore::in_memory().unwrap();

    // A scan produces a mix. Only what is marked for triage should be queued;
    // queueing everything would bury the items that need working through.
    let queued = finding("app.launch-hung", "steam", Severity::High, Triage::Queue);
    let inline = finding(
        "storage.volume-low-on-space",
        "C:",
        Severity::Medium,
        Triage::Inline,
    );
    let informational = finding("process.memory-hog", "vm", Severity::Low, Triage::None);

    for candidate in [&queued, &inline, &informational] {
        if candidate.triage == Triage::Queue {
            store.enqueue(candidate).unwrap();
        }
    }

    let pending = store.pending().unwrap();
    assert_eq!(
        pending.len(),
        1,
        "only triage-queue findings belong on the queue"
    );
    let item = &pending[0];
    assert_eq!(item.finding_id, "app.launch-hung");

    // Work it. The first candidate is refused by the safety rules, the second
    // works -- so this covers refusal and success in one pass.
    let engine = FixEngine::new(store, dir.join("snapshots"));
    let verifier = LockFileGone { path: lock.clone() };

    let outcome = engine
        .work_item(
            item,
            vec![
                FixAction::RemoveStaleFile {
                    path: PathBuf::from("/etc/shadow.lock"),
                    reason: "not really".to_string(),
                },
                FixAction::RemoveStaleFile {
                    path: lock.clone(),
                    reason: "left behind by a crash".to_string(),
                },
            ],
            Some(&verifier),
            &AlwaysApprove,
        )
        .await
        .unwrap();

    assert!(
        matches!(outcome, ItemOutcome::Resolved { .. }),
        "got {outcome:?}"
    );
    assert!(!lock.exists(), "the fix should have taken effect");

    // The item is done and does not come back.
    let store = engine.store();
    assert!(store.pending().unwrap().is_empty());
    assert_eq!(store.all().unwrap()[0].state, ItemState::Resolved);
    assert!(
        !store.enqueue(&queued).unwrap(),
        "a resolved item must not be re-queued"
    );

    // Everything is on the record: the refusal and the success both.
    let attempts = store.attempts_for(&item.occurrence_key).unwrap();
    assert_eq!(attempts.len(), 2);
    assert_eq!(attempts[0].outcome, AttemptOutcome::Refused);
    assert_eq!(attempts[1].outcome, AttemptOutcome::Succeeded);

    // And what worked is remembered, so a repeat is fixed on the first try.
    let known_good = store.known_good("app.launch-hung").unwrap();
    assert_eq!(known_good.len(), 1);
    assert!(known_good[0].contains("steam.lock"));

    let audit: Vec<String> = store
        .audit_log(50)
        .unwrap()
        .into_iter()
        .map(|entry| entry.kind)
        .collect();
    for expected in ["queued", "attempt", "state-change"] {
        assert!(
            audit.contains(&expected.to_string()),
            "audit log is missing `{expected}`"
        );
    }

    let _ = std::fs::remove_dir_all(&dir);
}

/// The real registry, exercised end to end: a stale lock file is removed, the
/// shipped verifier confirms it is gone, and the item is recorded as resolved.
///
/// The point of this test is that nothing here is a stand-in. The verifier is
/// the one the tool ships with, the file is a real file on disk, and the
/// engine is asked for a verifier the same way the command line asks.
#[tokio::test]
async fn a_stale_lock_is_removed_and_the_shipped_verifier_confirms_it() {
    use ork_fix::verify::VerifierRegistry;

    let dir = std::env::temp_dir().join(format!("ork-verify-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let lock = dir.join("steam.pid");
    std::fs::write(&lock, "12345").unwrap();

    let store = FixStore::in_memory().unwrap();
    let problem = Finding::builder("apps.launcher-check", "app.launch-stale-lock")
        .subject("steam")
        .severity(Severity::High)
        .category(Category::Application)
        .title("Steam left a lock behind")
        .detail("a previous session did not shut down cleanly")
        .evidence("path", lock.display().to_string())
        .triage(Triage::Queue)
        .build();
    store.enqueue(&problem).unwrap();

    let item = store.pending().unwrap().into_iter().next().expect("queued");
    let registry = VerifierRegistry::standard();
    let verifier = registry
        .for_item(&item)
        .expect("a stale-file problem must be testable");

    let engine = FixEngine::new(store, dir.join("snapshots"));
    let action = FixAction::RemoveStaleFile {
        path: lock.clone(),
        reason: "left behind by a previous session".to_string(),
    };

    let outcome = engine
        .work_item(&item, vec![action], Some(verifier), &AlwaysApprove)
        .await
        .unwrap();

    assert!(
        matches!(outcome, ItemOutcome::Resolved { .. }),
        "got {outcome:?}"
    );
    assert!(!lock.exists(), "the lock file is still there");

    let state = engine
        .store()
        .all()
        .unwrap()
        .into_iter()
        .find(|entry| entry.occurrence_key == item.occurrence_key)
        .expect("the item is still recorded");
    assert_eq!(state.state, ItemState::Resolved);

    let _ = std::fs::remove_dir_all(&dir);
}

/// A problem the tool cannot re-test is never acted on, however confident a
/// runbook is about the answer.
#[tokio::test]
async fn a_problem_with_no_verifier_is_left_alone() {
    use ork_fix::verify::VerifierRegistry;

    let registry = VerifierRegistry::standard();
    let problem = finding("memory.pressure", "system", Severity::High, Triage::Queue);
    let store = FixStore::in_memory().unwrap();
    store.enqueue(&problem).unwrap();
    let item = store.pending().unwrap().into_iter().next().unwrap();

    assert!(
        registry.for_item(&item).is_none(),
        "claiming to test this would make the engine change the machine and undo it again"
    );
    assert_eq!(registry.coverage([&item]), 0);
}
