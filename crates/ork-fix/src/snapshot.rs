//! Taking a copy before changing anything.
//!
//! Every change this tool makes is preceded by a snapshot, and the snapshot is
//! what makes "roll back on failure" a real promise rather than an intention.
//!
//! There are two levels, and the distinction is worth being precise about
//! because they are often conflated:
//!
//! * **Targeted backup** -- copies exactly the files a fix is about to touch.
//!   Always available, needs no privileges, no supporting infrastructure, and
//!   no configuration. This is what the tool actually relies on, because the
//!   operations it performs are file-scoped.
//! * **System-level snapshot** -- a Windows restore point, a btrfs snapshot, a
//!   Timeshift snapshot. Far broader, but needs administrator rights, needs to
//!   have been set up in advance, and can quietly be disabled. The tool
//!   *reports* whether one is available so a person can judge their own safety
//!   net, and does not silently depend on one being there.
//!
//! Claiming a rollback capability that turns out not to exist is worse than
//! admitting there isn't one, so the tool never assumes the second kind.

use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::Result;

/// One file copied aside before a change.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackedUpFile {
    pub original: PathBuf,
    pub backup: PathBuf,
    /// Whether the file existed when the snapshot was taken.
    ///
    /// A fix that *creates* a file needs the rollback to delete it again, so
    /// "there was nothing here" is itself a state worth recording.
    pub existed: bool,
}

/// A set of files copied aside, and the means to put them back.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub id: String,
    pub label: String,
    pub directory: PathBuf,
    pub files: Vec<BackedUpFile>,
    #[serde(with = "time::serde::rfc3339")]
    pub taken_at: time::OffsetDateTime,
}

impl Snapshot {
    /// Start a snapshot under `root`.
    ///
    /// `id` must be unique; the caller supplies it so the identifier can match
    /// the attempt it belongs to in the audit log.
    pub fn create(root: &Path, id: &str, label: &str) -> Result<Self> {
        let directory = root.join(id);
        std::fs::create_dir_all(&directory)
            .with_context(|| format!("could not create {}", directory.display()))?;
        Ok(Self {
            id: id.to_string(),
            label: label.to_string(),
            directory,
            files: Vec::new(),
            taken_at: time::OffsetDateTime::now_utc(),
        })
    }

    /// Copy a file aside before it is touched.
    ///
    /// Recording a file that does not exist is not an error -- it records the
    /// absence, so that rolling back removes anything the fix created.
    pub fn capture(&mut self, path: &Path) -> Result<()> {
        let backup = self.directory.join(format!("{}.backup", self.files.len()));

        let existed = path.exists();
        if existed {
            std::fs::copy(path, &backup)
                .with_context(|| format!("could not back up {}", path.display()))?;
            tracing::debug!(
                original = %path.display(),
                backup = %backup.display(),
                "captured file before change"
            );
        }

        self.files.push(BackedUpFile {
            original: path.to_path_buf(),
            backup,
            existed,
        });
        Ok(())
    }

    /// Put everything back the way it was.
    ///
    /// Every file is attempted even if an earlier one fails, because a partial
    /// rollback that stops at the first error leaves the machine in a worse
    /// state than either finishing or not starting.
    pub fn restore(&self) -> Result<()> {
        let mut failures = Vec::new();

        for file in &self.files {
            let outcome = if file.existed {
                std::fs::copy(&file.backup, &file.original)
                    .map(|_| ())
                    .with_context(|| format!("could not restore {}", file.original.display()))
            } else {
                // The file did not exist before, so putting things back means
                // removing whatever is there now.
                match std::fs::remove_file(&file.original) {
                    Ok(()) => Ok(()),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                    Err(error) => Err(error)
                        .with_context(|| format!("could not remove {}", file.original.display())),
                }
            };

            if let Err(error) = outcome {
                tracing::error!(%error, "rollback step failed");
                failures.push(format!("{error:#}"));
            }
        }

        if failures.is_empty() {
            tracing::debug!(id = self.id, files = self.files.len(), "rolled back");
            Ok(())
        } else {
            anyhow::bail!("rollback did not fully succeed: {}", failures.join("; "))
        }
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }
}

/// What system-level rollback, if any, this machine has.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SystemSnapshotSupport {
    /// Whether a broader safety net appears to exist.
    pub available: bool,
    /// What it is, or why there is none.
    pub detail: String,
}

