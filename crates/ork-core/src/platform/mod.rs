//! The seam for everything operating-system-specific.
//!
//! Probes never touch OS APIs or shell out directly. They ask a [`Platform`]
//! for what they need, which means a new operating system is a new
//! implementation of this trait rather than edits scattered through every
//! probe. Where two platforms genuinely share an implementation (a
//! cross-platform crate already covers it), both delegate to [`common`] --
//! the seam stays, the duplication does not.

mod common;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "windows")]
mod windows;

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::Result;

/// Which operating system family an implementation targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PlatformKind {
    Windows,
    Linux,
    /// Not implemented. Present so that probes can already declare support and
    /// so the compiler forces us to handle it when the implementation lands.
    MacOs,
}

impl PlatformKind {
    pub fn as_str(self) -> &'static str {
        match self {
            PlatformKind::Windows => "windows",
            PlatformKind::Linux => "linux",
            PlatformKind::MacOs => "macos",
        }
    }
}

impl std::fmt::Display for PlatformKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Identifying information about the machine being scanned.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostInfo {
    pub hostname: String,
    /// Human-readable OS name, e.g. `Windows 10 Pro` or `Manjaro Linux`.
    pub os_name: String,
    pub os_version: String,
    pub kernel_version: String,
    pub arch: String,
    pub cpu_brand: String,
    pub physical_cores: Option<usize>,
    pub logical_cores: usize,
    pub total_memory_bytes: u64,
}

/// What a volume is used for, which changes how strictly we judge it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum VolumeRole {
    /// Holds the running operating system. Running this one out of space
    /// breaks the machine, not just a workload.
    System,
    /// Ordinary fixed storage.
    Data,
    /// Removable media. Usually not our problem if it is full.
    Removable,
}

/// A mounted filesystem.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Volume {
    /// `C:\` on Windows, `/home` on Linux.
    pub mount_point: String,
    /// Backing device, where the platform can tell us.
    pub device: String,
    pub filesystem: String,
    pub total_bytes: u64,
    pub available_bytes: u64,
    pub role: VolumeRole,
    /// Read-only mounts are reported but never flagged for being full.
    pub read_only: bool,
}

impl Volume {
    /// Fraction of the volume still free, in the range 0.0 to 1.0. Returns
    /// `None` for a zero-sized volume, which is a pseudo-filesystem we should
    /// not be judging in the first place.
    pub fn free_fraction(&self) -> Option<f64> {
        if self.total_bytes == 0 {
            return None;
        }
        Some(self.available_bytes as f64 / self.total_bytes as f64)
    }

    pub fn used_bytes(&self) -> u64 {
        self.total_bytes.saturating_sub(self.available_bytes)
    }
}

/// A running process.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
    pub executable: Option<String>,
    pub memory_bytes: u64,
    /// Percent of one CPU, so this can exceed 100 on a multi-core machine.
    pub cpu_percent: f32,
    /// Seconds since the process started.
    pub run_time_secs: u64,
}

/// Everything OS-specific that the diagnostic core needs.
///
/// Methods are blocking and are expected to be called from a blocking context
/// (see [`crate::probe::ProbeContext::blocking`]). Keeping them synchronous
/// keeps the platform implementations honest -- they are wrapping OS calls and
/// external tools, not doing their own concurrency.
pub trait Platform: Send + Sync + 'static {
    fn kind(&self) -> PlatformKind;

    /// Identify the machine.
    fn host(&self) -> Result<HostInfo>;

    /// Every mounted filesystem worth reporting on. Pseudo-filesystems
    /// (`proc`, `sysfs`, and friends) are filtered out by the implementation.
    fn volumes(&self) -> Result<Vec<Volume>>;

    /// Every running process.
    fn processes(&self) -> Result<Vec<ProcessInfo>>;

    /// Whether an external tool is present and runnable.
    ///
    /// Probes call this to decide whether to run or to skip with a visible
    /// reason. A missing `smartctl` should produce "skipped: smartctl not
    /// installed", never a failed scan.
    fn tool_available(&self, tool: &str) -> bool {
        common::tool_on_path(tool)
    }
}

/// Build the platform implementation for the machine we are running on.
///
/// Returns an error rather than panicking on an unsupported OS, so a
/// front-end can report it as a normal failure.
pub fn detect() -> Result<Arc<dyn Platform>> {
    #[cfg(target_os = "windows")]
    {
        Ok(Arc::new(windows::WindowsPlatform::new()))
    }
    #[cfg(target_os = "linux")]
    {
        Ok(Arc::new(linux::LinuxPlatform::new()))
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        anyhow::bail!(
            "the Outlaw Repair Kit does not support this operating system yet \
             (supported: Windows, Linux)"
        )
    }
}
