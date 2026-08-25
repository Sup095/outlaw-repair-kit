//! The probe registry.
//!
//! Adding a check to the tool means adding a module here and one line to
//! [`default_registry`]. Everything else -- tier gating, platform gating,
//! missing-tool skips, cancellation, error isolation -- is handled by the
//! orchestrator.

pub mod apps;
pub mod devices;
pub mod disk_health;
pub mod disk_space;
pub mod launchers;
pub mod logs;
pub mod memory;
pub mod processes;
pub mod services;
pub mod system_files;

use crate::probe::{Probe, ProbeMeta};

/// Every probe the tool knows about, in the order a scan runs them.
///
/// Ordering matters a little: cheap checks come first so that a user watching
/// a scan sees results immediately rather than staring at a spinner while an
/// expensive check runs.
pub fn default_registry() -> Vec<Box<dyn Probe>> {
    vec![
        Box::new(disk_space::DiskSpaceProbe),
        Box::new(memory::MemoryPressureProbe),
        Box::new(processes::ProcessesProbe),
        Box::new(devices::DeviceHealthProbe),
        Box::new(services::FailedServicesProbe),
        // Asking the drives about themselves. Cheap, but only in a Full scan:
        // "your disk is dying" is not a thing to tell somebody in passing.
        // Two probes, one per platform, because the two need different
        // permissions to answer the same question.
        Box::new(disk_health::DiskHealthProbe),
        Box::new(disk_health::SmartHealthProbe),
        // Launch tests start real programs, so they go after everything
        // that only observes.
        Box::new(apps::AppLaunchProbe),
        // Reading the system log costs a process spawn and a parse, so it
        // goes last among the Quick checks.
        Box::new(logs::RecentLogErrorsProbe),
        // Starts real applications, so it runs after everything else and only
        // in the Full tier.
        Box::new(launchers::LauncherProbe),
        // The Deep tier, and the slowest thing here by a wide margin: it
        // reads and hashes most of the operating system. Last, so that
        // everything quick has already been reported by the time it starts.
        Box::new(system_files::SystemFilesProbe),
        Box::new(system_files::PackageFilesProbe),
    ]
}

/// One check, described for a person, including whether it can run *here*.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CheckInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub category: String,
    pub tier: String,
    pub platforms: Vec<String>,
    pub requires_elevation: bool,
    pub required_tools: Vec<String>,
    /// Whether this machine can run it at all.
    pub available: bool,
    /// Why not, in the same words the scan would use.
    pub unavailable_reason: Option<String>,
}

/// Every check, and what this machine can do with it.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Catalogue {
    pub platform: String,
    pub elevated: bool,
    pub checks: Vec<CheckInfo>,
}

/// Describe every check against this machine.
///
/// Availability is decided by the scanner's own [`ProbeMeta::skip_reason`]
/// rather than by a second copy of the rules, and every front-end shares this
/// one function for the same reason: a screen that disagreed with the scan
/// about what will run would be worse than no screen.
///
/// Asked at the deepest tier, so what comes back is a real blocker -- the
/// platform, a missing tool, rights the process does not have -- rather than
/// "you asked for a quick scan". Which tier a check belongs to is reported
/// separately.
pub fn catalogue(platform: &dyn crate::Platform, elevated: bool) -> Catalogue {
    let checks = all_meta()
        .into_iter()
        .map(|meta| {
            let blocked = meta.skip_reason(crate::ScanTier::Deep, platform, elevated);
            CheckInfo {
                id: meta.id.to_string(),
                name: meta.name.to_string(),
                description: meta.description.to_string(),
                category: meta.category.as_str().to_string(),
                tier: meta.min_tier.as_str().to_string(),
                platforms: meta.platforms.iter().map(|kind| kind.to_string()).collect(),
                requires_elevation: meta.requires_elevation,
                required_tools: meta.requires_tools.iter().map(|t| t.to_string()).collect(),
                available: blocked.is_none(),
                unavailable_reason: blocked.map(|reason| reason.to_string()),
            }
        })
        .collect();

    Catalogue {
        platform: platform.kind().to_string(),
        elevated,
        checks,
    }
}

/// Metadata for every known probe, for settings screens and `--list-probes`.
pub fn all_meta() -> Vec<ProbeMeta> {
    default_registry()
        .iter()
        .map(|probe| probe.meta())
        .collect()
}
