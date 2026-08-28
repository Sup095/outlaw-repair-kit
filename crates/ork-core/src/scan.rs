//! Scan orchestration.
//!
//! The scanner decides which probes run for a given tier, runs them, and
//! collects the results into a report. It never aborts the whole scan because
//! one probe broke, and it never stops work for taking too long -- only the
//! user's cancellation ends a scan early.

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::Result;
use crate::finding::{Finding, Severity};
use crate::platform::{HostInfo, Platform};
use crate::probe::{Probe, ProbeContext, ProbeOutcome, ProbeStatus};
use crate::tier::ScanTier;

/// Progress emitted while a scan runs, for a live UI or a CLI progress line.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "kebab-case")]
pub enum ScanEvent {
    Started {
        tier: ScanTier,
        probe_count: usize,
    },
    ProbeStarted {
        probe: String,
        name: String,
        index: usize,
        total: usize,
    },
    ProbeFinished {
        outcome: Box<ProbeOutcome>,
    },
    Finished {
        finding_count: usize,
        cancelled: bool,
    },
}

/// Everything one scan produced.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanReport {
    pub tier: ScanTier,
    pub host: HostInfo,
    #[serde(with = "time::serde::rfc3339")]
    pub started_at: OffsetDateTime,
    pub duration: Duration,
    /// True when the user cancelled before every probe had run.
    pub cancelled: bool,
    /// Whether the scan had administrator or root rights.
    pub elevated: bool,
    /// One entry per probe considered, including the ones that were skipped.
    pub outcomes: Vec<ProbeOutcome>,
}

impl ScanReport {
    /// Every finding from every probe, worst first.
    pub fn findings(&self) -> Vec<&Finding> {
        let mut findings: Vec<&Finding> = self
            .outcomes
            .iter()
            .flat_map(|outcome| outcome.findings.iter())
            .collect();
        // Descending severity, then a stable alphabetical order within a
        // severity so repeated scans of an unchanged machine read identically.
        findings.sort_by(|a, b| {
            b.severity
                .cmp(&a.severity)
                .then_with(|| a.id.cmp(&b.id))
                .then_with(|| {
                    a.subject
                        .as_deref()
                        .unwrap_or("")
                        .cmp(b.subject.as_deref().unwrap_or(""))
                })
        });
        findings
    }

    pub fn finding_count(&self) -> usize {
        self.outcomes
            .iter()
            .map(|outcome| outcome.findings.len())
            .sum()
    }

    /// The worst severity seen, or `None` for a clean scan.
    pub fn worst_severity(&self) -> Option<Severity> {
        self.outcomes
            .iter()
            .flat_map(|outcome| outcome.findings.iter())
            .map(|finding| finding.severity)
            .max()
    }

    pub fn skipped(&self) -> impl Iterator<Item = &ProbeOutcome> {
        self.outcomes
            .iter()
            .filter(|outcome| matches!(outcome.status, ProbeStatus::Skipped(_)))
    }

    pub fn failed(&self) -> impl Iterator<Item = &ProbeOutcome> {
        self.outcomes
            .iter()
            .filter(|outcome| matches!(outcome.status, ProbeStatus::Failed { .. }))
    }
}

/// Runs probes and produces a [`ScanReport`].
pub struct Scanner {
    platform: Arc<dyn Platform>,
    probes: Vec<Box<dyn Probe>>,
    elevated: bool,
    cancel: CancellationToken,
    events: Option<mpsc::UnboundedSender<ScanEvent>>,
}

impl Scanner {
    /// Build a scanner with the default probe registry for this platform.
    pub fn new() -> Result<Self> {
        let platform = crate::platform::detect()?;
        Ok(Self::with_probes(
            platform,
            crate::probes::default_registry(),
        ))
    }

    pub fn with_probes(platform: Arc<dyn Platform>, probes: Vec<Box<dyn Probe>>) -> Self {
        Self {
            platform,
            probes,
            // Asked, not assumed. This defaulted to `false` and nothing ever
            // set it, so every check needing elevation was skipped for want
            // of rights the process already had -- the SMART disk check could
            // not run even as root.
            elevated: crate::platform::is_elevated(),
            cancel: CancellationToken::new(),
            events: None,
        }
    }

    /// Override what the scanner believes about its own rights.
    ///
    /// Detected by default. This exists for the daemon, which deliberately
    /// runs unprivileged and obtains elevation per action through a separate
    /// helper -- so what it may do is not what its own process token says.
    pub fn elevated(mut self, elevated: bool) -> Self {
        self.elevated = elevated;
        self
    }

    /// Receive live progress events.
    pub fn with_events(mut self, sender: mpsc::UnboundedSender<ScanEvent>) -> Self {
        self.events = Some(sender);
        self
    }

