//! Driver and kernel-module health on Linux.
//!
//! Linux has no equivalent of Device Manager's yellow triangle, so this looks
//! for the specific situations that actually break machines, all of which are
//! variations on "the driver no longer matches what it is loaded into":
//!
//! * The running kernel's module directory has been removed. On rolling
//!   distributions this happens routinely: a package update replaces the
//!   kernel and deletes the old modules, and the still-running kernel can no
//!   longer load *anything* new -- no USB device, no filesystem, and critically
//!   no graphics driver. The machine appears to work until something needs a
//!   module, then behaves inexplicably until it is rebooted.
//! * A newer kernel is installed than the one running, which is the benign
//!   version of the same story and explains a great many "it broke after I
//!   updated" reports.
//! * The NVIDIA kernel module and the userspace driver disagree about their
//!   version, which produces freezes and crashes that look like application
//!   bugs.

use std::path::Path;

use crate::Result;
use crate::platform::{DeviceIssue, DeviceIssueKind, common};

/// Compare two kernel version strings well enough to tell "newer" from "same".
///
/// Kernel version strings are not semver -- `6.6.10-2-MANJARO` is normal --
/// so this compares the leading numeric components and treats anything else as
/// a tiebreak by string order.
fn is_newer(candidate: &str, running: &str) -> bool {
    fn numeric_parts(version: &str) -> Vec<u64> {
        version
            .split(|c: char| !c.is_ascii_digit())
            .filter(|part| !part.is_empty())
            .filter_map(|part| part.parse().ok())
            .take(4)
            .collect()
    }

    let (a, b) = (numeric_parts(candidate), numeric_parts(running));
    match a.cmp(&b) {
        std::cmp::Ordering::Greater => true,
        std::cmp::Ordering::Less => false,
        std::cmp::Ordering::Equal => candidate > running,
    }
}

/// Check the running kernel against what is installed under `/lib/modules`.
fn check_kernel_modules(running: &str, installed: &[String]) -> Vec<DeviceIssue> {
    let mut issues = Vec::new();

    if !installed.iter().any(|version| version == running) {
        issues.push(DeviceIssue {
            device: format!("Linux kernel {running}"),
            kind: DeviceIssueKind::DriverMismatch,
            detail: format!(
                "The running kernel is {running}, but its modules are no longer installed. \
                 This happens when a system update replaces the kernel while the old one is \
                 still running. Until this machine is restarted it cannot load any kernel \
                 module it has not already loaded -- including graphics, storage, and USB \
                 drivers -- which shows up as devices that stop working, applications that \
                 fail to start, and freezes that have no obvious cause."
            ),
            driver_version: Some(running.to_string()),
            code: None,
        });
        return issues;
    }

    if let Some(newest) = installed
        .iter()
        .filter(|version| is_newer(version, running))
        .max()
    {
        issues.push(DeviceIssue {
            device: format!("Linux kernel {newest}"),
            kind: DeviceIssueKind::RebootRequired,
            detail: format!(
                "Kernel {newest} is installed but this machine is still running {running}. \
                 The new kernel and its drivers take effect after a restart. Until then, \
                 newly installed drivers may not match the kernel actually in use."
            ),
            driver_version: Some(newest.clone()),
            code: None,
        });
    }

    issues
}

/// The NVIDIA kernel module's version, as the running kernel reports it.
///
/// `/proc/driver/nvidia/version` exists only when the module is loaded, which
/// makes its absence meaningful too: an NVIDIA card with no loaded module is
/// running on the open-source fallback driver, whatever the user installed.
fn nvidia_module_version(text: &str) -> Option<String> {
    // The line looks like:
    //   NVRM version: NVIDIA UNIX x86_64 Kernel Module  580.82.07  Tue ...
    let line = text.lines().find(|line| line.contains("Kernel Module"))?;
    line.split_whitespace()
        .find(|token| {
            token.contains('.') && token.chars().next().is_some_and(|c| c.is_ascii_digit())
        })
        .map(str::to_string)
}

/// Compare the loaded NVIDIA kernel module against the userspace driver.
fn check_nvidia(module_version: Option<&str>, userspace_version: Option<&str>) -> Vec<DeviceIssue> {
    let (Some(module), Some(userspace)) = (module_version, userspace_version) else {
        return Vec::new();
    };
    if module == userspace {
        return Vec::new();
    }

    vec![DeviceIssue {
        device: "NVIDIA graphics driver".to_string(),
        kind: DeviceIssueKind::DriverMismatch,
        detail: format!(
            "The loaded NVIDIA kernel module is version {module}, but the userspace driver \
             is version {userspace}. These two halves of the driver have to match. When they \
             do not, the usual result is that graphics acceleration silently fails, or the \
             machine freezes or crashes under load in a way that looks like the application's \
             fault. This normally means the driver was updated but the machine has not been \
             restarted since."
        ),
        driver_version: Some(format!("module {module}, userspace {userspace}")),
        code: None,
    }]
}

