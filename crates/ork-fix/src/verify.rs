//! Telling whether a fix actually worked.
//!
//! The fix engine will not apply a change it cannot test. That is not a
//! limitation to be worked around -- it is the rule that makes automated
//! fixing safe to offer at all, because a change nobody checked is not a
//! repair, it is a hope. This module is what turns "we cannot test that" into
//! "we can".
//!
//! Every verifier here re-runs **the same test that produced the finding**.
//! That matters more than it sounds: if the check that found the problem and
//! the check that declares it solved are different tests, then "fixed" means
//! something other than "not found any more", and the difference is exactly
//! where a tool starts lying to people.
//!
//! Verifiers answer one of three ways, and the third is the important one. A
//! verifier that cannot carry out its test says so, and the engine rolls the
//! change back -- because an unverified change to somebody's machine is not an
//! improvement, however plausible it looked.

use async_trait::async_trait;
use ork_core::PlatformKind;
use ork_core::platform::ServiceStatus;

use crate::engine::{Verdict, Verifier};
use crate::store::TriageItem;

pub mod launch;
pub mod responds;

pub use launch::LaunchVerifier;
pub use responds::RespondsVerifier;
// Re-exported so a caller does not have to know that the launch test itself
// lives in the core, while the judgement about it lives here.
pub use ork_core::launch::{LaunchResult, LaunchTarget, LaunchTester, RealLaunchTester};
pub use ork_core::respond::{RealResponseTester, RespondResult, ResponseTester};

/// How much captured output to quote back when reporting a failure.
const EXCERPT: usize = 400;

pub(crate) fn excerpt(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= EXCERPT {
        return trimmed.to_string();
    }
    let cut: String = trimmed.chars().take(EXCERPT).collect();
    format!("{cut}...")
}

pub(crate) fn this_platform() -> PlatformKind {
    ork_core::platform::detect()
        .map(|platform| platform.kind())
        .unwrap_or(PlatformKind::Linux)
}

/// Everything this build knows how to re-test.
///
/// The engine asks this for a verifier *before* it applies anything. When the
/// answer is `None` the change is offered as advice instead of being made --
/// so adding a verifier here is what promotes a class of problem from
/// "described to you" to "fixed for you".
pub struct VerifierRegistry {
    verifiers: Vec<Box<dyn ItemVerifier>>,
}

/// A verifier that knows which problems it can speak to.
pub trait ItemVerifier: Verifier {
    /// Whether this verifier can re-test that particular problem.
    ///
    /// Answering `true` is a promise that [`Verifier::verify`] will carry out
    /// a real test, not that it will guess well.
    fn handles(&self, item: &TriageItem) -> bool;
}

impl VerifierRegistry {
    /// The verifiers this build ships with.
    pub fn standard() -> Self {
        Self {
            verifiers: vec![
                Box::new(LaunchVerifier::new(RealLaunchTester::default())),
                // Both of these claim the same finding ids and are told apart
                // by the application slug. The tables they read from are kept
                // disjoint, so the order here does not decide anything -- see
                // the test that holds them apart.
                Box::new(RespondsVerifier::new(RealResponseTester::default())),
                Box::new(ServiceRunningVerifier),
                Box::new(FileGoneVerifier),
            ],
        }
    }

    pub fn empty() -> Self {
        Self {
            verifiers: Vec::new(),
        }
    }

    pub fn with(mut self, verifier: Box<dyn ItemVerifier>) -> Self {
        self.verifiers.push(verifier);
        self
    }

    /// The verifier for this problem, if there is one.
    ///
    /// Deliberately an `Option` rather than a fallback that always answers.
    /// A verifier that cannot really test something would make the engine
    /// apply a change and then roll it back, which is worse than never having
    /// touched the machine.
    pub fn for_item(&self, item: &TriageItem) -> Option<&dyn Verifier> {
        self.verifiers
            .iter()
            .find(|verifier| verifier.handles(item))
            .map(|verifier| verifier.as_ref() as &dyn Verifier)
    }