/// Report what system-level rollback exists, without creating one.
///
/// Detection only. Creating a restore point needs administrator rights and can
/// take minutes, so it is something a person asks for, not something that
/// happens because a scan noticed a full disk.
pub fn detect_system_snapshot_support() -> SystemSnapshotSupport {
    #[cfg(target_os = "linux")]
    {
        if std::path::Path::new("/usr/bin/timeshift").exists()
            || std::path::Path::new("/usr/bin/timeshift-launcher").exists()
        {
            return SystemSnapshotSupport {
                available: true,
                detail: "Timeshift is installed. Confirm it is configured and has recent \
                         snapshots before relying on it."
                    .to_string(),
            };
        }
        if std::path::Path::new("/usr/bin/snapper").exists() {
            return SystemSnapshotSupport {
                available: true,
                detail: "snapper is installed, which suggests btrfs snapshots are configured."
                    .to_string(),
            };
        }
        SystemSnapshotSupport {
            available: false,
            detail: "No system snapshot tool found. Installing Timeshift, or configuring \
                     btrfs snapshots, would give you a way back from a bad system change. \
                     This tool still backs up every file it touches."
                .to_string(),
        }
    }
    #[cfg(target_os = "windows")]
    {
        SystemSnapshotSupport {
            available: false,
            detail: "System Restore may be available but cannot be confirmed without \
                     administrator rights, so it is not assumed. Check that System \
                     Protection is switched on for your system drive. This tool still \
                     backs up every file it touches."
                .to_string(),
        }
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        SystemSnapshotSupport {
            available: false,
            detail: "System snapshot support is not implemented for this platform.".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ork-snapshot-{}-{name}-{}",
            std::process::id(),
            line!()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_modified_file_is_restored_to_its_original_contents() {
        let dir = scratch("modify");
        let target = dir.join("config.txt");
        std::fs::write(&target, "original").unwrap();

        let mut snapshot = Snapshot::create(&dir.join("snap"), "s1", "test").unwrap();
        snapshot.capture(&target).unwrap();

        std::fs::write(&target, "changed by a fix").unwrap();
        assert_eq!(
            std::fs::read_to_string(&target).unwrap(),
            "changed by a fix"
        );

        snapshot.restore().unwrap();
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "original");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_deleted_file_comes_back() {
        // This is the case that matters most: removing a stale lock file is
        // the one destructive-shaped thing the tool does.
        let dir = scratch("delete");
        let target = dir.join("steam.lock");
        std::fs::write(&target, "lock contents").unwrap();

        let mut snapshot = Snapshot::create(&dir.join("snap"), "s2", "test").unwrap();
        snapshot.capture(&target).unwrap();
        std::fs::remove_file(&target).unwrap();
        assert!(!target.exists());

        snapshot.restore().unwrap();
        assert!(target.exists());
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "lock contents");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_file_created_by_a_fix_is_removed_on_rollback() {
        // Rolling back means "as it was", and it was not there.
        let dir = scratch("create");
        let target = dir.join("new-file.conf");

        let mut snapshot = Snapshot::create(&dir.join("snap"), "s3", "test").unwrap();
        snapshot.capture(&target).unwrap();
        assert!(!target.exists());

        std::fs::write(&target, "created by a fix").unwrap();
        snapshot.restore().unwrap();
        assert!(
            !target.exists(),
            "a file the fix created should be gone after rollback"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rollback_attempts_every_file_even_if_one_fails() {
        // Stopping at the first failure would leave the machine half-restored,
        // which is worse than either finishing or never starting.
        let dir = scratch("partial");
        let good = dir.join("good.txt");
        std::fs::write(&good, "original").unwrap();

        let mut snapshot = Snapshot::create(&dir.join("snap"), "s4", "test").unwrap();
        snapshot.capture(&good).unwrap();
        snapshot.capture(&dir.join("also-missing.txt")).unwrap();

        std::fs::write(&good, "changed").unwrap();
        // Destroy one backup so restoring it must fail.
        std::fs::remove_file(&snapshot.files[0].backup).unwrap();

        let result = snapshot.restore();
        assert!(
            result.is_err(),
            "a failed restore must be reported, not swallowed"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_empty_snapshot_restores_cleanly() {
        let dir = scratch("empty");
        let snapshot = Snapshot::create(&dir.join("snap"), "s5", "test").unwrap();
        assert!(snapshot.is_empty());
        assert!(snapshot.restore().is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn system_snapshot_support_is_reported_honestly() {
        // Whatever it says, it must say something a person can act on rather
        // than claiming a safety net that might not exist.
        let support = detect_system_snapshot_support();
        assert!(!support.detail.trim().is_empty());
    }
}