/// Everything not in a healthy state.
pub fn device_issues() -> Result<Vec<DeviceIssue>> {
    let mut issues = Vec::new();

    let running = common::run_capture("uname", &["-r"])?;
    let running = running.stdout.trim().to_string();
    if running.is_empty() {
        anyhow::bail!("could not determine the running kernel version");
    }

    // Every kernel with modules installed shows up as a directory here.
    let mut installed = Vec::new();
    if let Ok(entries) = std::fs::read_dir("/lib/modules") {
        for entry in entries.flatten() {
            if entry.path().is_dir()
                && let Some(name) = entry.file_name().to_str()
            {
                installed.push(name.to_string());
            }
        }
    }

    if installed.is_empty() {
        // Reading nothing is not the same as finding nothing wrong, and
        // pretending otherwise would report a clean bill of health for a
        // machine we failed to inspect.
        tracing::debug!("/lib/modules was empty or unreadable; skipping kernel module checks");
    } else {
        issues.extend(check_kernel_modules(&running, &installed));
    }

    let module_version = std::fs::read_to_string("/proc/driver/nvidia/version")
        .ok()
        .as_deref()
        .and_then(nvidia_module_version);
    let userspace_version = if common::which("nvidia-smi").is_some() {
        common::run_capture(
            "nvidia-smi",
            &["--query-gpu=driver_version", "--format=csv,noheader"],
        )
        .ok()
        .filter(|output| output.success)
        .and_then(|output| {
            output
                .stdout
                .lines()
                .next()
                .map(|line| line.trim().to_string())
        })
        .filter(|version| !version.is_empty())
    } else {
        None
    };
    issues.extend(check_nvidia(
        module_version.as_deref(),
        userspace_version.as_deref(),
    ));

    Ok(issues)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_matching_kernel_with_nothing_newer_is_healthy() {
        let installed = vec!["6.6.10-2-MANJARO".to_string()];
        assert!(check_kernel_modules("6.6.10-2-MANJARO", &installed).is_empty());
    }

    #[test]
    fn a_removed_module_directory_is_the_serious_case() {
        // The running kernel's modules were deleted by an update. This is the
        // one that makes a machine behave inexplicably until it is rebooted.
        let installed = vec!["6.6.16-1-MANJARO".to_string()];
        let issues = check_kernel_modules("6.6.10-2-MANJARO", &installed);
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].kind, DeviceIssueKind::DriverMismatch);
        assert!(issues[0].detail.contains("restarted"));
    }

    #[test]
    fn a_newer_installed_kernel_is_only_a_pending_reboot() {
        let installed = vec![
            "6.6.10-2-MANJARO".to_string(),
            "6.6.16-1-MANJARO".to_string(),
        ];
        let issues = check_kernel_modules("6.6.10-2-MANJARO", &installed);
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].kind, DeviceIssueKind::RebootRequired);
    }

    #[test]
    fn an_older_installed_kernel_left_behind_is_not_a_problem() {
        // Keeping the previous kernel around is normal and deliberate.
        let installed = vec![
            "6.6.10-2-MANJARO".to_string(),
            "6.5.9-1-MANJARO".to_string(),
        ];
        assert!(check_kernel_modules("6.6.10-2-MANJARO", &installed).is_empty());
    }

    #[test]
    fn kernel_versions_compare_numerically_not_alphabetically() {
        // String comparison would put 6.6.9 above 6.6.10, which is exactly
        // backwards and would hide a pending reboot.
        assert!(is_newer("6.6.10-1-MANJARO", "6.6.9-1-MANJARO"));
        assert!(!is_newer("6.6.9-1-MANJARO", "6.6.10-1-MANJARO"));
        assert!(is_newer("6.7.0-1", "6.6.99-1"));
        assert!(!is_newer("6.6.10-1", "6.6.10-1"));
    }

    #[test]
    fn the_nvidia_module_version_is_extracted_from_the_proc_file() {
        let text = "NVRM version: NVIDIA UNIX x86_64 Kernel Module  580.82.07  Tue Aug 5\n\
                    GCC version:  gcc version 14.2.1\n";
        assert_eq!(nvidia_module_version(text).as_deref(), Some("580.82.07"));
    }

    #[test]
    fn a_missing_proc_file_yields_no_version() {
        assert_eq!(nvidia_module_version(""), None);
        assert_eq!(nvidia_module_version("something unrelated"), None);
    }

    #[test]
    fn matching_nvidia_halves_are_healthy() {
        assert!(check_nvidia(Some("580.82.07"), Some("580.82.07")).is_empty());
    }

    #[test]
    fn mismatched_nvidia_halves_are_reported() {
        let issues = check_nvidia(Some("580.82.07"), Some("575.64.03"));
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].kind, DeviceIssueKind::DriverMismatch);
    }

    #[test]
    fn a_machine_without_nvidia_reports_nothing() {
        assert!(check_nvidia(None, None).is_empty());
        assert!(check_nvidia(Some("580.82.07"), None).is_empty());
        assert!(check_nvidia(None, Some("580.82.07")).is_empty());
    }
}