    /// How many problems in a list this build could actually fix rather than
    /// describe. Worth showing people up front.
    pub fn coverage<'a>(&self, items: impl IntoIterator<Item = &'a TriageItem>) -> usize {
        items
            .into_iter()
            .filter(|item| self.for_item(item).is_some())
            .count()
    }
}

impl Default for VerifierRegistry {
    fn default() -> Self {
        Self::standard()
    }
}

/// Confirms a service that was supposed to be restarted is actually running.
///
/// This is the other half of the only two changes the engine can make. Without
/// it, `restart-service` could never be applied at all: the engine refuses to
/// make a change nobody can test, so an action with no verifier is an action
/// that never runs.
///
/// The distinction it exists to draw: the restart command exiting zero says
/// the command ran. It does not say the service came up, and it certainly does
/// not say the service stayed up. Only asking afterwards answers that.
pub struct ServiceRunningVerifier;

#[async_trait]
impl Verifier for ServiceRunningVerifier {
    async fn verify(&self, item: &TriageItem) -> Verdict {
        let Some(service) = service_name(item) else {
            return Verdict::CannotTell {
                reason: "this problem does not name a service to check".to_string(),
            };
        };

        let Ok(platform) = ork_core::platform::detect() else {
            return Verdict::CannotTell {
                reason: "could not read this machine to check the service".to_string(),
            };
        };

        match platform.service_status(&service) {
            ServiceStatus::Running => Verdict::Fixed,
            ServiceStatus::Stopped => Verdict::StillBroken {
                detail: format!("`{service}` is not running"),
            },
            // A name that does not resolve is not evidence the fix worked, and
            // it is not evidence it failed either -- it means the finding and
            // this machine disagree about what the service is called.
            ServiceStatus::NotFound => Verdict::CannotTell {
                reason: format!("this machine has no service called `{service}`"),
            },
            ServiceStatus::Unknown { detail } => Verdict::CannotTell {
                reason: format!("could not tell what `{service}` is doing: {detail}"),
            },
        }
    }
}

impl ItemVerifier for ServiceRunningVerifier {
    fn handles(&self, item: &TriageItem) -> bool {
        service_name(item).is_some()
    }
}

/// The service a finding is about, taken from its evidence.
fn service_name(item: &TriageItem) -> Option<String> {
    let named = item.finding.evidence.iter().find(|evidence| {
        let label = evidence.label.to_ascii_lowercase();
        label == "service" || label == "unit"
    })?;
    let name = named.value.trim();
    (!name.is_empty()).then(|| name.to_string())
}

/// Confirms a file that was supposed to be removed is actually gone.
///
/// The cheapest verifier there is, and a real one: removing a stale lock file
/// is one of only two changes this engine can make, and "is it gone" is
/// exactly the question the fix claimed to answer.
pub struct FileGoneVerifier;

#[async_trait]
impl Verifier for FileGoneVerifier {
    async fn verify(&self, item: &TriageItem) -> Verdict {
        let Some(path) = stale_file_path(item) else {
            return Verdict::CannotTell {
                reason: "this problem does not name a file to check".to_string(),
            };
        };

        if std::path::Path::new(&path).exists() {
            Verdict::StillBroken {
                detail: format!("{path} is still there"),
            }
        } else {
            Verdict::Fixed
        }
    }
}

impl ItemVerifier for FileGoneVerifier {
    fn handles(&self, item: &TriageItem) -> bool {
        stale_file_path(item).is_some()
    }
}

