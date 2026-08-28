//! Deciding what a second "are you working?" means.
//!
//! The test itself lives in [`ork_core::respond`], for the same reason the
//! launch test lives in the core: the check that found the application broken
//! and the check that later declares it repaired have to be the same code. If
//! they ever drifted, "fixed" would quietly come to mean something other than
//! "not found any more", and nothing would announce the change.
//!
//! What lives here is the judgement. Three of the five possible answers are
//! **not** good news, and only one of them means the problem is gone:
//!
//! * it answers cleanly -- fixed;
//! * it still errors, or still hangs -- still broken, and the output usually
//!   says why;
//! * it is no longer installed, or the test could not be run -- cannot tell.
//!
//! That third case deserves the caution. A program that has vanished since the
//! scan is not a program that was repaired, and a change that made it
//! disappear is a change worth undoing rather than congratulating.

use async_trait::async_trait;
use ork_core::respond::{AppDefinition, RespondResult, ResponseTester, find};

use crate::engine::{Verdict, Verifier};
use crate::store::TriageItem;
use crate::verify::{ItemVerifier, excerpt, this_platform};

/// Asks an application again whether it works, and decides what that means.
pub struct RespondsVerifier<T: ResponseTester> {
    tester: T,
}

impl<T: ResponseTester> RespondsVerifier<T> {
    pub fn new(tester: T) -> Self {
        Self { tester }
    }
}

/// The application a finding is about, if this build can ask it anything.
pub fn app_for(item: &TriageItem) -> Option<&'static AppDefinition> {
    if !matches!(
        item.finding_id.as_str(),
        "app.launch-failed" | "app.launch-hung"
    ) {
        return None;
    }
    let subject = item.subject.as_deref()?.to_ascii_lowercase();
    find(&subject).filter(|app| app.runs_on(this_platform()))
}

/// Turn the answer into a verdict.
///
/// Separated from the process work because this is where the judgement lives,
/// and judgement is worth testing without running anything.
pub fn verdict_for(app: &AppDefinition, result: RespondResult) -> Verdict {
    match result {
        RespondResult::Responds { .. } => Verdict::Fixed,
        RespondResult::Failed {
            code,
            stdout,
            stderr,
            ..
        } => {
            // Whichever stream carried the explanation. Programs disagree
            // about which one that is, and the message is the useful part.
            let output = if stderr.trim().is_empty() {
                stdout
            } else {
                stderr
            };
            let code = code
                .map(|code| format!(" (exit {code})"))
                .unwrap_or_default();
            let detail = if output.trim().is_empty() {
                format!("{} still fails to start{code}", app.name)
            } else {
                format!(
                    "{} still fails to start{code}: {}",
                    app.name,
                    excerpt(&output)
                )
            };
            Verdict::StillBroken { detail }
        }
        RespondResult::Hung { idle, .. } => Verdict::StillBroken {
            detail: format!(
                "{} still hangs instead of starting -- it went quiet for {} seconds",
                app.name,
                idle.as_secs()
            ),
        },
        // Gone is not the same as mended. A change that removed the program
        // has not repaired it, so this is not allowed to read as success.
        RespondResult::NotInstalled => Verdict::CannotTell {
            reason: format!(
                "{} is no longer installed, so nothing can be asked",
                app.name
            ),
        },
        RespondResult::CouldNotTest { reason } => Verdict::CannotTell {
            reason: format!("{} could not be tested again: {reason}", app.name),
        },
    }
}

#[async_trait]
impl<T: ResponseTester> Verifier for RespondsVerifier<T> {
    async fn verify(&self, item: &TriageItem) -> Verdict {
        let Some(app) = app_for(item) else {
            return Verdict::CannotTell {
                reason: "this build cannot re-test that application".to_string(),
            };
        };
        verdict_for(app, self.tester.test(app).await)
    }
}

