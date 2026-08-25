//! Model routing and AI analysis for the Outlaw Repair Kit.
//!
//! This crate is separate from `ork-core` on purpose. The diagnostic core does
//! the real detection work and must keep working with no model available at
//! all, so it does not depend on this crate, does not link an HTTP client, and
//! cannot accidentally grow a dependency on a model being reachable. The
//! dependency runs one way: analysis reads findings, findings never ask for
//! analysis.
//!
//! The same boundary is what keeps the AI layer honest. It receives structured
//! findings that the deterministic probes already produced. It never gets
//! access to the machine.

pub mod analysis;
pub mod client;
pub mod router;
pub mod runbook;
pub mod secrets;

/// Result type used throughout this crate.
pub type Result<T> = anyhow::Result<T>;
