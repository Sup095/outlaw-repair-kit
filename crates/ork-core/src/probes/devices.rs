//! Devices and drivers that are not in a healthy state.
//!
//! This is the check for the class of fault where nothing is obviously broken
//! but the machine misbehaves: a driver that no longer matches the kernel it
//! is loaded into, a device the operating system has given up on, an update
//! that has been installed but not yet taken effect. Users experience all
//! three as unexplained instability, and none of them announce themselves.

use async_trait::async_trait;

use crate::Result;
use crate::finding::{Category, Finding, Severity, Triage};
use crate::platform::{DeviceIssue, DeviceIssueKind, PlatformKind};
use crate::probe::{Probe, ProbeContext, ProbeMeta};
use crate::tier::ScanTier;

fn severity_for(kind: DeviceIssueKind) -> Severity {
    match kind {
        // A driver that does not match what it is loaded into is the most
        // dangerous of these, because the machine keeps running and only
        // misbehaves later, which makes the cause very hard to find.
        DeviceIssueKind::DriverMismatch => Severity::High,
        DeviceIssueKind::NotWorking => Severity::Medium,
        DeviceIssueKind::DriverMissing => Severity::Medium,
        // Everything is working; it just is not the newest thing installed.
        DeviceIssueKind::RebootRequired => Severity::Low,
    }
}

fn category_for(issue: &DeviceIssue) -> Category {
    let device = issue.device.to_ascii_lowercase();
    if device.contains("nvidia") || device.contains("radeon") || device.contains("graphics") {
        Category::Gpu
    } else {
        Category::Drivers
    }
}

fn headline(issue: &DeviceIssue) -> String {
    match issue.kind {
        DeviceIssueKind::DriverMismatch => {
            format!(
                "`{}` does not match the system it is running on",
                issue.device
            )
        }
        DeviceIssueKind::NotWorking => format!("`{}` is not working", issue.device),
        DeviceIssueKind::DriverMissing => format!("`{}` has no driver installed", issue.device),
        DeviceIssueKind::RebootRequired => {
            format!("`{}` needs a restart to take effect", issue.device)
        }
    }
}

fn hint_for(kind: DeviceIssueKind) -> &'static str {
    match kind {
        DeviceIssueKind::DriverMismatch => {
            "Restart the machine first -- that resolves most version mismatches. If the \
             mismatch survives a restart, the installed driver genuinely does not match this \
             system and needs reinstalling against the running kernel."
        }
        DeviceIssueKind::NotWorking => {
            "Check whether this device worked before a recent update, and reinstall or roll \
             back its driver."
        }
        DeviceIssueKind::DriverMissing => {
            "Install a driver for this device from the vendor or the distribution's driver \
             manager."
        }
        DeviceIssueKind::RebootRequired => "Restart the machine when convenient.",
    }
}

fn to_finding(issue: &DeviceIssue) -> Finding {
    let mut builder = Finding::builder("devices.health", format!("device.{}", issue.kind.as_str()))
        .subject(&issue.device)
        .severity(severity_for(issue.kind))
        .category(category_for(issue))
        .title(headline(issue))
        .detail(issue.detail.clone())
        .evidence("device", &issue.device)
        .evidence("issue_kind", issue.kind.as_str())
        .remediation_hint(hint_for(issue.kind))
        .triage(Triage::Queue);

    if let Some(version) = &issue.driver_version {
        builder = builder.evidence("driver_version", version);
    }
    if let Some(code) = &issue.code {
        builder = builder.evidence("platform_code", code);
    }
    builder.build()
}

#[derive(Debug, Default)]
pub struct DeviceHealthProbe;

#[async_trait]
impl Probe for DeviceHealthProbe {
    fn meta(&self) -> ProbeMeta {
        ProbeMeta {
            id: "devices.health",
            name: "Device and driver health",
            description: "Finds devices the system cannot start and drivers that no longer match the \
                 running system.",
            category: Category::Drivers,
            min_tier: ScanTier::Quick,
            platforms: &[PlatformKind::Windows, PlatformKind::Linux],
            requires_tools: &[],
            // Built from `DeviceIssueKind`, one id per variant.
            emits: &[
                "device.not-working",
                "device.driver-missing",
                "device.driver-mismatch",
                "device.reboot-required",
            ],
            requires_elevation: false,
        }
    }

    async fn run(&self, ctx: &ProbeContext) -> Result<Vec<Finding>> {
        let issues = ctx.blocking(|platform| platform.device_issues()).await?;
        tracing::debug!(issues = issues.len(), "checked device and driver health");
        Ok(issues.iter().map(to_finding).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn issue(device: &str, kind: DeviceIssueKind) -> DeviceIssue {
        DeviceIssue {
            device: device.to_string(),
            kind,
            detail: "something is wrong".to_string(),
            driver_version: None,
            code: None,
        }
    }

    #[test]
    fn a_driver_mismatch_outranks_a_pending_reboot() {
        // Both mean "the driver and the system disagree", but only one of them
        // is currently causing damage.
        assert!(
            severity_for(DeviceIssueKind::DriverMismatch)
                > severity_for(DeviceIssueKind::RebootRequired)
        );
    }

    #[test]
    fn graphics_devices_are_categorised_as_gpu() {
        assert_eq!(
            category_for(&issue(
                "NVIDIA graphics driver",
                DeviceIssueKind::DriverMismatch
            )),
            Category::Gpu
        );
        assert_eq!(
            category_for(&issue("Realtek Audio", DeviceIssueKind::NotWorking)),
            Category::Drivers
        );
    }

    #[test]
    fn findings_carry_a_stable_id_per_issue_kind() {
        let finding = to_finding(&issue("Some Device", DeviceIssueKind::DriverMissing));
        assert_eq!(finding.id, "device.driver-missing");
        assert_eq!(finding.subject.as_deref(), Some("Some Device"));
        assert_eq!(finding.triage, Triage::Queue);
    }

    #[test]
    fn version_and_platform_code_become_evidence_when_present() {
        let mut with_details = issue("GPU", DeviceIssueKind::NotWorking);
        with_details.driver_version = Some("580.82.07".to_string());
        with_details.code = Some("43".to_string());

        let finding = to_finding(&with_details);
        let labels: Vec<&str> = finding
            .evidence
            .iter()
            .map(|item| item.label.as_str())
            .collect();
        assert!(labels.contains(&"driver_version"));
        assert!(labels.contains(&"platform_code"));
    }
}
