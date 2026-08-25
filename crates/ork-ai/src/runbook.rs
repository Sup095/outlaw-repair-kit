//! The runbook library: known problems and what has been known to fix them.
//!
//! Runbooks are consulted *before* a model is, and that ordering is the whole
//! point. A known problem with a known fix should not be re-derived by a
//! language model every time it occurs: the runbook answer is faster, costs
//! nothing, produces the same result twice, and does not require a model to be
//! available at all. The model is for the problems nobody has written down yet.
//!
//! Entries are TOML rather than YAML. The obvious choice would have been YAML,
//! but the maintained Rust YAML parsers are in flux and the format is already
//! TOML everywhere else in this tool; a second configuration language, parsed
//! by an unmaintained crate, is a poor trade for slightly prettier multi-line
//! strings.

use std::path::Path;

use anyhow::Context;
use ork_core::Finding;
use serde::{Deserialize, Serialize};

use crate::Result;

/// The library that ships with the tool.
///
/// Embedding it means a fresh install has useful answers immediately, with no
/// download and no first-run setup.
const BUILT_IN: &str = include_str!("../runbooks/built-in.toml");

/// How disruptive a fix is. Candidate fixes are tried least invasive first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Invasiveness {
    /// Changes nothing; gathers more information.
    Inspect,
    /// Reversible and small: clearing a cache, restarting a service.
    Low,
    /// Reversible but disruptive: restarting the machine, reinstalling a package.
    Medium,
    /// Changes system-level state: drivers, kernels, partitions.
    High,
}

/// One thing that might fix the problem.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateFix {
    /// What to do, in plain language.
    pub description: String,
    #[serde(default = "default_invasiveness")]
    pub invasiveness: Invasiveness,
    /// A suggested command.
    ///
    /// Present for the user's benefit and for the fix layer to work from
    /// later. Nothing in this release runs it.
    #[serde(default)]
    pub command: Option<String>,
    /// Platforms this fix applies to. Empty means all of them.
    #[serde(default)]
    pub platforms: Vec<String>,
    /// How the fix layer should carry this out, if it can.
    ///
    /// Without one, a fix is advice: written down for a person to act on. With
    /// one, the engine can attempt it -- but only if something can also test
    /// the result afterwards, and only after the person confirms it.
    #[serde(default)]
    pub action: Option<Recipe>,
}

/// A fix named in the vocabulary the fix layer understands.
///
/// Deliberately just two strings. The set of things that can actually be done
/// to a machine is decided by the code that has to do them safely, not by
/// whoever edits a runbook file -- so an unrecognised `kind` is refused rather
/// than approximated, and there is no "run this command" kind at all.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Recipe {
    /// `restart-service` or `remove-stale-file`.
    pub kind: String,
    /// What to act on: a service name, or a path. `~` and environment
    /// variables in a path are expanded on the machine it runs on.
    pub target: String,
}

fn default_invasiveness() -> Invasiveness {
    Invasiveness::Low
}

/// A known problem and its ranked candidate fixes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunbookEntry {
    pub id: String,
    pub title: String,
    /// Finding identifiers this entry answers.
    #[serde(default)]
    pub finding_ids: Vec<String>,
    /// Optional extra requirement: at least one of these substrings must appear
    /// in the finding's text or evidence. Used to separate several distinct
    /// problems that share one finding identifier.
    #[serde(default)]
    pub keywords: Vec<String>,
    /// What is actually going on, in plain language.
    pub explanation: String,
    #[serde(default)]
    pub fixes: Vec<CandidateFix>,
}

#[derive(Debug, Deserialize)]
struct RunbookFile {
    #[serde(default)]
    entry: Vec<RunbookEntry>,
}

impl RunbookEntry {
    /// Whether this entry answers a given finding.
    pub fn matches(&self, finding: &Finding) -> bool {
        if !self.finding_ids.iter().any(|id| id == &finding.id) {
            return false;
        }
        if self.keywords.is_empty() {
            return true;
        }

        // Search the finding's own words and its evidence, so an entry can key
        // off an exact error string the probe captured.
        let haystack = std::iter::once(finding.title.to_ascii_lowercase())
            .chain(std::iter::once(finding.detail.to_ascii_lowercase()))
            .chain(
                finding
                    .evidence
                    .iter()
                    .map(|item| item.value.to_ascii_lowercase()),
            )
            .collect::<Vec<_>>()
            .join("\n");

        self.keywords
            .iter()
            .any(|keyword| haystack.contains(&keyword.to_ascii_lowercase()))
    }

