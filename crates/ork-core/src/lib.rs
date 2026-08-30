//! Diagnostic core for the Outlaw Repair Kit.
//!
//! This crate holds everything that is not a user interface: the platform
//! abstraction layer, the probe registry, and the scan orchestrator. The CLI,
//! the daemon, and the desktop app are all thin clients over this library --
//! nothing reachable from a front-end should be implemented in one.
//!
//! The two ideas worth knowing before reading further:
//!
//! * A [`Probe`] is one deterministic check. It declares which platforms it
//!   supports and which external tools it needs, and it is *skipped with a
//!   visible reason* rather than failing when those are absent.
//! * A [`Platform`] is the seam for everything OS-specific. Probes never call
//!   OS APIs directly; they go through this trait, so adding a new operating
//!   system is a new implementation rather than a rewrite.

pub mod commands;
pub mod config;
pub mod docs;
pub mod exec;
pub mod finding;
pub mod incident;
pub mod launch;
pub mod platform;
pub mod probe;
pub mod probes;
pub mod processes;
pub mod respond;
pub mod scan;
pub mod stress;
pub mod tier;
pub mod unseen;
pub mod util;
pub mod watch;
pub mod ways_in;

pub use config::Config;
pub use finding::{Category, Evidence, Finding, Severity, Triage};
pub use platform::{HostInfo, Platform, PlatformKind, ProcessInfo, Volume, VolumeRole};
pub use probe::{Probe, ProbeContext, ProbeMeta, ProbeOutcome, SkipReason};
pub use scan::{ScanReport, Scanner};
pub use tier::ScanTier;

/// Where this project lives.
///
/// One constant, in the crate every front-end already depends on, because
/// this address is used by the update check, the bug reporter, and the
/// documentation links -- and three copies of it is three chances for one to
/// go stale after a move.
pub const REPOSITORY: &str = "https://github.com/Sup095/outlaw-repair-kit";

/// Owner and name, for building API addresses.
pub const REPOSITORY_SLUG: &str = "Sup095/outlaw-repair-kit";

/// Result type used throughout the core.
pub type Result<T> = anyhow::Result<T>;

#[cfg(test)]
mod tests {
    #[test]
    fn the_two_forms_of_the_repository_address_agree() {
        // They are used to build different kinds of link, and a mismatch
        // would send the update check and the bug reporter to two different
        // projects.
        assert!(
            super::REPOSITORY.ends_with(super::REPOSITORY_SLUG),
            "{} does not end with {}",
            super::REPOSITORY,
            super::REPOSITORY_SLUG
        );
        assert!(super::REPOSITORY.starts_with("https://"));
    }
}
