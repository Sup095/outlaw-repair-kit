//! The start-up sequence shared by the terminal and the desktop app.
//!
//! Both front-ends show a boot screen while the tool checks itself and asks
//! whether a newer release exists. The two screens look nothing alike, and
//! neither of them belongs in here: this crate decides *what* happens and in
//! what order, and reports each step as it goes. Drawing is the front-end's
//! job.
//!
//! That split is the whole point. The boot screen is a front-end, and no
//! behaviour lives in a front-end that cannot be reached programmatically --
//! so a headless run, a script, or a test can call [`boot`] and get the same
//! sequence with the progress reported to a callback that does nothing.

pub mod selftest;
pub mod update;

use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

pub use selftest::{CheckResult, CheckState, SelfTestReport};
pub use update::{CURRENT_VERSION, UpdateStatus};

/// The number of reported steps: every self-check, plus the update check.
pub const TOTAL_STEPS: usize = selftest::CHECK_COUNT + 1;

/// Something that happened during start-up, as it happens.
///
/// Every event carries enough to drive both a progress bar (`step` of
/// [`TOTAL_STEPS`]) and a rolling log pane (`line`), so a front-end does not
/// have to reconstruct either.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BootEvent {
    /// Start-up has begun. Nothing has been checked yet.
    Started { version: String, total_steps: usize },
    /// A self-check finished.
    Check {
        step: usize,
        total_steps: usize,
        result: CheckResult,
        line: String,
    },
    /// The update check finished.
    Update {
        step: usize,
        total_steps: usize,
        status: UpdateStatus,
        line: String,
    },
    /// Start-up is over. `ready` is false only if something is actually broken.
    Finished { ready: bool, line: String },
}

impl BootEvent {
    /// The line a log pane should show for this event.
    pub fn line(&self) -> &str {
        match self {
            BootEvent::Started { .. } => "starting",
            BootEvent::Check { line, .. }
            | BootEvent::Update { line, .. }
            | BootEvent::Finished { line, .. } => line,
        }
    }

    /// How far along start-up is, from 0.0 to 1.0.
    pub fn progress(&self) -> f32 {
        match self {
            BootEvent::Started { .. } => 0.0,
            BootEvent::Check {
                step, total_steps, ..
            }
            | BootEvent::Update {
                step, total_steps, ..
            } => *step as f32 / *total_steps as f32,
            BootEvent::Finished { .. } => 1.0,
        }
    }

    /// The state this event should be coloured by, if a front-end colours it.
    pub fn state(&self) -> CheckState {
        match self {
            BootEvent::Started { .. } => CheckState::Pass,
            BootEvent::Check { result, .. } => result.state,
            BootEvent::Update { status, .. } => match status {
                UpdateStatus::UpToDate { .. } => CheckState::Pass,
                // An available update is worth noticing but is not a problem,
                // and an unfinished check is not the user's business to fix.
                UpdateStatus::Available { .. } | UpdateStatus::Unknown { .. } => CheckState::Warn,
            },
            BootEvent::Finished { ready, .. } => {
                if *ready {
                    CheckState::Pass
                } else {
                    CheckState::Fail
                }
            }
        }
    }
}

/// What start-up concluded.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootReport {
    pub version: String,
    pub selftest: SelfTestReport,
    pub update: UpdateStatus,
    pub duration: Duration,
}

impl BootReport {
    /// Whether the tool is fit to be trusted with this machine.
    ///
    /// Warnings do not block: a missing credential store or an unreachable
    /// update server does not make the diagnostics wrong.
    pub fn ready(&self) -> bool {
        self.selftest.passed()
    }
}

/// Run the start-up sequence, reporting each step as it completes.
///
/// The update check runs alongside the self-checks rather than after them,
/// because it is the only step that waits on a network and there is no reason
/// to make the user wait on it on top of everything else.
pub async fn boot(mut on_event: impl FnMut(BootEvent)) -> BootReport {
    let started = Instant::now();

    on_event(BootEvent::Started {
        version: CURRENT_VERSION.to_string(),
        total_steps: TOTAL_STEPS,
    });

    let update_check = tokio::spawn(update::check());

    // The checks touch the disk and the platform, so they run on a blocking
    // thread. Their results are buffered rather than reported from inside the
    // closure, which keeps `boot`'s own callback off that thread and means it
    // does not have to be Send.
    //
    // `spawn_blocking` rather than `block_in_place`: the latter panics on a
    // single-threaded runtime, and a start-up sequence that brings down the
    // program depending on how the caller built its runtime is not one.
    let (selftest, steps) = tokio::task::spawn_blocking(|| {
        let mut steps = Vec::with_capacity(selftest::CHECK_COUNT);
        let report = selftest::run(|result, index, _| steps.push((index, result.clone())));
        (report, steps)
    })
    .await
    .expect("the self-test panicked");

    for (index, result) in steps {
        let line = format!(
            "[{}] {} -- {}",
            result.state.as_str(),
            result.name,
            result.detail
        );
        on_event(BootEvent::Check {
            step: index,
            total_steps: TOTAL_STEPS,
            result,
            line,
        });
    }

    let update = update_check
        .await
        // The update check cannot itself fail, so a join error means the task
        // was cancelled or panicked -- neither of which should stop start-up.
        .unwrap_or_else(|error| UpdateStatus::Unknown {
            reason: error.to_string(),
        });
    on_event(BootEvent::Update {
        step: TOTAL_STEPS,
        total_steps: TOTAL_STEPS,
        line: update.summary(),
        status: update.clone(),
    });

    let report = BootReport {
        version: CURRENT_VERSION.to_string(),
        selftest,
        update,
        duration: started.elapsed(),
    };

    let line = if report.ready() {
        let warnings = report.selftest.warnings().count();
        if warnings == 0 {
            "all systems ready".to_string()
        } else {
            format!("ready, with {warnings} warning(s)")
        }
    } else {
        format!("{} check(s) failed", report.selftest.failures().count())
    };
    on_event(BootEvent::Finished {
        ready: report.ready(),
        line,
    });

    report
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "multi_thread")]
    async fn boot_reports_every_step_once_and_finishes_at_full_progress() {
        let mut events = Vec::new();
        let report = boot(|event| events.push(event)).await;

        assert!(matches!(events.first(), Some(BootEvent::Started { .. })));
        assert!(matches!(events.last(), Some(BootEvent::Finished { .. })));

        let checks = events
            .iter()
            .filter(|e| matches!(e, BootEvent::Check { .. }))
            .count();
        assert_eq!(checks, selftest::CHECK_COUNT);
        assert_eq!(
            events
                .iter()
                .filter(|e| matches!(e, BootEvent::Update { .. }))
                .count(),
            1
        );

        // Progress must never go backwards, or a bar drawn from it jumps about.
        let mut last = 0.0;
        for event in &events {
            assert!(
                event.progress() >= last,
                "progress went backwards at {event:?}"
            );
            last = event.progress();
        }
        assert_eq!(last, 1.0);

        assert_eq!(report.version, CURRENT_VERSION);
        assert_eq!(report.selftest.checks.len(), selftest::CHECK_COUNT);
    }

    /// Deliberately on a single-threaded runtime: start-up must not depend on
    /// how the caller built theirs.
    #[tokio::test(flavor = "current_thread")]
    async fn every_event_carries_a_line_for_the_log_pane() {
        let mut events = Vec::new();
        boot(|event| events.push(event)).await;
        for event in &events {
            assert!(
                !event.line().trim().is_empty(),
                "{event:?} has nothing to show"
            );
        }
    }
}
