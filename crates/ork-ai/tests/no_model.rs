//! The tool has to work with no model and no API key at all.
//!
//! This is not a degraded mode that happens to work; it is the mode most
//! people will run in. The deterministic checks find problems without help,
//! and the runbook library explains them without help. A model is an addition
//! for the cases nobody has written an answer for.
//!
//! Everything here runs with routing that has no client, which is exactly what
//! a machine with nothing configured produces. Nothing in this file may reach
//! the network, and that is the point: if any of it ever needed to, the tool
//! would have quietly acquired a dependency on somebody else's server in order
//! to say what is wrong with a computer that may not have a working one.

use std::time::Duration;

use ork_ai::analysis::{AnalysisSource, Analyst};
use ork_ai::router::Routing;
use ork_ai::runbook::RunbookLibrary;
use ork_core::finding::{Category, Finding, Severity, Triage};
use ork_core::platform::{HostInfo, PlatformKind};
use ork_core::probe::{ProbeOutcome, ProbeStatus};
use ork_core::scan::ScanReport;
use ork_core::tier::ScanTier;
use time::OffsetDateTime;

/// What the router produces on a machine with nothing set up: no client, no
/// tier, and nothing to fall back to.
fn nothing_configured() -> Routing {
    Routing {
        client: None,
        tier: None,
        attempts: Vec::new(),
    }
}

fn report(findings: Vec<Finding>) -> ScanReport {
    ScanReport {
        tier: ScanTier::Quick,
        host: HostInfo {
            hostname: "test-machine".to_string(),
            os_name: "Test OS".to_string(),
            os_version: "1".to_string(),
            kernel_version: "1".to_string(),
            arch: "x86_64".to_string(),
            cpu_brand: "Test".to_string(),
            physical_cores: Some(4),
            logical_cores: 8,
            total_memory_bytes: 16 * 1024 * 1024 * 1024,
        },
        started_at: OffsetDateTime::now_utc(),
        duration: Duration::from_secs(1),
        cancelled: false,
        elevated: false,
        outcomes: vec![ProbeOutcome {
            probe: "test".to_string(),
            name: "Test probe".to_string(),
            status: ProbeStatus::Completed,
            skipped_because: None,
            findings,
            duration: Duration::from_secs(1),
        }],
    }
}

fn known_problem() -> Finding {
    Finding::builder("services.failed", "service.stopped")
        .subject("steam")
        .severity(Severity::High)
        .category(Category::Application)
        .title("A service that should be running is not")
        .detail("It is set to start automatically and it is stopped.")
        .evidence("service", "steam")
        .triage(Triage::Queue)
        .build()
}

fn unknown_problem() -> Finding {
    Finding::builder("system.processes", "process.memory-hog")
        .subject("something.exe")
        .severity(Severity::Medium)
        .category(Category::Memory)
        .title("One process holds most of the machine's memory")
        .detail("Deliberately one of the findings with no canned answer.")
        .triage(Triage::Queue)
        .build()
}

#[tokio::test]
async fn a_known_problem_is_explained_with_no_model_at_all() {
    let analyst = Analyst::new(
        RunbookLibrary::built_in().unwrap(),
        PlatformKind::Linux.as_str(),
    );
    let analysis = analyst
        .analyse(&report(vec![known_problem()]), &nothing_configured())
        .await
        .expect("analysis must not need a model");

    assert_eq!(analysis.answered_by_runbook, 1);
    assert_eq!(analysis.items.len(), 1);

    let answered = &analysis.items[0];
    assert!(matches!(answered.source, AnalysisSource::Runbook { .. }));
    assert!(
        !answered.explanation.trim().is_empty(),
        "a runbook answer with no words in it is not an answer"
    );
    assert!(
        !answered.fixes.is_empty(),
        "the point of a runbook is the ranked things to try"
    );
    assert!(
        analysis.model.is_none(),
        "no model was available, so none may be claimed"
    );
}

#[tokio::test]
async fn a_problem_with_no_runbook_answer_is_left_unexplained_rather_than_invented() {
    // The honest outcome. With no model and no prepared answer, the finding
    // still reaches the person -- the scan found it, and that stands on its
    // own -- but nothing makes something up to fill the gap.
    let analyst = Analyst::new(
        RunbookLibrary::built_in().unwrap(),
        PlatformKind::Linux.as_str(),
    );
    let analysis = analyst
        .analyse(&report(vec![unknown_problem()]), &nothing_configured())
        .await
        .expect("analysis must not need a model");

    assert_eq!(analysis.answered_by_runbook, 0);
    assert_eq!(
        analysis.unexplained(),
        1,
        "the finding should be reported as having no explanation"
    );
    assert!(analysis.model.is_none());
}

#[tokio::test]
async fn a_mixed_report_answers_what_it_can_and_says_what_it_could_not() {
    let analyst = Analyst::new(
        RunbookLibrary::built_in().unwrap(),
        PlatformKind::Linux.as_str(),
    );
    let analysis = analyst
        .analyse(
            &report(vec![known_problem(), unknown_problem()]),
            &nothing_configured(),
        )
        .await
        .expect("analysis must not need a model");

    assert_eq!(analysis.answered_by_runbook, 1);
    assert_eq!(analysis.unexplained(), 1);
    assert_eq!(
        analysis.items.len(),
        2,
        "both findings must survive, explained or not"
    );
}

#[tokio::test]
async fn an_empty_report_is_not_an_error() {
    let analyst = Analyst::new(
        RunbookLibrary::built_in().unwrap(),
        PlatformKind::Linux.as_str(),
    );
    let analysis = analyst
        .analyse(&report(Vec::new()), &nothing_configured())
        .await
        .expect("nothing to explain is an ordinary outcome");
    assert!(analysis.items.is_empty());
    assert_eq!(analysis.answered_by_runbook, 0);
}

#[test]
fn the_routing_says_plainly_that_there_is_no_model() {
    // What every screen prints when nothing is configured. It has to name the
    // situation rather than look like a failure, because for most people this
    // is not one.
    let summary = nothing_configured().summary();
    assert!(summary.contains("no model available"), "got: {summary}");
    assert!(
        summary.contains("runbooks"),
        "it should say what still works: {summary}"
    );
}
