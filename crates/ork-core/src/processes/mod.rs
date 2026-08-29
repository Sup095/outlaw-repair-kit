//! Looking at what is running, and deciding what may be stopped.
//!
//! Nothing here stops anything. This is the enumeration and the judgement,
//! built and tested first precisely because it is the part where a mistake
//! does the damage -- see `docs/proposals/process-control.md` for what is
//! meant to be built on top of it.

pub mod in_front;
pub mod standing;
pub mod survey;

pub use in_front::InFront;
pub use standing::{Circumstances, Protection, Restraint, Standing, classify};
pub use survey::{Row, Survey};