/// The file a stale-file finding is about, taken from its evidence.
fn stale_file_path(item: &TriageItem) -> Option<String> {
    if !item.finding_id.contains("stale") {
        return None;
    }
    item.finding
        .evidence
        .iter()
        .find(|evidence| {
            let label = evidence.label.to_ascii_lowercase();
            label == "path" || label == "file"
        })
        .map(|evidence| evidence.value.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::ItemState;
    use ork_core::finding::{Category, Finding, Severity, Triage};

    fn item(finding_id: &str, evidence: &[(&str, &str)]) -> TriageItem {
        let mut builder = Finding::builder("probe", finding_id)
            .subject("something")
            .severity(Severity::High)
            .category(Category::Application)
            .title("a problem")
            .detail("details")
            .triage(Triage::Queue);
        for (label, value) in evidence {
            builder = builder.evidence(*label, *value);
        }
        let finding = builder.build();
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
    fn a_service_is_recognised_by_the_evidence_that_names_it() {
        assert!(ServiceRunningVerifier.handles(&item("service.stopped", &[("service", "cups")])));
        // systemd calls them units, and a finding may say so.
        assert!(ServiceRunningVerifier.handles(&item("service.stopped", &[("unit", "cups")])));
    }

    #[test]
    fn a_problem_that_names_no_service_is_not_claimed() {
        assert!(!ServiceRunningVerifier.handles(&item("service.stopped", &[])));
        assert!(!ServiceRunningVerifier.handles(&item("service.stopped", &[("service", "   ")])));
        assert!(!ServiceRunningVerifier.handles(&item("memory.pressure", &[("used", "97%")])));
    }

    #[tokio::test]
    async fn a_service_this_machine_has_never_heard_of_is_never_called_fixed() {
        // The engine treats cannot-tell as failure and rolls back, which is
        // the right outcome: the finding and this machine disagree about what
        // the service is called, so nothing has been proved either way.
        let verdict = ServiceRunningVerifier
            .verify(&item(
                "service.stopped",
                &[("service", "ork-not-a-real-service")],
            ))
            .await;
        assert!(
            matches!(verdict, Verdict::CannotTell { .. }),
            "got {verdict:?}"
        );
    }

    #[test]
    fn a_stale_file_is_recognised_by_the_path_in_its_evidence() {
        assert!(
            FileGoneVerifier.handles(&item("app.launch-stale-lock", &[("path", "/tmp/x.pid")]))
        );
        assert!(!FileGoneVerifier.handles(&item("app.launch-failed", &[("path", "/tmp/x.pid")])));
        assert!(!FileGoneVerifier.handles(&item("app.launch-stale-lock", &[])));
    }

    #[tokio::test]
    async fn a_file_still_present_is_reported_as_still_broken() {
        let dir = std::env::temp_dir().join(format!("ork-verify-unit-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("still-here.pid");
        std::fs::write(&path, "1").unwrap();

        let present = item(
            "app.launch-stale-lock",
            &[("path", &path.display().to_string())],
        );
        assert!(matches!(
            FileGoneVerifier.verify(&present).await,
            Verdict::StillBroken { .. }
        ));

        std::fs::remove_file(&path).unwrap();
        assert_eq!(FileGoneVerifier.verify(&present).await, Verdict::Fixed);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_registry_only_claims_what_it_can_really_test() {
        let registry = VerifierRegistry::standard();
        assert!(
            registry
                .for_item(&item("service.stopped", &[("service", "cups")]))
                .is_some()
        );
        assert!(
            registry
                .for_item(&item("app.launch-stale-lock", &[("path", "/tmp/x.pid")]))
                .is_some()
        );
        // Nothing can re-test memory pressure, so nothing pretends to.
        assert!(registry.for_item(&item("memory.pressure", &[])).is_none());
    }

    #[test]
    fn coverage_counts_only_the_problems_that_can_be_fixed() {
        let registry = VerifierRegistry::standard();
        let testable = item("service.stopped", &[("service", "cups")]);
        let not_testable = item("memory.pressure", &[]);
        assert_eq!(registry.coverage([&testable, &not_testable]), 1);
        assert_eq!(VerifierRegistry::empty().coverage([&testable]), 0);
    }
}
