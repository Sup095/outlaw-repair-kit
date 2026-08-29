//! The seam for everything operating-system-specific.
//!
//! Probes never touch OS APIs or shell out directly. They ask a [`Platform`]
//! for what they need, which means a new operating system is a new
//! implementation of this trait rather than edits scattered through every
//! probe. Where two platforms genuinely share an implementation (a
//! cross-platform crate already covers it), both delegate to [`common`] --
//! the seam stays, the duplication does not.

pub(crate) mod common;
pub mod disks;
pub mod integrity;
mod services;
pub mod startup;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
mod linux_devices;
#[cfg(target_os = "linux")]
mod linux_journal;
#[cfg(target_os = "windows")]
mod win_devices;
#[cfg(target_os = "windows")]
mod win_eventlog;
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

/// What state a process is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProcessState {
    Running,
    Sleeping,
    /// Finished, but its parent has not collected the exit status. A handful
    /// is normal and transient; a pile of them is a bug in the parent.
    Zombie,
    Stopped,
    /// Some other or platform-specific state.
    Other,
}

/// A running process.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessInfo {
    pub pid: u32,
    pub parent_pid: Option<u32>,
    pub name: String,
    pub executable: Option<String>,
    pub memory_bytes: u64,
    /// Percent of one CPU, so this can exceed 100 on a multi-core machine.
    pub cpu_percent: f32,
    /// Seconds since the process started.
    pub run_time_secs: u64,
    pub state: ProcessState,
    /// Whether this process belongs to the account the tool is running as.
    ///
    /// `None` means it could not be established, which is not the same as
    /// `false` and must not be rounded to it. On Windows an unprivileged
    /// process cannot read the owner of a service running as SYSTEM, so
    /// "could not tell" is the ordinary answer for exactly the processes it
    /// matters most not to touch.
    pub runs_as_you: Option<bool>,
}

/// Memory pressure at a moment in time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryInfo {
    pub total_bytes: u64,
    /// Memory genuinely available to a new allocation. On Linux this is
    /// `MemAvailable`, not `MemFree` -- reclaimable cache is available, and
    /// treating it as used is how people talk themselves into believing a
    /// healthy Linux box is out of memory.
    pub available_bytes: u64,
    pub swap_total_bytes: u64,
    pub swap_used_bytes: u64,
}

impl MemoryInfo {
    pub fn used_bytes(&self) -> u64 {
        self.total_bytes.saturating_sub(self.available_bytes)
    }

    /// Fraction of memory in use, 0.0 to 1.0.
    pub fn used_fraction(&self) -> Option<f64> {
        if self.total_bytes == 0 {
            return None;
        }
        Some(self.used_bytes() as f64 / self.total_bytes as f64)
    }

    /// Fraction of swap in use, or `None` when there is no swap or page file.
    pub fn swap_used_fraction(&self) -> Option<f64> {
        if self.swap_total_bytes == 0 {
            return None;
        }
        Some(self.swap_used_bytes as f64 / self.swap_total_bytes as f64)
    }
}

/// How serious the operating system considered a log entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Warning,
    Error,
    Critical,
}

/// One entry from the system log.
///
/// Deliberately flat and source-agnostic: the Windows Event Log and journald
/// disagree about almost everything, and the correlation logic should not have
/// to care which one it is reading.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogRecord {
    #[serde(with = "time::serde::rfc3339::option")]
    pub timestamp: Option<time::OffsetDateTime>,
    /// The provider, unit, or kernel subsystem that emitted this.
    pub source: String,
    pub level: LogLevel,
    /// Windows event ID, or the syslog identifier on Linux. This is what makes
    /// repeated occurrences of the same fault groupable.
    pub event_id: Option<String>,
    pub message: String,
}

impl LogRecord {
    /// Key used to group repeats of the same fault together.
    pub fn signature(&self) -> String {
        match &self.event_id {
            Some(id) => format!("{}/{id}", self.source),
            None => self.source.clone(),
        }
    }
}

/// The kind of trouble a device or its driver is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DeviceIssueKind {
    /// The device is present but the operating system cannot start it.
    NotWorking,
    /// No driver is installed for a device that needs one.
    DriverMissing,
    /// A driver is installed but does not match the running kernel or system.
    /// This is the class of fault behind "it worked until I updated".
    DriverMismatch,
    /// The system needs a restart before the installed driver can take effect.
    RebootRequired,
}

