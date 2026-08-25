//! Deciding what a launch test means.
//!
//! The test itself lives in [`ork_core::launch`], because the check that finds
//! a broken application and the check that later declares it repaired have to
//! be the same code. What lives here is the judgement: given what was
//! observed, is the problem gone, still there, or unknowable?
//!
//! The unknowable case is the one worth writing down. A launcher that exits
//! straight away with success has not told us it works -- it may have handed
//! off to something that quit, or done nothing at all. Reading that as "fixed"
//! would be the single easiest way for this tool to tell somebody their
//! problem is solved when it is not, so it is read as "cannot tell", and the
//! engine puts the machine back as it found it.

use async_trait::async_trait;
use ork_core::launch::{LAUNCHERS, LaunchResult, LaunchTarget, LaunchTester};

use crate::engine::{Verdict, Verifier};
use crate::store::TriageItem;
use crate::verify::{ItemVerifier, excerpt, this_platform};

/// Re-tests an application launch, and decides what the result means.
pub struct LaunchVerifier<T: LaunchTester> {
    tester: T,
}

impl<T: LaunchTester> LaunchVerifier<T> {
    pub fn new(tester: T) -> Self {
        Self { tester }
    }
}

/// The application a finding is about, if this build can test it.
pub fn target_for(item: &TriageItem) -> Option<&'static LaunchTarget> {
    if !matches!(
        item.finding_id.as_str(),
        "app.launch-failed" | "app.launch-hung"
    ) {
        return None;
    }
    let subject = item.subject.as_deref()?.to_ascii_lowercase();
    let platform = this_platform();
    LAUNCHERS
        .iter()
        .find(|target| target.id == subject && target.runs_on(platform))
}

/// Turn what was observed into a verdict.
///
/// Separated from the process work because this is where the judgement lives,
/// and judgement is worth testing without starting anything.
pub fn verdict_for(target: &LaunchTarget, result: LaunchResult) -> Verdict {
    match result {
        LaunchResult::Started => Verdict::Fixed,
        LaunchResult::Failed { code, output } => {
            let detail = match (code, output.trim().is_empty()) {
                (Some(code), false) => {
                    format!(
                        "{} still fails to start (exit {code}): {}",
                        target.name,
                        excerpt(&output)
                    )
                }
                (Some(code), true) => format!("{} still fails to start (exit {code})", target.name),
                (None, false) => {
                    format!(
                        "{} was stopped before it started: {}",
                        target.name,
                        excerpt(&output)
                    )
                }
                (None, true) => format!("{} was stopped before it started", target.name),
            };
            Verdict::StillBroken { detail }
        }
        // A launcher exiting straight away with success, and nothing running
        // afterwards, is genuinely ambiguous: it might have handed off to
        // something that then quit, or done nothing at all. Guessing "fixed"
        // here would be the single easiest way for this tool to tell somebody
        // their problem is solved when it is not.
        LaunchResult::ExitedImmediately { code } => Verdict::CannotTell {
            reason: format!(
                "{} exited immediately with code {code} and is not running, so whether it \
                 started cannot be told from that",
                target.name
            ),
        },
        LaunchResult::AlreadyRunning => Verdict::CannotTell {
            reason: format!(
                "{} was already running before the test, so starting it would prove nothing",
                target.name
            ),
        },
        LaunchResult::NotFound => Verdict::CannotTell {
            reason: format!("{} could not be found to test", target.name),
        },
        LaunchResult::CouldNotTest { reason } => Verdict::CannotTell { reason },
    }
}

#[async_trait]
impl<T: LaunchTester> Verifier for LaunchVerifier<T> {
    async fn verify(&self, item: &TriageItem) -> Verdict {
        let Some(target) = target_for(item) else {
            return Verdict::CannotTell {
                reason: "this build cannot test that application by starting it".to_string(),
            };
        };
        verdict_for(target, self.tester.test(target).await)
    }
}

impl<T: LaunchTester> ItemVerifier for LaunchVerifier<T> {
    fn handles(&self, item: &TriageItem) -> bool {
        target_for(item).is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::ItemState;
    use ork_core::PlatformKind;
    use ork_core::{Category, Finding, Severity, Triage};
    use std::sync::Mutex;

    /// Answers with whatever the test wants, so the decisions above can be
    /// checked without starting a real application.
    struct ScriptedTester {
        result: Mutex<Option<LaunchResult>>,
    }

    impl ScriptedTester {
        fn new(result: LaunchResult) -> Self {
            Self {
                result: Mutex::new(Some(result)),
            }
        }
    }

    #[async_trait]
    impl LaunchTester for ScriptedTester {
        async fn test(&self, _target: &LaunchTarget) -> LaunchResult {
            self.result.lock().unwrap().take().expect("tested twice")
        }
    }

    fn steam() -> &'static LaunchTarget {
        LAUNCHERS
            .iter()
            .find(|target| target.id == "steam")
            .expect("steam is catalogued")
    }

