//! The probe registry.
//!
//! Adding a check to the tool means adding a module here and one line to
//! [`default_registry`]. Everything else -- tier gating, platform gating,
//! missing-tool skips, cancellation, error isolation -- is handled by the
//! orchestrator.

pub mod disk_space;
pub mod logs;
pub mod memory;

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
        // Reading the system log costs a process spawn and a parse, so it
        // goes last among the Quick checks.
        Box::new(logs::RecentLogErrorsProbe),
    ]
}

/// Metadata for every known probe, for settings screens and `--list-probes`.
pub fn all_meta() -> Vec<ProbeMeta> {
    default_registry()
        .iter()
        .map(|probe| probe.meta())
        .collect()
}
