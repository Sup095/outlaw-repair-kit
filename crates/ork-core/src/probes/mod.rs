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
    ]
}

/// Metadata for every known probe, for settings screens and `--list-probes`.
pub fn all_meta() -> Vec<ProbeMeta> {
    default_registry()
        .iter()
        .map(|probe| probe.meta())
        .collect()
}