    fn item(finding_id: &str, subject: &str) -> TriageItem {
        let finding = Finding::builder(finding_id, finding_id)
            .subject(subject)
            .severity(Severity::High)
            .category(Category::Application)
            .title("Steam is installed but fails to start")
            .detail("exited with an error")
            .triage(Triage::Queue)
            .build();
        TriageItem {
            id: 1,
            occurrence_key: finding.occurrence_key(),
            finding_id: finding.id.clone(),
            subject: finding.subject.clone(),
            severity: finding.severity,
            title: finding.title.clone(),
            finding,
            state: ItemState::Pending,
            attempts: 0,
        }
    }

    #[test]
    fn steam_is_recognised_on_the_platforms_it_runs_on() {
        let target = steam();
        assert!(target.runs_on(PlatformKind::Linux));
        assert!(target.runs_on(PlatformKind::Windows));
        assert!(
            !target.runs_on(PlatformKind::MacOs),
            "no macOS build is tested yet"
        );
    }

    #[test]
    fn a_launch_finding_about_steam_is_matched_to_steam() {
        assert!(target_for(&item("app.launch-failed", "steam")).is_some());
        assert!(target_for(&item("app.launch-hung", "steam")).is_some());
        assert!(
            target_for(&item("app.launch-failed", "STEAM")).is_some(),
            "case must not matter"
        );
    }

    #[test]
    fn unrelated_problems_are_not_claimed() {
        // Claiming a problem this cannot test would make the engine apply a
        // change and then roll it back, which is worse than leaving it alone.
        assert!(target_for(&item("app.launch-failed", "some-other-program")).is_none());
        assert!(target_for(&item("storage.disk-space", "steam")).is_none());
        assert!(target_for(&item("app.launch-failed", "")).is_none());
    }

    #[test]
    fn staying_up_is_the_only_thing_that_counts_as_fixed() {
        assert_eq!(verdict_for(steam(), LaunchResult::Started), Verdict::Fixed);
    }

    #[test]
    fn a_failure_is_reported_with_what_it_said() {
        let verdict = verdict_for(
            steam(),
            LaunchResult::Failed {
                code: Some(1),
                output: "error while loading shared libraries: libGL.so.1".to_string(),
            },
        );
        match verdict {
            Verdict::StillBroken { detail } => {
                assert!(
                    detail.contains("libGL.so.1"),
                    "the cause was dropped: {detail}"
                );
                assert!(detail.contains("exit 1"));
            }
            other => panic!("expected still-broken, got {other:?}"),
        }
    }

    #[test]
    fn exiting_straight_away_is_not_treated_as_success() {
        // The easiest way for this tool to tell somebody their problem is
        // solved when it is not.
        let verdict = verdict_for(steam(), LaunchResult::ExitedImmediately { code: 0 });
        assert!(
            matches!(verdict, Verdict::CannotTell { .. }),
            "got {verdict:?}"
        );
    }

    #[test]
    fn an_application_that_was_already_running_proves_nothing() {
        let verdict = verdict_for(steam(), LaunchResult::AlreadyRunning);
        match verdict {
            Verdict::CannotTell { reason } => assert!(reason.contains("already running")),
            other => panic!("expected cannot-tell, got {other:?}"),
        }
    }

    #[test]
    fn every_way_of_not_knowing_ends_as_cannot_tell() {
        for result in [
            LaunchResult::NotFound,
            LaunchResult::AlreadyRunning,
            LaunchResult::ExitedImmediately { code: 0 },
            LaunchResult::CouldNotTest {
                reason: "no".into(),
            },
        ] {
            assert!(
                matches!(
                    verdict_for(steam(), result.clone()),
                    Verdict::CannotTell { .. }
                ),
                "{result:?} must not be mistaken for an answer"
            );
        }
    }

    #[tokio::test]
    async fn the_verifier_reports_what_the_launch_showed() {
        let verifier = LaunchVerifier::new(ScriptedTester::new(LaunchResult::Started));
        assert_eq!(
            verifier.verify(&item("app.launch-failed", "steam")).await,
            Verdict::Fixed
        );
    }

    #[tokio::test]
    async fn the_verifier_refuses_problems_it_cannot_test() {
        let verifier = LaunchVerifier::new(ScriptedTester::new(LaunchResult::Started));
        let verdict = verifier
            .verify(&item("app.launch-failed", "not-catalogued"))
            .await;
        assert!(
            matches!(verdict, Verdict::CannotTell { .. }),
            "got {verdict:?}"
        );
    }

    #[test]
    fn nothing_in_the_catalogue_promises_a_test_it_cannot_run() {
        for target in LAUNCHERS {
            assert!(
                !target.executables.is_empty(),
                "{} has nothing to run",
                target.id
            );
            assert!(
                !target.process_names.is_empty(),
                "{} cannot be recognised once running",
                target.id
            );
            assert!(!target.platforms.is_empty(), "{} runs nowhere", target.id);
        }
    }
}
