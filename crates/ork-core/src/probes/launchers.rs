//! Testing whether applications with a user interface actually start.
//!
//! [`super::apps`] covers programs that can be asked to report themselves and
//! exit. This one covers the rest -- launchers and clients that have to be
//! genuinely started to learn anything about them.
//!
//! **This check starts and closes real applications**, which is why it is in
//! the Full tier and not the Quick one. A scan you asked to be quick has no
//! business opening windows on your desktop. A scan you asked to be thorough
//! is exactly where "does Steam actually start?" gets answered, and answering
//! it is the whole point of the exercise.
//!
//! Anything already running is left strictly alone: it is working, which is
//! the answer we wanted, and it is not this tool's place to close something
//! somebody is using.

use anyhow::Result;
use async_trait::async_trait;

use crate::finding::{Category, Finding, Severity, Triage};
use crate::launch::{LAUNCHERS, LaunchResult, LaunchTarget, LaunchTester, RealLaunchTester};
use crate::probe::{Probe, ProbeContext, ProbeMeta};
use crate::tier::ScanTier;

/// How much of the captured error output to keep.
const EXCERPT: usize = 600;

fn excerpt(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= EXCERPT {
        return trimmed.to_string();
    }
    let cut: String = trimmed.chars().take(EXCERPT).collect();
    format!("{cut}...")
}

/// Turn one launch attempt into a finding, if it deserves one.
///
/// Only an outright failure is reported. Everything else is either good news
/// or not knowledge -- and a diagnostic tool that reports "I could not tell"
/// as a problem trains people to ignore it.
pub fn finding_for(target: &LaunchTarget, result: &LaunchResult) -> Option<Finding> {
    match result {
        // It started, or it was already running. Either way it works.
        LaunchResult::Started | LaunchResult::AlreadyRunning => None,
        // Not installed is not a fault.
        LaunchResult::NotFound => None,
        // Ambiguous. Saying nothing is more honest than guessing either way.
        LaunchResult::ExitedImmediately { .. } => None,
        LaunchResult::CouldNotTest { .. } => None,
        LaunchResult::Failed { code, output } => {
            let mut builder = Finding::builder("apps.launcher-check", "app.launch-failed")
                .subject(target.id)
                .severity(Severity::High)
                .category(Category::Application)
                .title(format!("{} is installed but fails to start", target.name))
                .detail(format!(
                    "{} was started and exited with an error instead of opening. The output \
                     below usually names the cause.",
                    target.name
                ))
                .evidence("application", target.name)
                .triage(Triage::Queue);

            if let Some(code) = code {
                builder = builder.evidence("exit code", code.to_string());
            }
            if !output.trim().is_empty() {
                builder = builder.evidence("error output", excerpt(output));
            }
            Some(builder.build())
        }
    }
}

/// Starts catalogued applications to see whether they work.
pub struct LauncherProbe;

#[async_trait]
impl Probe for LauncherProbe {
    fn meta(&self) -> ProbeMeta {
        ProbeMeta {
            id: "apps.launcher-check",
            name: "Application launch test",
            description: "Starts applications that have to be run to be tested, such as Steam, \
                          and closes them again. Anything already running is left alone.",
            category: Category::Application,
            min_tier: ScanTier::Full,
            platforms: &[
                crate::PlatformKind::Windows,
                crate::PlatformKind::Linux,
                crate::PlatformKind::MacOs,
            ],
            requires_tools: &[],
            requires_elevation: false,
        }
    }

    async fn run(&self, ctx: &ProbeContext) -> Result<Vec<Finding>> {
        let platform = ctx.platform().kind();
        let tester = RealLaunchTester::default();
        let mut findings = Vec::new();

        for target in LAUNCHERS.iter().filter(|target| target.runs_on(platform)) {
            if ctx.cancel_token().is_cancelled() {
                break;
            }
            let result = tester.test(target).await;
            tracing::debug!(application = target.id, ?result, "launch test");
            if let Some(finding) = finding_for(target, &result) {
                findings.push(finding);
            }
        }

        Ok(findings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn steam() -> &'static LaunchTarget {
        LAUNCHERS
            .iter()
            .find(|target| target.id == "steam")
            .expect("steam is catalogued")
    }

    #[test]
    fn a_failure_becomes_a_finding_that_names_the_cause() {
        let finding = finding_for(
            steam(),
            &LaunchResult::Failed {
                code: Some(1),
                output: "error while loading shared libraries: libGL.so.1".to_string(),
            },
        )
        .expect("a failure must be reported");

        assert_eq!(finding.id, "app.launch-failed");
        assert_eq!(finding.subject.as_deref(), Some("steam"));
        assert_eq!(finding.severity, Severity::High);
        assert_eq!(finding.triage, Triage::Queue);
        // The runbook matches on this text, and a person reading it needs it.
        let evidence: String = finding
            .evidence
            .iter()
            .map(|item| item.value.clone())
            .collect::<Vec<_>>()
            .join(" ");
        assert!(
            evidence.contains("libGL.so.1"),
            "the cause was dropped: {evidence}"
        );
        assert!(evidence.contains('1'), "the exit code was dropped");
    }

    #[test]
    fn an_application_that_works_is_not_reported() {
        assert!(finding_for(steam(), &LaunchResult::Started).is_none());
        // Already running is the best possible answer to "does it start?".
        assert!(finding_for(steam(), &LaunchResult::AlreadyRunning).is_none());
    }

    #[test]
    fn not_being_installed_is_not_a_fault() {
        assert!(finding_for(steam(), &LaunchResult::NotFound).is_none());
    }

    #[test]
    fn not_knowing_is_never_reported_as_a_problem() {
        // A tool that reports its own uncertainty as a fault teaches people to
        // ignore it.
        assert!(finding_for(steam(), &LaunchResult::ExitedImmediately { code: 0 }).is_none());
        assert!(
            finding_for(
                steam(),
                &LaunchResult::CouldNotTest {
                    reason: "no".into()
                }
            )
            .is_none()
        );
    }

    #[test]
    fn this_check_is_never_part_of_a_quick_scan() {
        // It starts real applications. A scan somebody asked to be quick must
        // not open windows on their desktop.
        assert_eq!(LauncherProbe.meta().min_tier, ScanTier::Full);
    }

    #[test]
    fn long_error_output_is_trimmed_but_keeps_its_beginning() {
        let noisy = "x".repeat(EXCERPT * 3);
        let finding = finding_for(
            steam(),
            &LaunchResult::Failed {
                code: Some(2),
                output: noisy,
            },
        )
        .unwrap();
        let captured = finding
            .evidence
            .iter()
            .find(|item| item.label == "error output")
            .expect("output kept");
        assert!(captured.value.len() < EXCERPT * 2);
        assert!(captured.value.ends_with("..."));
    }
}
