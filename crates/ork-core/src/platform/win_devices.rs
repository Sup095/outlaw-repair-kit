//! Devices Windows itself considers broken.
//!
//! Windows already knows which devices are not working -- it is what puts the
//! yellow warning triangle in Device Manager. This reads that same state
//! through CIM, so the tool agrees with what the user would see if they looked
//! themselves, rather than inventing a second opinion.

use anyhow::Context;
use serde::Deserialize;

use crate::Result;
use crate::platform::{DeviceIssue, DeviceIssueKind, common};

#[derive(Debug, Deserialize)]
struct RawDevice {
    name: Option<String>,
    code: Option<i64>,
    id: Option<String>,
}

/// Meaning of a Windows Configuration Manager error code.
///
/// Only the codes worth acting on are translated. Anything else is reported
/// with its number, which is still enough to search for, rather than being
/// dressed up in a guess.
fn explain(code: i64) -> (DeviceIssueKind, &'static str) {
    match code {
        1 => (
            DeviceIssueKind::NotWorking,
            "The device is not configured correctly.",
        ),
        3 => (
            DeviceIssueKind::NotWorking,
            "The driver may be corrupted, or the system is low on memory or other resources.",
        ),
        10 => (DeviceIssueKind::NotWorking, "The device cannot start."),
        12 => (
            DeviceIssueKind::NotWorking,
            "The device cannot find enough free resources to use. Another device may be \
             claiming what it needs.",
        ),
        14 => (
            DeviceIssueKind::RebootRequired,
            "The device will not work properly until the computer is restarted.",
        ),
        18 => (
            DeviceIssueKind::DriverMismatch,
            "The drivers for this device need to be reinstalled.",
        ),
        19 => (
            DeviceIssueKind::DriverMismatch,
            "The registry entry for this device is incomplete or damaged.",
        ),
        28 => (
            DeviceIssueKind::DriverMissing,
            "The drivers for this device are not installed.",
        ),
        31 => (
            DeviceIssueKind::NotWorking,
            "Windows cannot load the drivers required for this device.",
        ),
        37..=40 => (
            DeviceIssueKind::DriverMismatch,
            "Windows cannot load the device driver -- the driver may be damaged or missing.",
        ),
        43 => (
            DeviceIssueKind::NotWorking,
            "Windows stopped this device because it reported a problem. This one frequently \
             means failing hardware rather than a software fault.",
        ),
        _ => (
            DeviceIssueKind::NotWorking,
            "Windows reported a problem with this device.",
        ),
    }
}

/// Error codes that describe a user's choice rather than a fault.
///
/// Code 22 is "you disabled this", and 45 is "this was unplugged". Reporting
/// either as a problem would be the tool arguing with a decision the user
/// already made.
fn is_deliberate(code: i64) -> bool {
    matches!(code, 22 | 45)
}

fn parse_devices(json: &str) -> Result<Vec<DeviceIssue>> {
    let json = json.trim();
    if json.is_empty() {
        return Ok(Vec::new());
    }

    let raw: Vec<RawDevice> = match serde_json::from_str::<Vec<RawDevice>>(json) {
        Ok(devices) => devices,
        Err(_) => vec![
            serde_json::from_str::<RawDevice>(json)
                .context("could not parse device query output as JSON")?,
        ],
    };

    Ok(raw
        .into_iter()
        .filter_map(|device| {
            let code = device.code?;
            if code == 0 || is_deliberate(code) {
                return None;
            }
            let (kind, explanation) = explain(code);
            // Fall back to the hardware ID when a device is broken enough
            // that Windows never got a friendly name for it.
            let name = device
                .name
                .filter(|name| !name.trim().is_empty())
                .or(device.id)
                .unwrap_or_else(|| "an unnamed device".to_string());
            Some(DeviceIssue {
                device: name,
                kind,
                detail: explanation.to_string(),
                driver_version: None,
                code: Some(code.to_string()),
            })
        })
        .collect())
}

/// Every device Windows currently considers to be in a fault state.
pub fn device_issues() -> Result<Vec<DeviceIssue>> {
    let script = "$ErrorActionPreference='SilentlyContinue'; \
         Get-CimInstance -ClassName Win32_PnPEntity | \
         Where-Object { $_.ConfigManagerErrorCode -ne 0 } | \
         ForEach-Object { [pscustomobject]@{ name=$_.Name; code=$_.ConfigManagerErrorCode; \
         id=$_.DeviceID } } | ConvertTo-Json -Compress -Depth 2";

    let output = common::run_capture(
        "powershell",
        &["-NoProfile", "-NonInteractive", "-Command", script],
    )?;

    if !output.success && output.stdout.trim().is_empty() {
        tracing::debug!(stderr = %output.stderr.trim(), "device query returned nothing");
        return Ok(Vec::new());
    }

    parse_devices(&output.stdout)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_healthy_machine_reports_nothing() {
        assert!(parse_devices("").unwrap().is_empty());
    }

    #[test]
    fn a_missing_driver_is_recognised() {
        let json = r#"{"name":"PCI Device","code":28,"id":"PCI\\VEN_10DE"}"#;
        let issues = parse_devices(json).unwrap();
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].kind, DeviceIssueKind::DriverMissing);
        assert_eq!(issues[0].code.as_deref(), Some("28"));
    }

    #[test]
    fn a_device_the_user_disabled_is_not_a_fault() {
        // Code 22 is "you turned this off". Reporting it would be the tool
        // arguing with a decision the user already made.
        let json = r#"[{"name":"Some NIC","code":22,"id":"X"},
                       {"name":"Unplugged thing","code":45,"id":"Y"}]"#;
        assert!(parse_devices(json).unwrap().is_empty());
    }

    #[test]
    fn an_unnamed_device_falls_back_to_its_hardware_id() {
        let json = r#"{"name":null,"code":43,"id":"USB\\VID_1234"}"#;
        let issues = parse_devices(json).unwrap();
        assert_eq!(issues[0].device, "USB\\VID_1234");
    }

    #[test]
    fn an_unknown_code_is_still_reported_with_its_number() {
        let json = r#"{"name":"Mystery","code":99,"id":"Z"}"#;
        let issues = parse_devices(json).unwrap();
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].code.as_deref(), Some("99"));
    }
}
