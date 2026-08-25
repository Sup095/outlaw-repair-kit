//! Linking two machines, so one can lend the other a model.
//!
//! The problem this solves: a computer with a weak graphics card can run every
//! diagnostic check perfectly well, but cannot run a model worth asking. A
//! stronger computer in the same house can. Getting the two to talk should not
//! require setting up a private network first.
//!
//! So: one machine shows a pairing code, the other is told the code, and from
//! then on they know each other. On a shared network they find each other
//! without anyone typing an address at all. A private network -- Tailscale,
//! WireGuard, anything -- still works and is still what you need to reach a
//! machine somewhere else, but it is no longer the price of entry.
//!
//! **What a link can and cannot do.** A linked machine can be asked to think
//! about a problem, and can be asked what its last scan found. It cannot be
//! made to do anything. There is no command in this protocol that changes the
//! machine at the other end -- not because it is blocked, but because it was
//! never written. Fixing is something you do at the keyboard of the machine
//! being fixed.

pub mod client;
pub mod code;
pub mod discovery;
pub mod pair;
pub mod peer;
pub mod routing;
pub mod server;

pub use code::PairingCode;
pub use peer::{Peer, PeerBook, Role};

/// The port a host listens on, chosen from the unassigned range.
pub const DEFAULT_PORT: u16 = 7341;

/// How long a pairing code is accepted for.
///
/// Long enough to walk to the other room, short enough that a code left on a
/// screen is not a standing invitation.
pub const PAIRING_WINDOW: std::time::Duration = std::time::Duration::from_secs(10 * 60);

/// How many wrong codes are accepted before the host stops listening.
///
/// A short code is only safe because guessing is not allowed to be cheap.
pub const MAX_PAIRING_ATTEMPTS: usize = 5;
