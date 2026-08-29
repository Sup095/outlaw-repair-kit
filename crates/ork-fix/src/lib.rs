//! The triage queue and the fix engine.
//!
//! Detection tells you what is wrong. This crate is what tries to do something
//! about it, and it is the only part of the tool that changes your machine --
//! which is why almost all of it is about restraint rather than capability.
//!
//! The shape of the thing:
//!
//! * Simple, unambiguous problems are fixed during the scan.
//! * Anything complex or ambiguous goes on a **triage queue** with full
//!   context, instead of blocking the scan.
//! * Afterwards the queue is worked one item at a time, in priority order.
//!   For each item: take a snapshot, apply one candidate fix, test whether it
//!   worked, and roll back if it did not. Then the next candidate, until
//!   something works or the list is exhausted.
//! * Nothing is given a deadline. The loop keeps going until it succeeds or
//!   runs out of ideas.
//!
//! The safety rails live in [`action`], which is worth reading before anything
//! else here.

pub mod action;
pub mod engine;
pub mod plan;
pub mod processes;
pub mod snapshot;
pub mod store;
pub mod verify;

/// Result type used throughout this crate.
pub type Result<T> = anyhow::Result<T>;