    /// Fixes that apply to this platform, least invasive first.
    pub fn fixes_for(&self, platform: &str) -> Vec<&CandidateFix> {
        let mut fixes: Vec<&CandidateFix> = self
            .fixes
            .iter()
            .filter(|fix| {
                fix.platforms.is_empty()
                    || fix
                        .platforms
                        .iter()
                        .any(|p| p.eq_ignore_ascii_case(platform))
            })
            .collect();
        // Least invasive first, so the fix layer tries the cheap reversible
        // thing before it touches a driver.
        fixes.sort_by_key(|fix| fix.invasiveness);
        fixes
    }
}

/// A loaded runbook library.
#[derive(Debug, Clone, Default)]
pub struct RunbookLibrary {
    entries: Vec<RunbookEntry>,
}

impl RunbookLibrary {
    /// Load the built-in library, plus any user entries in `user_dir`.
    ///
    /// User entries are loaded after the built-ins and win on conflict, so a
    /// person can correct an entry that is wrong for their machine without
    /// editing the tool.
    pub fn load(user_dir: Option<&Path>) -> Result<Self> {
        let mut library = Self::built_in()?;

        if let Some(dir) = user_dir {
            match std::fs::read_dir(dir) {
                Ok(entries) => {
                    // Sorted, so loading is deterministic rather than dependent
                    // on filesystem ordering.
                    let mut paths: Vec<_> = entries
                        .flatten()
                        .map(|entry| entry.path())
                        .filter(|path| {
                            path.extension()
                                .is_some_and(|ext| ext.eq_ignore_ascii_case("toml"))
                        })
                        .collect();
                    paths.sort();

                    for path in paths {
                        match std::fs::read_to_string(&path)
                            .map_err(anyhow::Error::from)
                            .and_then(|text| Self::parse(&text))
                        {
                            Ok(extra) => library.merge(extra),
                            // One malformed user file must not cost the whole
                            // library, but it does need saying.
                            Err(error) => {
                                tracing::warn!(
                                    path = %path.display(),
                                    %error,
                                    "skipping unreadable runbook file"
                                );
                            }
                        }
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    tracing::debug!(dir = %dir.display(), %error, "could not read runbook directory");
                }
            }
        }

        Ok(library)
    }

    pub fn built_in() -> Result<Self> {
        Self::parse(BUILT_IN).context("the built-in runbook library is malformed")
    }

    pub fn parse(text: &str) -> Result<Self> {
        let file: RunbookFile = toml::from_str(text).context("runbook file is not valid TOML")?;
        Ok(Self {
            entries: file.entry,
        })
    }

    /// Add entries from another library, replacing any with the same id.
    fn merge(&mut self, other: Self) {
        for entry in other.entries {
            match self
                .entries
                .iter_mut()
                .find(|existing| existing.id == entry.id)
            {
                Some(existing) => *existing = entry,
                None => self.entries.push(entry),
            }
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn entries(&self) -> &[RunbookEntry] {
        &self.entries
    }

    /// The entry that answers this finding, if the library has one.
    pub fn lookup(&self, finding: &Finding) -> Option<&RunbookEntry> {
        // A more specific entry -- one that also requires a keyword -- wins
        // over a general one for the same finding.
        self.entries
            .iter()
            .filter(|entry| entry.matches(finding))
            .max_by_key(|entry| entry.keywords.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ork_core::finding::{Finding, Severity};

    fn finding(id: &str, detail: &str) -> Finding {
        Finding::builder("test", id)
            .title("something happened")
            .detail(detail)
            .severity(Severity::High)
            .build()
    }

    #[test]
    fn the_built_in_library_is_valid_and_not_empty() {
        let library = RunbookLibrary::built_in().expect("the shipped library must parse");
        assert!(library.len() >= 15, "got {} entries", library.len());

        for entry in library.entries() {
            assert!(!entry.id.is_empty());
            assert!(!entry.title.is_empty());
            assert!(
                !entry.explanation.trim().is_empty(),
                "{} has no explanation",
                entry.id
            );
            assert!(
                !entry.finding_ids.is_empty(),
                "{} answers no finding",
                entry.id
            );
            assert!(!entry.fixes.is_empty(), "{} suggests nothing", entry.id);
            for fix in &entry.fixes {
                assert!(
                    !fix.description.trim().is_empty(),
                    "{} has an empty fix",
                    entry.id
                );
            }
        }
    }

    #[test]
    fn entry_ids_are_unique() {
        let library = RunbookLibrary::built_in().unwrap();
        let mut ids: Vec<&str> = library.entries().iter().map(|e| e.id.as_str()).collect();
        ids.sort_unstable();
        let count = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), count, "the library contains duplicate entry ids");
    }

    #[test]
    fn every_finding_the_probes_emit_has_an_answer() {
        // If a probe grows a new finding id, this fails until the library
        // learns about it -- which is the point. A finding with no runbook
        // falls through to the model every single time it occurs.
        let library = RunbookLibrary::built_in().unwrap();
        let emitted = [
            "storage.volume-low-on-space",
            "memory.high-pressure",
            "process.zombie-buildup",
            "device.driver-mismatch",
            "device.driver-missing",
            "device.not-working",
            "device.reboot-required",
            "app.launch-failed",
            "app.launch-hung",
            "logs.unexpected-shutdown",
            "logs.bugcheck",
            "logs.hardware-error",
            "logs.display-driver-timeout",
            "logs.kernel-panic",
            "logs.gpu-fault",
            "logs.oom-kill",
            "logs.storage-error",
        ];

        for id in emitted {
            assert!(
                library.lookup(&finding(id, "some detail")).is_some(),
                "no runbook entry answers `{id}`"
            );
        }
    }

    #[test]
    fn findings_that_are_deliberately_left_to_the_model_have_no_entry() {
        // These are genuinely ambiguous -- a large process might be a leak or
        // a virtual machine, and an unrecognised repeated error is
        // unrecognised by definition. Inventing a canned answer for them would
        // be worse than admitting there is not one.
        let library = RunbookLibrary::built_in().unwrap();
        for id in [
            "process.memory-hog",
            "process.sustained-high-cpu",
            "logs.repeated-error",
        ] {
            assert!(
                library.lookup(&finding(id, "detail")).is_none(),
                "{id} unexpectedly matched"
            );
        }
    }

    #[test]
    fn a_more_specific_entry_wins_over_a_general_one() {
        let library = RunbookLibrary::built_in().unwrap();

        let specific = finding(
            "device.not-working",
            "Windows stopped this device because it reported a problem.",
        );
        assert_eq!(library.lookup(&specific).unwrap().id, "device.not-working");

        let general = finding("device.not-working", "The device cannot start.");
        assert_eq!(
            library.lookup(&general).unwrap().id,
            "device.not-working-general"
        );
    }

    #[test]
    fn a_keyword_entry_matches_against_evidence_too() {
        // The exact error string usually lives in captured evidence rather
        // than in the human-readable detail.
        let with_evidence = Finding::builder("test", "app.launch-failed")
            .title("will not start")
            .detail("it failed")
            .evidence(
                "stderr",
                "error while loading shared libraries: libfoo.so.1",
            )
            .build();

        let library = RunbookLibrary::built_in().unwrap();
        assert_eq!(
            library.lookup(&with_evidence).unwrap().id,
            "app.launch-missing-library"
        );
    }

    #[test]
    fn fixes_come_back_least_invasive_first() {
        let library = RunbookLibrary::built_in().unwrap();
        let entry = library
            .entries()
            .iter()
            .find(|e| e.id == "device.driver-mismatch")
            .expect("entry should exist");

        let order: Vec<Invasiveness> = entry
            .fixes_for("linux")
            .iter()
            .map(|fix| fix.invasiveness)
            .collect();
        let mut sorted = order.clone();
        sorted.sort();
        assert_eq!(order, sorted, "fixes must be ordered least invasive first");
    }

    #[test]
    fn platform_specific_fixes_are_filtered_out_elsewhere() {
        let library = RunbookLibrary::built_in().unwrap();
        let entry = library
            .entries()
            .iter()
            .find(|e| e.id == "storage.volume-low-on-space")
            .unwrap();

        let linux_commands: Vec<String> = entry
            .fixes_for("linux")
            .iter()
            .filter_map(|fix| fix.command.clone())
            .collect();
        assert!(linux_commands.iter().any(|c| c.contains("pacman")));
        assert!(
            !linux_commands.iter().any(|c| c.contains("cleanmgr")),
            "a Windows command must not be offered on Linux"
        );
    }

    #[test]
    fn a_user_entry_replaces_a_built_in_one_with_the_same_id() {
        let mut library = RunbookLibrary::built_in().unwrap();
        let before = library.len();

        library.merge(
            RunbookLibrary::parse(
                "[[entry]]
                 id = \"memory.high-pressure\"
                 title = \"My own answer\"
                 finding_ids = [\"memory.high-pressure\"]
                 explanation = \"I know better for this machine.\"
                 [[entry.fixes]]
                 description = \"Do the thing that actually works here.\"
",
            )
            .unwrap(),
        );

        assert_eq!(library.len(), before, "replacing must not add an entry");
        assert_eq!(
            library
                .lookup(&finding("memory.high-pressure", "x"))
                .unwrap()
                .title,
            "My own answer"
        );
    }

    #[test]
    fn a_malformed_library_is_an_error_rather_than_an_empty_one() {
        assert!(RunbookLibrary::parse("not toml {{{").is_err());
    }
}
