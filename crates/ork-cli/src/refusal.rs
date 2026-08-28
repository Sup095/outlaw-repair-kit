//! Saying no, without calling it a fault.
//!
//! Every command that fails is recorded so that it can be turned into a bug
//! report afterwards. That is right for a command that broke, and wrong for a
//! command that worked perfectly and declined to do something.
//!
//! "That is not a credential this tool knows about", "say which machine with
//! `--at`", "`--json` cannot ask before heating the machine" -- all of those
//! are the tool doing its job. Filing them alongside crashes has two costs,
//! and the second is the serious one. The list of things worth reporting fills
//! up with things that are not, and somebody eventually posts one of them as
//! an issue, having been told by the program that it was worth reporting.
//!
//! So a refusal is still an error -- it still stops the command and still
//! exits non-zero, because a script has to be able to tell -- but it is a
//! *kind* of error, and the recorder knows the difference.

use std::fmt;

/// Something the tool will not do, as opposed to something that went wrong.
#[derive(Debug)]
pub struct Refusal(String);

impl Refusal {
    /// A refusal, already wrapped, because every caller wants it that way.
    ///
    /// Not called `new`: it does not return a `Refusal`, and a constructor
    /// that hands back something other than its own type is the sort of thing
    /// that reads fine here and surprises somebody two files away.
    pub fn saying(said: impl Into<String>) -> anyhow::Error {
        anyhow::Error::new(Self(said.into()))
    }

    /// Whether this error is a refusal rather than a fault.
    ///
    /// Looks through the whole chain, so a refusal that picked up context on
    /// its way up is still a refusal.
    pub fn is_one(error: &anyhow::Error) -> bool {
        error.chain().any(|cause| cause.is::<Self>())
    }
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Refusal {}

/// Refuse, with a reason.
macro_rules! refuse {
    ($($arg:tt)*) => {
        return Err($crate::refusal::Refusal::saying(format!($($arg)*)))
    };
}

pub(crate) use refuse;

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Context;

    #[test]
    fn a_refusal_is_recognised_as_one() {
        let said = Refusal::saying("no, and here is why");
        assert!(Refusal::is_one(&said));
        assert_eq!(said.to_string(), "no, and here is why");
    }

    #[test]
    fn an_ordinary_failure_is_not_a_refusal() {
        // The distinction the recorder acts on. Getting this wrong in this
        // direction would quietly stop real faults being recorded, which is
        // worse than the noise it was written to remove.
        let broke = anyhow::anyhow!("the disk went away");
        assert!(!Refusal::is_one(&broke));
    }

    #[test]
    fn a_refusal_wrapped_in_context_is_still_a_refusal() {
        let said: anyhow::Error = Refusal::saying("say which machine with --at");
        let wrapped = Err::<(), _>(said)
            .context("while working out which machine to ask")
            .unwrap_err();
        assert!(Refusal::is_one(&wrapped));
        // And the reason survives to the surface, so the person still reads
        // the sentence that matters.
        assert!(format!("{wrapped:#}").contains("--at"));
    }

    #[test]
    fn an_io_error_is_not_a_refusal_however_it_is_wrapped() {
        let broke: anyhow::Error =
            std::io::Error::new(std::io::ErrorKind::PermissionDenied, "nope").into();
        let wrapped = Err::<(), _>(broke).context("while reading").unwrap_err();
        assert!(!Refusal::is_one(&wrapped));
    }
}
