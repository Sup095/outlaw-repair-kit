//! Services that were meant to be running and are not.
//!
//! The naive version of this check -- "list every automatic service that is
//! not running" -- is an alarm generator, not a diagnostic. On a perfectly
//! healthy Windows machine it reports half a dozen updaters and delayed-start
//! services that are stopped for entirely ordinary reasons. A check that cries
//! wolf on a working computer teaches people to ignore it, which costs more
//! than not having the check at all.
//!
//! So the signal is narrower and means something: a service set to start
//! automatically, which is **not running and terminated with an error**. That
//! is a service that tried and failed, which is a real fault with a real fix.
//!
//! On Linux systemd already draws this distinction itself, and `failed` is
//! exactly the state being asked about.

use anyhow::Result;
use async_trait::async_trait;

use crate::finding::{Category, Finding, Severity, Triage};
use crate::platform::common::run_capture;
use crate::probe::{Probe, ProbeContext, ProbeMeta};
use crate::tier::ScanTier;

/// A service that was supposed to be running and is not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailedService {
    pub name: String,
    /// What the system calls its current state.
    pub state: String,
    /// Why it stopped, where the system says.
    pub detail: Option<String>,
}

/// Build the finding for one failed service.
///
/// The service name goes in the evidence under `service`, which is what the
/// verifier reads to re-test it afterwards. Without that, a fix could be
/// applied but never confirmed.
pub fn finding_for(service: &FailedService) -> Finding {
    let mut builder = Finding::builder("services.failed", "service.stopped")
        .subject(&service.name)
        .severity(Severity::Medium)
        .category(Category::Configuration)
        .title(format!("The `{}` service is not running", service.name))
        .detail(format!(
            "`{}` is set to start automatically but is {} after stopping with an error. \
             Something that expects it will not be working.",
            service.name, service.state
        ))
        .evidence("service", &service.name)
        .evidence("state", &service.state)
        .triage(Triage::Queue);

    if let Some(detail) = &service.detail {
        builder = builder.evidence("reported", detail);
    }
    builder.build()
}

/// Parse the CSV that the Windows query produces.
///
/// One service per line: name, state, exit code.
pub fn parse_windows(output: &str) -> Vec<FailedService> {
    output
        .lines()
        .filter_map(|line| {
            let mut parts = line.trim().splitn(3, ',');
            let name = parts.next()?.trim();
            let state = parts.next()?.trim();
            let code = parts.next().unwrap_or("").trim();
            if name.is_empty() {
                return None;
            }
            Some(FailedService {
                name: name.to_string(),
                state: state.to_ascii_lowercase(),
                detail: (!code.is_empty() && code != "0").then(|| format!("exit code {code}")),
            })
        })
        .collect()
}

/// Parse `systemctl list-units --state=failed --plain --no-legend`.
pub fn parse_systemd(output: &str) -> Vec<FailedService> {
    output
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let unit = fields.next()?;
            // Only services. A failed mount or timer is a different problem
            // with a different answer, and restarting it is not it.
            let name = unit.strip_suffix(".service")?;
            if name.is_empty() {
                return None;
            }
            let rest: Vec<&str> = fields.collect();
            Some(FailedService {
                name: name.to_string(),
                state: "failed".to_string(),
                detail: (!rest.is_empty()).then(|| rest.join(" ")),
            })
        })
        .collect()
}

