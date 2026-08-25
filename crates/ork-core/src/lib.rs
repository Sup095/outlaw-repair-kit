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

pub mod config;
pub mod exec;
pub mod finding;
pub mod launch;
pub mod platform;
pub mod probe;
pub mod probes;
pub mod respond;
pub mod scan;
pub mod tier;
pub mod util;

pub use config::Config;
pub use finding::{Category, Evidence, Finding, Severity, Triage};
pub use platform::{HostInfo, Platform, PlatformKind, ProcessInfo, Volume, VolumeRole};
pub use probe::{Probe, ProbeContext, ProbeMeta, ProbeOutcome, SkipReason};
pub use scan::{ScanReport, Scanner};
pub use tier::ScanTier;

/// Result type used throughout the core.
pub type Result<T> = anyhow::Result<T>;
