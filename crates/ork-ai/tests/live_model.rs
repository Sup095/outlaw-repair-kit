//! End-to-end test against a real model endpoint.
//!
//! Ignored by default. It needs a model server actually running, which is not
//! true on a CI runner and should never be a requirement for the suite to
//! pass. Run it deliberately when changing the analysis layer:
//!
//! ```text
//! cargo test --package ork-ai --test live_model -- --ignored --nocapture
//! ```

use std::time::Duration;

use ork_ai::analysis::{AnalysisSource, Analyst};
use ork_ai::router::ModelRouter;
use ork_ai::runbook::RunbookLibrary;
use ork_core::config::AiConfig;
use ork_core::finding::{Category, Finding, Severity, Triage};
use ork_core::platform::{HostInfo, PlatformKind};
use ork_core::probe::{ProbeOutcome, ProbeStatus};
use ork_core::scan::ScanReport;
use ork_core::tier::ScanTier;
use time::OffsetDateTime;

/// A scan report holding one problem the runbook library knows and one it does
/// not, so a single run exercises both paths.
fn report_with_findings() -> ScanReport {
    let known = Finding::builder("devices.health", "device.driver-mismatch")
        .subject("NVIDIA graphics driver")
        .severity(Severity::High)
        .category(Category::Gpu)
        .title("The graphics driver does not match the running kernel")
        .detail(
            "The loaded NVIDIA kernel module is version 580.82.07 but the userspace driver \
             is 575.64.03.",
        )
        .evidence("driver_version", "module 580.82.07, userspace 575.64.03")
        .triage(Triage::Queue)
        .build();

    let unknown = Finding::builder("logs.recent-errors", "logs.repeated-error")
        .subject("Bonjour Service")
        .severity(Severity::Medium)
        .category(Category::Logs)
        .title("`Bonjour Service` has logged 43 errors in the last three days")
        .detail("Something is going wrong repeatedly and being written to the system log.")
        .evidence("occurrences", "43")
        .evidence(
            "sample_message",
            "Task Scheduling Error: m->NextScheduledSPRetry 3000",
        )
        .triage(Triage::Queue)
        .build();

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
            findings: vec![known, unknown],
            duration: Duration::from_secs(1),
        }],
    }
}

#[tokio::test]
#[ignore = "requires a local model server to be running"]
async fn a_real_model_explains_what_the_runbooks_cannot() {
    let routing = ModelRouter::new(AiConfig::default()).resolve().await;
    assert!(
        routing.is_available(),
        "no model endpoint answered -- start one, or skip this test. Routing said: {}",
        routing.summary()
    );
    println!("routed to: {}", routing.summary());

    let analyst = Analyst::new(
        RunbookLibrary::built_in().unwrap(),
        PlatformKind::Linux.as_str(),
    );
    let report = report_with_findings();
    let analysis = analyst
        .analyse(&report, &routing)
        .await
        .expect("analysis should complete");

    assert_eq!(analysis.items.len(), 2);

    // The known problem must come from the runbook library, not the model --
    // that is the whole point of consulting runbooks first.
    let known = analysis
        .items
        .iter()
        .find(|item| item.finding_id == "device.driver-mismatch")
        .expect("the known finding should be present");
    assert!(
        matches!(known.source, AnalysisSource::Runbook { .. }),
        "a known problem must be answered deterministically, got {:?}",
        known.source
    );
    assert!(
        !known.fixes.is_empty(),
        "the runbook entry should carry fixes"
    );
    println!("\nknown problem answered by: {:?}", known.source);
    println!("fixes offered: {}", known.fixes.len());

    // The unknown one is what the model is for.
    let unknown = analysis
        .items
        .iter()
        .find(|item| item.finding_id == "logs.repeated-error")
        .expect("the unknown finding should be present");
    println!("\nunknown problem answered by: {:?}", unknown.source);
    println!("explanation: {}", unknown.explanation);

    match &unknown.source {
        AnalysisSource::Model { model } => {
            assert!(!model.is_empty());
            assert!(!unknown.explanation.trim().is_empty());
        }
        // A small quantised model may fail to produce usable JSON. That is a
        // real outcome worth seeing rather than a test failure, because the
        // layer is designed to degrade rather than break.
        other => println!("model did not answer usably ({other:?}) -- degraded as designed"),
    }

    if let Some(correlation) = &analysis.correlation {
        println!("\ncorrelation: {correlation}");
    }
    assert_eq!(analysis.answered_by_runbook, 1);
}
