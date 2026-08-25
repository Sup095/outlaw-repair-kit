//! The unit of detection.
//!
//! A probe is one deterministic check. Probes are the bulk of the real work in
//! this tool -- the AI layer reasons about what probes found, and never
//! replaces a check that can be made deterministically.
//!
//! A probe declares what it needs up front. If the platform is wrong, a tool
//! is missing, or elevation it requires was not granted, the orchestrator
//! records a visible *skip with a reason* instead of running it. A scan that
//! quietly covered less than the user thinks it did is worse than one that
//! says so.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::Result;
use crate::finding::{Category, Finding};
use crate::platform::{Platform, PlatformKind};
use crate::tier::ScanTier;

/// What a probe is and what it needs in order to run.
#[derive(Debug, Clone)]
pub struct ProbeMeta {
    /// Stable identifier, e.g. `storage.disk-space`. Used in configuration and
    /// in the audit log, so it must not change casually.
    pub id: &'static str,
    /// Short human-readable name for the UI.
    pub name: &'static str,
    /// One line describing what this probe checks, shown in the UI next to the
    /// toggle that enables it.
    pub description: &'static str,
    pub category: Category,
    /// The lowest scan tier that runs this probe.
    pub min_tier: ScanTier,
    /// Platforms this probe supports.
    pub platforms: &'static [PlatformKind],
    /// External executables this probe needs. Missing ones cause a skip.
    pub requires_tools: &'static [&'static str],
    /// Whether this probe needs administrator or root rights.
    pub requires_elevation: bool,
}

/// Why a probe did not run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "reason", rename_all = "kebab-case")]
pub enum SkipReason {
    /// The probe does not support this operating system.
    UnsupportedPlatform { platform: PlatformKind },
    /// A required external tool is not installed.
    MissingTool { tool: String },
    /// The probe needs elevation and the scan is not running elevated.
    RequiresElevation,
    /// The probe is above the requested scan tier.
    AboveTier { min_tier: ScanTier },
    /// The user turned this probe off.
    DisabledByUser,
}

impl std::fmt::Display for SkipReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SkipReason::UnsupportedPlatform { platform } => {
                write!(f, "not supported on {platform}")
            }
            SkipReason::MissingTool { tool } => write!(f, "`{tool}` is not installed"),
            SkipReason::RequiresElevation => {
                f.write_str("needs administrator rights, which were not granted")
            }
            SkipReason::AboveTier { min_tier } => write!(f, "only runs in a {min_tier} scan"),
            SkipReason::DisabledByUser => f.write_str("turned off in settings"),
        }
    }
}

/// How a probe finished.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum ProbeStatus {
    /// Ran to completion. Zero findings means a clean result, not a failure.
    Completed,
    /// Did not run. The reason is shown to the user.
    Skipped(SkipReason),
    /// Ran and broke. The scan continues; one broken probe does not lose the
    /// rest of the results.
    Failed { error: String },
    /// The user cancelled the scan while this probe was running.
    Cancelled,
}

/// The result of one probe within a scan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeOutcome {
    pub probe: String,
    pub name: String,
    pub status: ProbeStatus,
    pub findings: Vec<Finding>,
    pub duration: Duration,
}

impl ProbeOutcome {
    pub fn skipped(meta: &ProbeMeta, reason: SkipReason) -> Self {
        Self {
            probe: meta.id.to_string(),
            name: meta.name.to_string(),
            status: ProbeStatus::Skipped(reason),
            findings: Vec::new(),
            duration: Duration::ZERO,
        }
    }
}

/// What a probe is given when it runs.
#[derive(Clone)]
pub struct ProbeContext {
    platform: Arc<dyn Platform>,
    cancel: CancellationToken,
    elevated: bool,
}

impl ProbeContext {
    pub fn new(platform: Arc<dyn Platform>, cancel: CancellationToken, elevated: bool) -> Self {
        Self {
            platform,
            cancel,
            elevated,
        }
    }

    /// The platform implementation for this machine.
    ///
    /// Prefer [`ProbeContext::blocking`] for anything that actually touches the
    /// OS, so a slow syscall cannot stall the async runtime.
    pub fn platform(&self) -> &Arc<dyn Platform> {
        &self.platform
    }

    /// Whether the scan is running with administrator or root rights.
    pub fn is_elevated(&self) -> bool {
        self.elevated
    }

    /// The scan's cancellation token.
    ///
    /// Long-running probes must poll this. It is the *user's* manual cancel --
    /// nothing in this tool cancels work for taking too long.
    pub fn cancel_token(&self) -> &CancellationToken {
        &self.cancel
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }

    /// Run a blocking platform call off the async runtime.
    pub async fn blocking<T, F>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&dyn Platform) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let platform = Arc::clone(&self.platform);
        tokio::task::spawn_blocking(move || f(platform.as_ref())).await?
    }
}

/// One deterministic check.
#[async_trait]
pub trait Probe: Send + Sync + 'static {
    fn meta(&self) -> ProbeMeta;

    /// Run the check.
    ///
    /// Returning an empty vector means "checked, nothing wrong". Return an
    /// error only when the check itself could not be performed; the
    /// orchestrator records that without aborting the scan.
    async fn run(&self, ctx: &ProbeContext) -> Result<Vec<Finding>>;
}

impl ProbeMeta {
    /// Decide whether this probe should run, given the scan's conditions.
    pub fn skip_reason(
        &self,
        tier: ScanTier,
        platform: &dyn Platform,
        elevated: bool,
    ) -> Option<SkipReason> {
        if !tier.includes(self.min_tier) {
            return Some(SkipReason::AboveTier {
                min_tier: self.min_tier,
            });
        }
        let kind = platform.kind();
        if !self.platforms.contains(&kind) {
            return Some(SkipReason::UnsupportedPlatform { platform: kind });
        }
        if self.requires_elevation && !elevated {
            return Some(SkipReason::RequiresElevation);
        }
        for tool in self.requires_tools {
            if !platform.tool_available(tool) {
                return Some(SkipReason::MissingTool {
                    tool: (*tool).to_string(),
                });
            }
        }
        None
    }
}
