//! Telling whether a fix actually worked.
//!
//! The fix engine will not apply a change it cannot test. That is not a
//! limitation to be worked around -- it is the rule that makes automated
//! fixing safe to offer at all, because a change nobody checked is not a
//! repair, it is a hope. This module is what turns "we cannot test that" into
//! "we can".
//!
//! Every verifier here re-runs **the same test that produced the finding**.
//! That matters more than it sounds: if the check that found the problem and
//! the check that declares it solved are different tests, then "fixed" means
//! something other than "not found any more", and the difference is exactly
//! where a tool starts lying to people.
//!
//! Verifiers answer one of three ways, and the third is the important one. A
//! verifier that cannot carry out its test says so, and the engine rolls the
//! change back -- because an unverified change to somebody's machine is not an
//! improvement, however plausible it looked.

use async_trait::async_trait;
use ork_core::PlatformKind;

use crate::engine::{Verdict, Verifier};
use crate::store::TriageItem;

pub mod launch;

pub use launch::LaunchVerifier;
// Re-exported so a caller does not have to know that the launch test itself
// lives in the core, while the judgement about it lives here.
pub use ork_core::launch::{LaunchResult, LaunchTarget, LaunchTester, RealLaunchTester};

/// How much captured output to quote back when reporting a failure.
const EXCERPT: usize = 400;

pub(crate) fn excerpt(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= EXCERPT {
        return trimmed.to_string();
    }
    let cut: String = trimmed.chars().take(EXCERPT).collect();
    format!("{cut}...")
}

pub(crate) fn this_platform() -> PlatformKind {
    ork_core::platform::detect()
        .map(|platform| platform.kind())
        .unwrap_or(PlatformKind::Linux)
}

/// Everything this build knows how to re-test.
///
/// The engine asks this for a verifier *before* it applies anything. When the
/// answer is `None` the change is offered as advice instead of being made --
/// so adding a verifier here is what promotes a class of problem from
/// "described to you" to "fixed for you".
pub struct VerifierRegistry {
    verifiers: Vec<Box<dyn ItemVerifier>>,
}

/// A verifier that knows which problems it can speak to.
pub trait ItemVerifier: Verifier {
    /// Whether this verifier can re-test that particular problem.
    ///
    /// Answering `true` is a promise that [`Verifier::verify`] will carry out
    /// a real test, not that it will guess well.
    fn handles(&self, item: &TriageItem) -> bool;
}

impl VerifierRegistry {
    /// The verifiers this build ships with.
    pub fn standard() -> Self {
        Self {
            verifiers: vec![
                Box::new(LaunchVerifier::new(RealLaunchTester::default())),
                Box::new(FileGoneVerifier),
            ],
        }
    }

    pub fn empty() -> Self {
        Self {
            verifiers: Vec::new(),
        }
    }

    pub fn with(mut self, verifier: Box<dyn ItemVerifier>) -> Self {
        self.verifiers.push(verifier);
        self
    }

    /// The verifier for this problem, if there is one.
    ///
    /// Deliberately an `Option` rather than a fallback that always answers.
    /// A verifier that cannot really test something would make the engine
    /// apply a change and then roll it back, which is worse than never having
    /// touched the machine.
    pub fn for_item(&self, item: &TriageItem) -> Option<&dyn Verifier> {
        self.verifiers
            .iter()
            .find(|verifier| verifier.handles(item))
            .map(|verifier| verifier.as_ref() as &dyn Verifier)
    }

    /// How many problems in a list this build could actually fix rather than
    /// describe. Worth showing people up front.
    pub fn coverage<'a>(&self, items: impl IntoIterator<Item = &'a TriageItem>) -> usize {
        items
            .into_iter()
            .filter(|item| self.for_item(item).is_some())
            .count()
    }
}

impl Default for VerifierRegistry {
    fn default() -> Self {
        Self::standard()
    }
}

/// Confirms a file that was supposed to be removed is actually gone.
///
/// The cheapest verifier there is, and a real one: removing a stale lock file
/// is one of only two changes this engine can make, and "is it gone" is
/// exactly the question the fix claimed to answer.
pub struct FileGoneVerifier;

#[async_trait]
impl Verifier for FileGoneVerifier {
    async fn verify(&self, item: &TriageItem) -> Verdict {
        let Some(path) = stale_file_path(item) else {
            return Verdict::CannotTell {
                reason: "this problem does not name a file to check".to_string(),
            };
        };

        if std::path::Path::new(&path).exists() {
            Verdict::StillBroken {
                detail: format!("{path} is still there"),
            }
        } else {
            Verdict::Fixed
        }
    }
}

impl ItemVerifier for FileGoneVerifier {
    fn handles(&self, item: &TriageItem) -> bool {
        stale_file_path(item).is_some()
    }
}

/// The file a stale-file finding is about, taken from its evidence.
fn stale_file_path(item: &TriageItem) -> Option<String> {
    if !item.finding_id.contains("stale") {
        return None;
    }
    item.finding
        .evidence
        .iter()
        .find(|evidence| {
            let label = evidence.label.to_ascii_lowercase();
            label == "path" || label == "file"
        })
        .map(|evidence| evidence.value.clone())
}