impl DeviceIssueKind {
    pub fn as_str(self) -> &'static str {
        match self {
            DeviceIssueKind::NotWorking => "not-working",
            DeviceIssueKind::DriverMissing => "driver-missing",
            DeviceIssueKind::DriverMismatch => "driver-mismatch",
            DeviceIssueKind::RebootRequired => "reboot-required",
        }
    }
}

/// A device or driver that is not in a healthy state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceIssue {
    /// What the device calls itself.
    pub device: String,
    pub kind: DeviceIssueKind,
    /// Plain-language description of what specifically is wrong, from the
    /// platform that knows.
    pub detail: String,
    pub driver_version: Option<String>,
    /// Platform-specific code, where one exists -- a Windows Configuration
    /// Manager error code, for instance.
    pub code: Option<String>,
}

/// A graphics processor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuInfo {
    pub name: String,
    /// Total video memory. `None` when the vendor's tools are not installed
    /// and the amount cannot be determined honestly.
    pub vram_total_bytes: Option<u64>,
    pub vram_used_bytes: Option<u64>,
    pub driver_version: Option<String>,
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

    /// Current memory and swap pressure.
    fn memory(&self) -> Result<MemoryInfo>;

    /// Warning-and-worse entries from the system log, going back `since`.
    ///
    /// This is the check that catches driver, kernel, and hardware faults that
    /// the user experiences as "it randomly freezes" -- the correlation is in
    /// the log long before anyone thinks to look there.
    fn recent_log_errors(&self, since: std::time::Duration) -> Result<Vec<LogRecord>>;

    /// Devices and drivers that are not in a healthy state.
    ///
    /// The interesting case is not a missing driver -- it is a driver that is
    /// installed but no longer matches what it is loaded into, which is what a
    /// kernel or distribution update produces and what users experience as the
    /// machine becoming unstable for no visible reason.
    fn device_issues(&self) -> Result<Vec<DeviceIssue>>;

    /// Graphics processors and how much memory they have.
    ///
    /// Used to size a local model to the hardware, and later to target GPU
    /// stress tests. Returns an empty list rather than failing when no vendor
    /// tooling is installed -- not knowing is a normal state.
    fn gpus(&self) -> Result<Vec<GpuInfo>> {
        Ok(common::detect_gpus())
    }

    /// Whether an external tool is present and runnable.
    ///
    /// Probes call this to decide whether to run or to skip with a visible
    /// reason. A missing `smartctl` should produce "skipped: smartctl not
    /// installed", never a failed scan.
    fn tool_available(&self, tool: &str) -> bool {
        self.locate_tool(tool).is_some()
    }

    /// Whether a system service is running.
    ///
    /// Asked after a service is restarted, to find out whether the restart
    /// actually took. A service reporting itself running is the only evidence
    /// that counts: an exit code of zero from the restart command says the
    /// command ran, not that the service came up and stayed up.
    fn service_status(&self, name: &str) -> ServiceStatus {
        services::status(name)
    }

    /// Where an executable lives, if it is installed.
    ///
    /// Probes that need to *run* something go through this rather than
    /// searching the filesystem themselves, so path resolution stays one
    /// platform-aware implementation instead of one per probe.
    fn locate_tool(&self, tool: &str) -> Option<std::path::PathBuf> {
        common::which(tool)
    }
}

/// What a system service is doing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum ServiceStatus {
    Running,
    Stopped,
    /// No service by that name exists here.
    NotFound,
    /// Could not be determined. Never treated as either good or bad news.
    Unknown {
        detail: String,
    },
}

impl ServiceStatus {
    pub fn is_running(&self) -> bool {
        matches!(self, ServiceStatus::Running)
    }

    pub fn describe(&self) -> String {
        match self {
            ServiceStatus::Running => "running".to_string(),
            ServiceStatus::Stopped => "stopped".to_string(),
            ServiceStatus::NotFound => "no service by that name".to_string(),
            ServiceStatus::Unknown { detail } => format!("could not tell: {detail}"),
        }
    }
}

/// Build the platform implementation for the machine we are running on.
///
/// Returns an error rather than panicking on an unsupported OS, so a
/// front-end can report it as a normal failure.
// `run_capture` and `which` are exported because the installer needs them:
// it is a separate program in this workspace that has to ask the machine the
// same questions this crate already knows how to ask, and a second copy of
// "run a command and read what it said" is a second set of quoting mistakes.
pub use common::{CommandOutput, is_elevated, open_url, run_capture, which};

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