/// Ask this machine which services failed.
fn failed_services(_platform: &dyn crate::Platform) -> Result<Vec<FailedService>> {
    #[cfg(windows)]
    {
        // Auto-start, not running, and stopped with an error. All three
        // conditions matter: without the exit code this reports ordinary
        // delayed-start services as faults.
        const QUERY: &str = "Get-CimInstance Win32_Service | \
             Where-Object { $_.StartMode -eq 'Auto' -and $_.State -ne 'Running' -and \
             $_.ExitCode -ne 0 } | \
             ForEach-Object { \"$($_.Name),$($_.State),$($_.ExitCode)\" }";
        let output = run_capture(
            "powershell",
            &["-NoProfile", "-NonInteractive", "-Command", QUERY],
        )?;
        Ok(parse_windows(&output.stdout))
    }
    #[cfg(target_os = "linux")]
    {
        let output = run_capture(
            "systemctl",
            &[
                "list-units",
                "--state=failed",
                "--plain",
                "--no-legend",
                "--no-pager",
            ],
        )?;
        Ok(parse_systemd(&output.stdout))
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        Ok(Vec::new())
    }
}

/// Reports services that tried to start and failed.
pub struct FailedServicesProbe;

#[async_trait]
impl Probe for FailedServicesProbe {
    fn meta(&self) -> ProbeMeta {
        ProbeMeta {
            id: "services.failed",
            name: "Failed services",
            description: "Services set to start automatically that stopped with an error. \
                          Services that are simply idle or start on demand are not reported.",
            category: Category::Configuration,
            min_tier: ScanTier::Quick,
            platforms: &[crate::PlatformKind::Windows, crate::PlatformKind::Linux],
            requires_tools: &[],
            emits: &["service.stopped"],
            requires_elevation: false,
        }
    }

    async fn run(&self, ctx: &ProbeContext) -> Result<Vec<Finding>> {
        let services = ctx.blocking(failed_services).await?;
        Ok(services.iter().map(finding_for).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_windows_line_becomes_a_service_with_its_exit_code() {
        let parsed = parse_windows("Spooler,Stopped,1053\ngpsvc,Stopped,0\n");
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].name, "Spooler");
        assert_eq!(parsed[0].state, "stopped");
        assert_eq!(parsed[0].detail.as_deref(), Some("exit code 1053"));
        // A zero exit code carries no explanation to give.
        assert_eq!(parsed[1].detail, None);
    }

    #[test]
    fn blank_and_malformed_lines_are_skipped_rather_than_reported() {
        let parsed = parse_windows("\n   \nSpooler,Stopped,1053\n,Stopped,1\n");
        assert_eq!(parsed.len(), 1, "got {parsed:?}");
        assert_eq!(parsed[0].name, "Spooler");
    }

    #[test]
    fn a_failed_systemd_unit_becomes_a_service() {
        let parsed = parse_systemd(
            "bluetooth.service loaded failed failed Bluetooth service\n\
             systemd-modules-load.service loaded failed failed Load Kernel Modules\n",
        );
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].name, "bluetooth");
        assert_eq!(parsed[0].state, "failed");
        assert!(parsed[0].detail.as_deref().unwrap().contains("Bluetooth"));
    }

    #[test]
    fn failed_units_that_are_not_services_are_left_alone() {
        // Restarting a failed mount or timer is not the answer to it, so this
        // check does not claim them.
        let parsed = parse_systemd(
            "tmp.mount loaded failed failed Temporary Directory\n\
             backup.timer loaded failed failed Daily backup\n\
             cups.service loaded failed failed CUPS\n",
        );
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].name, "cups");
    }

    #[test]
    fn nothing_wrong_produces_nothing() {
        assert!(parse_windows("").is_empty());
        assert!(parse_systemd("").is_empty());
    }

    #[test]
    fn the_finding_names_the_service_where_a_verifier_will_look_for_it() {
        // The verifier reads the service name from this evidence label. If it
        // moved, a fix could be applied and never confirmed.
        let finding = finding_for(&FailedService {
            name: "cups".to_string(),
            state: "failed".to_string(),
            detail: Some("exit code 1".to_string()),
        });
        assert_eq!(finding.id, "service.stopped");
        assert_eq!(finding.triage, Triage::Queue);
        let named = finding
            .evidence
            .iter()
            .find(|item| item.label == "service")
            .expect("the service name must be in the evidence");
        assert_eq!(named.value, "cups");
    }
}