    /// The token that cancels this scan. Cancellation is always the user's
    /// choice; nothing in this tool cancels work on a timer.
    pub fn cancel_token(&self) -> CancellationToken {
        self.cancel.clone()
    }

    pub fn platform(&self) -> &Arc<dyn Platform> {
        &self.platform
    }

    fn emit(&self, event: ScanEvent) {
        if let Some(sender) = &self.events {
            // A dropped receiver means nobody is watching progress any more.
            // That is not a reason to interrupt the scan.
            let _ = sender.send(event);
        }
    }

    /// Run every probe that applies at `tier`.
    ///
    /// Probes run one at a time on purpose: several of them measure CPU, I/O,
    /// and memory pressure, and running them concurrently would have them
    /// measure each other. Probes that are safe to overlap will opt in
    /// explicitly rather than being parallelised by default.
    pub async fn run(&self, tier: ScanTier) -> Result<ScanReport> {
        let started_at = OffsetDateTime::now_utc();
        let clock = Instant::now();

        let host = {
            let platform = Arc::clone(&self.platform);
            tokio::task::spawn_blocking(move || platform.host()).await??
        };

        let ctx = ProbeContext::new(
            Arc::clone(&self.platform),
            self.cancel.clone(),
            self.elevated,
        );

        self.emit(ScanEvent::Started {
            tier,
            probe_count: self.probes.len(),
        });

        let total = self.probes.len();
        let mut outcomes = Vec::with_capacity(total);
        let mut cancelled = false;

        for (index, probe) in self.probes.iter().enumerate() {
            let meta = probe.meta();

            if let Some(reason) = meta.skip_reason(tier, self.platform.as_ref(), self.elevated) {
                let outcome = ProbeOutcome::skipped(&meta, reason);
                self.emit(ScanEvent::ProbeFinished {
                    outcome: Box::new(outcome.clone()),
                });
                outcomes.push(outcome);
                continue;
            }

            if self.cancel.is_cancelled() {
                cancelled = true;
                outcomes.push(ProbeOutcome {
                    probe: meta.id.to_string(),
                    name: meta.name.to_string(),
                    status: ProbeStatus::Cancelled,
                    skipped_because: None,
                    findings: Vec::new(),
                    duration: Duration::ZERO,
                });
                continue;
            }

            self.emit(ScanEvent::ProbeStarted {
                probe: meta.id.to_string(),
                name: meta.name.to_string(),
                index,
                total,
            });

            let probe_clock = Instant::now();
            let result = probe.run(&ctx).await;
            let duration = probe_clock.elapsed();

            let (status, findings) = match result {
                Ok(findings) => (ProbeStatus::Completed, findings),
                Err(error) if self.cancel.is_cancelled() => {
                    cancelled = true;
                    tracing::debug!(probe = meta.id, %error, "probe ended during cancellation");
                    (ProbeStatus::Cancelled, Vec::new())
                }
                Err(error) => {
                    // One broken probe must not cost the user the rest of the
                    // scan. Record it and carry on.
                    tracing::warn!(probe = meta.id, %error, "probe failed");
                    (
                        ProbeStatus::Failed {
                            error: format!("{error:#}"),
                        },
                        Vec::new(),
                    )
                }
            };

            // A probe declares what it can report, and this is what keeps
            // that declaration honest. Without it the list is a comment: it
            // would go stale the first time somebody added a finding, and the
            // things built on top of it -- chiefly the check that every
            // finding has a prepared answer -- would quietly stop covering
            // the new one. Warned about always, and a hard failure under test,
            // so it is caught by whichever test happens to run the probe
            // rather than needing a test of its own for each.
            for finding in &findings {
                if !meta.emits.contains(&finding.id.as_str()) {
                    tracing::warn!(
                        probe = meta.id,
                        finding = %finding.id,
                        "probe reported a finding it does not declare"
                    );
                    debug_assert!(
                        false,
                        "probe `{}` reported `{}`, which is not in its `emits` list",
                        meta.id, finding.id
                    );
                }
            }

            let outcome = ProbeOutcome {
                probe: meta.id.to_string(),
                name: meta.name.to_string(),
                // A probe that got as far as running did not skip. Anything
                // that skips is built by `ProbeOutcome::skipped`, which is the
                // only place this is set.
                skipped_because: None,
                status,
                findings,
                duration,
            };
            self.emit(ScanEvent::ProbeFinished {
                outcome: Box::new(outcome.clone()),
            });
            outcomes.push(outcome);
        }

        let report = ScanReport {
            tier,
            host,
            started_at,
            duration: clock.elapsed(),
            cancelled: cancelled || self.cancel.is_cancelled(),
            elevated: self.elevated,
            outcomes,
        };

        self.emit(ScanEvent::Finished {
            finding_count: report.finding_count(),
            cancelled: report.cancelled,
        });

        Ok(report)
    }
}