impl<T: ResponseTester> ItemVerifier for RespondsVerifier<T> {
    fn handles(&self, item: &TriageItem) -> bool {
        app_for(item).is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    const APP: AppDefinition = AppDefinition {
        id: "test-app",
        name: "Test App",
        executables: &["testapp"],
        version_args: &["--version"],
        platforms: &[ork_core::PlatformKind::Linux],
    };

    #[test]
    fn a_clean_answer_is_the_only_thing_read_as_fixed() {
        assert_eq!(
            verdict_for(
                &APP,
                RespondResult::Responds {
                    executable: "/usr/bin/testapp".to_string()
                }
            ),
            Verdict::Fixed
        );
    }

    #[test]
    fn a_failure_is_still_broken_and_quotes_what_it_said() {
        let verdict = verdict_for(
            &APP,
            RespondResult::Failed {
                executable: "/usr/bin/testapp".to_string(),
                code: Some(127),
                stdout: String::new(),
                stderr: "libpcre.so.3: cannot open shared object file".to_string(),
            },
        );
        match verdict {
            Verdict::StillBroken { detail } => {
                assert!(detail.contains("exit 127"));
                assert!(detail.contains("libpcre"));
            }
            other => panic!("expected still broken, got {other:?}"),
        }
    }

    #[test]
    fn an_explanation_on_stdout_is_used_when_stderr_is_empty() {
        // Programs disagree about which stream an error belongs on, and the
        // message is worth more than the convention.
        let verdict = verdict_for(
            &APP,
            RespondResult::Failed {
                executable: "/usr/bin/testapp".to_string(),
                code: Some(1),
                stdout: "config file is corrupt".to_string(),
                stderr: "   ".to_string(),
            },
        );
        match verdict {
            Verdict::StillBroken { detail } => assert!(detail.contains("config file is corrupt")),
            other => panic!("expected still broken, got {other:?}"),
        }
    }

    #[test]
    fn a_hang_is_still_broken_rather_than_untestable() {
        let verdict = verdict_for(
            &APP,
            RespondResult::Hung {
                executable: "/usr/bin/testapp".to_string(),
                idle: Duration::from_secs(30),
            },
        );
        assert!(matches!(verdict, Verdict::StillBroken { .. }));
    }

    #[test]
    fn an_application_that_vanished_is_not_reported_as_repaired() {
        // This is the dangerous one. A fix that deleted the program would
        // otherwise be congratulated for it.
        assert!(matches!(
            verdict_for(&APP, RespondResult::NotInstalled),
            Verdict::CannotTell { .. }
        ));
    }

    #[test]
    fn a_test_that_could_not_run_cannot_tell() {
        assert!(matches!(
            verdict_for(
                &APP,
                RespondResult::CouldNotTest {
                    reason: "the machine could not be read".to_string()
                }
            ),
            Verdict::CannotTell { .. }
        ));
    }

    fn item(finding_id: &str, subject: &str) -> TriageItem {
        let finding = ork_core::Finding::builder("apps.launch-check", finding_id)
            .subject(subject)
            .severity(ork_core::Severity::High)
            .category(ork_core::finding::Category::Application)
            .title("a problem")
            .detail("details")
            .build();
        TriageItem {
            id: 1,
            occurrence_key: finding.occurrence_key(),
            finding_id: finding.id.clone(),
            subject: finding.subject.clone(),
            severity: finding.severity,
            title: finding.title.clone(),
            finding,
            state: crate::store::ItemState::Pending,
            attempts: 0,
            first_seen: "2026-01-01T00:00:00Z".to_string(),
            last_seen: "2026-01-01T00:00:00Z".to_string(),
            seen: "seen once, 2026-01-01 00:00:00".to_string(),
        }
    }

    #[test]
    fn only_launch_findings_are_claimed() {
        assert!(app_for(&item("disk.low-space", "git")).is_none());
    }

    #[test]
    fn an_application_this_build_does_not_know_is_not_claimed() {
        // Claiming it would mean the engine applied a change and then found
        // it had no way to check the result, which costs a rollback for
        // nothing.
        assert!(app_for(&item("app.launch-failed", "some-other-app")).is_none());
    }

    #[test]
    fn an_application_this_build_does_know_is_claimed() {
        // Git is in the table on every platform, so this holds wherever the
        // tests run.
        assert!(app_for(&item("app.launch-failed", "git")).is_some());
        assert!(app_for(&item("app.launch-hung", "GIT")).is_some());
    }

    #[test]
    fn the_two_application_tables_do_not_overlap() {
        // Both verifiers claim the same finding ids and are told apart by the
        // slug alone. A slug in both tables would make which one runs depend
        // on registration order, which is not a thing anyone should have to
        // know.
        for app in ork_core::respond::APPS {
            assert!(
                !ork_core::launch::LAUNCHERS
                    .iter()
                    .any(|target| target.id == app.id),
                "{} is in both tables",
                app.id
            );
        }
    }
}
