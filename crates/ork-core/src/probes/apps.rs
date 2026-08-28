//! Do the installed applications actually run?
//!
//! "It won't launch any more" is one of the most common real complaints about
//! a computer, and one of the least visible to a scan that only looks at
//! hardware and logs. This probe catches it directly, by asking installed
//! applications to report themselves and seeing what happens.
//!
//! The table of applications and the test itself live in [`crate::respond`],
//! not here, so that the check which finds a broken application and the check
//! which later declares it repaired are the same code. This module only turns
//! the answer into a finding.

use async_trait::async_trait;

use crate::Result;
use crate::finding::{Category, Finding, Severity, Triage};
use crate::platform::PlatformKind;
use crate::probe::{Probe, ProbeContext, ProbeMeta};
use crate::respond::{APPS, AppDefinition, RealResponseTester, RespondResult, ResponseTester};
use crate::tier::ScanTier;

/// Trim program output down to something readable in a report.
fn excerpt(text: &str, limit: usize) -> String {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.chars().count() > limit {
        format!("{}...", text.chars().take(limit).collect::<String>())
    } else {
        text
    }
}

/// Build a finding from an application that did not answer cleanly.
///
/// Only two answers are findings. An application that responds is fine, one
/// that is not installed is not a fault, and a test that could not be carried
/// out says nothing about the application -- reporting any of those would be
/// inventing a problem.
fn finding_for(app: &AppDefinition, result: &RespondResult) -> Option<Finding> {
    let (id, severity, title, detail, hint, executable, stdout, stderr) = match result {
        RespondResult::Responds { .. }
        | RespondResult::NotInstalled
        | RespondResult::CouldNotTest { .. } => return None,
        RespondResult::Failed {
            executable,
            code,
            stdout,
            stderr,
        } => (
            "app.launch-failed",
            Severity::High,
            format!("{} is installed but fails to start", app.name),
            format!(
                "Running `{executable}` returned an error instead of starting normally \
                 (exit code {}). Something about this installation is broken -- a missing \
                 dependency, a permissions problem, or a partially applied update are the \
                 usual causes.",
                code.map(|code| code.to_string())
                    .unwrap_or_else(|| "unknown".to_string())
            ),
            "Run the program from a terminal to see its full error, then check whether it \
             was affected by a recent update.",
            executable.clone(),
            stdout.clone(),
            stderr.clone(),
        ),
        RespondResult::Hung { executable, idle } => (
            "app.launch-hung",
            Severity::High,
            format!("{} hangs instead of starting", app.name),
            format!(
                "Running `{executable}` never finished starting. It stopped using the \
                 processor, stopped reading and writing, and produced no output for {} \
                 seconds, so it was stopped. A program that hangs this way is usually \
                 waiting on something that will never arrive -- a service that is not \
                 running, a network resource, or a lock held by another process.",
                idle.as_secs()
            ),
            "Check whether any service this program depends on is running, and look for a \
             stale lock file left behind by a previous crash.",
            executable.clone(),
            String::new(),
            String::new(),
        ),
    };

    let mut builder = Finding::builder("apps.launch-check", id)
        // The slug, not the display name: this is what a verifier looks the
        // application back up by, so it has to be the machine-readable one.
        .subject(app.id)
        .severity(severity)
        .category(Category::Application)
        .title(title)
        .detail(detail)
        .evidence("application", app.name)
        .evidence("executable", executable)
        .remediation_hint(hint)
        // An application that will not start rarely has one known cause, so
        // this is worked through the triage queue rather than fixed inline.
        .triage(Triage::Queue);

    let stdout = excerpt(&stdout, 400);
    if !stdout.is_empty() {
        builder = builder.evidence("stdout", stdout);
    }
    let stderr = excerpt(&stderr, 400);
    if !stderr.is_empty() {
        builder = builder.evidence("stderr", stderr);
    }

    Some(builder.build())
}

#[derive(Debug, Default)]
pub struct AppLaunchProbe;

#[async_trait]
impl Probe for AppLaunchProbe {
    fn meta(&self) -> ProbeMeta {
        ProbeMeta {
            id: "apps.launch-check",
            name: "Application launch check",
            description: "Runs installed applications to confirm they still start cleanly.",
            category: Category::Application,
            min_tier: ScanTier::Quick,
            platforms: &[
                PlatformKind::Windows,
                PlatformKind::Linux,
                PlatformKind::MacOs,
            ],
            requires_tools: &[],
            emits: &["app.launch-failed", "app.launch-hung"],
            requires_elevation: false,
        }
    }

    async fn run(&self, ctx: &ProbeContext) -> Result<Vec<Finding>> {
        let platform_kind = ctx.platform().kind();
        let tester = RealResponseTester::new(ctx.cancel_token().clone());
        let mut findings = Vec::new();

        for app in APPS {
            if ctx.is_cancelled() {
                break;
            }
            if !app.runs_on(platform_kind) {
                continue;
            }

            let result = tester.test(app).await;
            tracing::debug!(app = app.id, ?result, "launch tested");
            findings.extend(finding_for(app, &result));
        }

        Ok(findings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn app() -> AppDefinition {
        AppDefinition {
            id: "test-app",
            name: "Test App",
            executables: &["testapp"],
            version_args: &["--version"],
            platforms: &[PlatformKind::Linux],
        }
    }

    #[test]
    fn an_application_that_answers_produces_no_finding() {
        assert!(
            finding_for(
                &app(),
                &RespondResult::Responds {
                    executable: "/usr/bin/testapp".to_string()
                }
            )
            .is_none()
        );
    }

    #[test]
    fn an_absent_application_is_not_a_problem() {
        // Reporting on programs somebody chose not to install would be
        // reporting on their choices.
        assert!(finding_for(&app(), &RespondResult::NotInstalled).is_none());
    }

    #[test]
    fn a_test_that_could_not_run_is_never_blamed_on_the_application() {
        assert!(
            finding_for(
                &app(),
                &RespondResult::CouldNotTest {
                    reason: "the scan was stopped".to_string()
                }
            )
            .is_none()
        );
    }

    #[test]
    fn a_failure_is_queued_and_carries_its_output() {
        let finding = finding_for(
            &app(),
            &RespondResult::Failed {
                executable: "/usr/bin/testapp".to_string(),
                code: Some(127),
                stdout: String::new(),
                stderr: "libpcre.so.3: cannot open shared object file".to_string(),
            },
        )
        .expect("a failing application is a finding");

        assert_eq!(finding.id, "app.launch-failed");
        assert_eq!(finding.triage, Triage::Queue);
        // The slug, so a verifier can look the application back up.
        assert_eq!(finding.subject.as_deref(), Some("test-app"));
        assert!(
            finding
                .evidence
                .iter()
                .any(|evidence| evidence.label == "stderr" && evidence.value.contains("libpcre")),
            "the error that explains it must survive into the finding"
        );
    }

    #[test]
    fn a_hang_is_reported_as_a_hang_rather_than_a_failure() {
        // These have different causes and different fixes, so collapsing them
        // into one finding would send people looking in the wrong place.
        let finding = finding_for(
            &app(),
            &RespondResult::Hung {
                executable: "/usr/bin/testapp".to_string(),
                idle: Duration::from_secs(30),
            },
        )
        .expect("a hung application is a finding");
        assert_eq!(finding.id, "app.launch-hung");
        assert!(finding.detail.contains("30"));
    }
}
