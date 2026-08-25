//! Is a drive about to fail?
//!
//! The single most consequential thing this tool can find. Everything else it
//! reports costs an afternoon; this one costs everything on the disk, and the
//! only useful version of the warning arrives before the failure rather than
//! after.
//!
//! Two probes, because the two platforms need different permissions to answer:
//!
//! * On **Windows**, `Get-PhysicalDisk` publishes each drive's own health
//!   status and needs no elevation, so the check simply runs.
//! * On **Linux**, the same answer comes from the drive over SMART, which
//!   needs `smartmontools` installed and root to talk to the hardware. Rather
//!   than half-answering, that probe declares both requirements and is skipped
//!   *with a visible reason* when they are not met -- so a scan that could not
//!   check your disks says so, instead of looking like a clean bill of health.
//!
//! What is reported is the drive's own verdict, never an interpretation of raw
//! attributes -- see [`crate::platform::disks`] for why that distinction is
//! the whole design.

use async_trait::async_trait;

use crate::Result;
use crate::finding::{Category, Finding, Severity, Triage};
use crate::platform::PlatformKind;
use crate::platform::disks::{DriveHealth, DriveVerdict};
use crate::probe::{Probe, ProbeContext, ProbeMeta};
use crate::tier::ScanTier;

const PROBE_ID: &str = "storage.health";

/// Turn one drive's verdict into a finding, if there is one to make.
///
/// A healthy drive produces nothing. So does one that would not answer: not
/// knowing is not a fault, and reporting every USB enclosure that declines to
/// talk about itself would bury the one drive that is genuinely dying.
pub fn finding_for(drive: &DriveHealth) -> Option<Finding> {
    let (id, severity, title, detail, hint) = match &drive.verdict {
        DriveVerdict::Healthy | DriveVerdict::Unknown { .. } => return None,
        DriveVerdict::Failing { detail } => (
            "storage.drive-failing",
            Severity::Critical,
            format!("{} is failing", drive.describe()),
            format!(
                "{}. This is the drive's own assessment of itself, not a guess made \
                 from the outside -- the firmware has decided it is no longer reliable. \
                 Treat everything on it as at risk from now, not from when it stops \
                 working.",
                capitalise(detail)
            ),
            "Copy anything you care about off this drive now, before doing anything else. \
             Replace it. Do not run a repair tool on it first -- that costs time the drive \
             may not have, and reads it harder than a backup does.",
        ),
        DriveVerdict::Warning { detail } => (
            "storage.drive-warning",
            Severity::High,
            format!("{} is reporting a problem", drive.describe()),
            format!(
                "{}. It has not declared itself failing, but something is wrong enough \
                 for it to say so.",
                capitalise(detail)
            ),
            "Make sure you have a backup of this drive, then look at the details below. \
             A drive that reports a problem and then recovers is common; one that reports \
             a problem and gets worse is not.",
        ),
    };

    let mut builder = Finding::builder(PROBE_ID, id)
        .subject(&drive.name)
        .severity(severity)
        .category(Category::Storage)
        .title(title)
        .detail(detail)
        .remediation_hint(hint)
        // Never fixed inline. There is no automatic action here at all: the
        // answer to a failing disk is a person with a backup plan.
        .triage(Triage::Queue);

    if !drive.model.trim().is_empty() {
        builder = builder.evidence("model", &drive.model);
    }
    if !drive.kind.is_empty() {
        builder = builder.evidence("kind", &drive.kind);
    }
    // Everything below is evidence beside the verdict, never the basis for
    // one. Present only when the drive actually reported it: an absent value
    // means "not reported", and printing it as zero would be inventing data.
    if let Some(hours) = drive.power_on_hours {
        builder = builder.evidence("powered on for", format!("{hours} hours"));
    }
    if let Some(celsius) = drive.temperature_c {
        builder = builder.evidence("temperature", format!("{celsius} C"));
    }
    if let Some(sectors) = drive.reallocated_sectors {
        builder = builder.evidence("reallocated sectors", sectors.to_string());
    }
    if let Some(errors) = drive.media_errors {
        builder = builder.evidence("media errors", errors.to_string());
    }
    if let Some(wear) = drive.wear_percent {
        builder = builder.evidence("write endurance used", format!("{wear}%"));
    }

    Some(builder.build())
}

fn capitalise(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

async fn check(ctx: &ProbeContext) -> Result<Vec<Finding>> {
    let drives = ctx
        .blocking(|_platform: &dyn crate::Platform| crate::platform::disks::health())
        .await?;

    for drive in &drives {
        tracing::debug!(
            drive = drive.name,
            healthy = drive.verdict.is_healthy(),
            "checked disk health"
        );
    }

    Ok(drives.iter().filter_map(finding_for).collect())
}

/// The Windows half: no elevation needed, so it simply runs.
#[derive(Debug, Default)]
pub struct DiskHealthProbe;

#[async_trait]
impl Probe for DiskHealthProbe {
    fn meta(&self) -> ProbeMeta {
        ProbeMeta {
            id: PROBE_ID,
            name: "Disk health",
            description: "Asks each drive whether it considers itself healthy.",
            category: Category::Storage,
            // Reading a drive's published health status costs nothing, but it
            // is not the kind of answer a quick look-around should be giving:
            // a scan that reports "your disk is dying" wants the user's
            // attention, and a quick scan is the one people run in passing.
            min_tier: ScanTier::Full,
            platforms: &[PlatformKind::Windows],
            requires_tools: &[],
            requires_elevation: false,
        }
    }

    async fn run(&self, ctx: &ProbeContext) -> Result<Vec<Finding>> {
        check(ctx).await
    }
}

/// The Linux half: the same question, asked of the hardware directly.
///
/// Declares what it needs rather than working around it, so a scan without
/// `smartmontools` or without root reports "could not check your disks"
/// instead of quietly returning nothing and reading as a clean result.
#[derive(Debug, Default)]
pub struct SmartHealthProbe;

#[async_trait]
impl Probe for SmartHealthProbe {
    fn meta(&self) -> ProbeMeta {
        ProbeMeta {
            id: "storage.health-smart",
            name: "Disk health (SMART)",
            description: "Asks each drive directly whether it considers itself healthy.",
            category: Category::Storage,
            min_tier: ScanTier::Full,
            platforms: &[PlatformKind::Linux],
            requires_tools: &["smartctl"],
            // Talking to a drive means talking to the device node, and that
            // is root's business on every distribution.
            requires_elevation: true,
        }
    }

    async fn run(&self, ctx: &ProbeContext) -> Result<Vec<Finding>> {
        check(ctx).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drive(verdict: DriveVerdict) -> DriveHealth {
        DriveHealth {
            name: "/dev/sda".to_string(),
            model: "Samsung SSD 860".to_string(),
            kind: "ssd".to_string(),
            verdict,
            ..Default::default()
        }
    }

    #[test]
    fn a_healthy_drive_produces_nothing() {
        assert!(finding_for(&drive(DriveVerdict::Healthy)).is_none());
    }

    #[test]
    fn a_drive_that_would_not_answer_produces_nothing() {
        // Reporting every USB enclosure that declines to talk about itself
        // would bury the one drive that is genuinely dying.
        assert!(
            finding_for(&drive(DriveVerdict::Unknown {
                detail: "no self-assessment".to_string()
            }))
            .is_none()
        );
    }

    #[test]
    fn a_failing_drive_is_critical_and_says_to_copy_things_off_first() {
        let finding = finding_for(&drive(DriveVerdict::Failing {
            detail: "the drive failed its own self-assessment".to_string(),
        }))
        .expect("a failing drive is a finding");

        assert_eq!(finding.id, "storage.drive-failing");
        assert_eq!(finding.severity, Severity::Critical);
        assert_eq!(finding.triage, Triage::Queue);
        let hint = finding.remediation_hint.unwrap_or_default();
        assert!(
            hint.to_lowercase().contains("copy"),
            "the first instruction must be to get the data off: {hint}"
        );
    }

    #[test]
    fn a_warning_is_high_rather_than_critical() {
        // A drive that reports a problem and then recovers is common. Calling
        // that critical would spend the word on the wrong thing, and leave
        // nothing louder for an actual failure.
        let finding = finding_for(&drive(DriveVerdict::Warning {
            detail: "the drive is reporting a problem".to_string(),
        }))
        .expect("a warning is a finding");
        assert_eq!(finding.id, "storage.drive-warning");
        assert_eq!(finding.severity, Severity::High);
    }

    #[test]
    fn measurements_are_carried_as_evidence_only_when_reported() {
        // An absent measurement means "not reported". Printing it as zero
        // would be inventing data, and zero media errors is a meaningfully
        // different statement from not knowing.
        let bare = finding_for(&drive(DriveVerdict::Failing {
            detail: "failed".to_string(),
        }))
        .unwrap();
        assert!(
            !bare
                .evidence
                .iter()
                .any(|item| item.label == "media errors" || item.label == "temperature"),
            "unreported measurements must not appear: {:?}",
            bare.evidence
        );

        let measured = finding_for(&DriveHealth {
            temperature_c: Some(58),
            media_errors: Some(0),
            wear_percent: Some(94),
            reallocated_sectors: Some(120),
            power_on_hours: Some(41000),
            ..drive(DriveVerdict::Failing {
                detail: "failed".to_string(),
            })
        })
        .unwrap();

        let labels: Vec<&str> = measured
            .evidence
            .iter()
            .map(|item| item.label.as_str())
            .collect();
        for expected in [
            "temperature",
            "media errors",
            "write endurance used",
            "reallocated sectors",
            "powered on for",
        ] {
            assert!(
                labels.contains(&expected),
                "{expected} is missing: {labels:?}"
            );
        }
    }

    #[test]
    fn a_finding_names_the_drive_it_is_about() {
        let finding = finding_for(&drive(DriveVerdict::Failing {
            detail: "failed".to_string(),
        }))
        .unwrap();
        assert_eq!(finding.subject.as_deref(), Some("/dev/sda"));
        assert!(finding.title.contains("Samsung SSD 860"));
    }

    #[test]
    fn no_serial_number_reaches_a_finding() {
        // DriveHealth never carries one, and this is the test that keeps it
        // that way if somebody adds a field.
        let finding = finding_for(&drive(DriveVerdict::Failing {
            detail: "failed".to_string(),
        }))
        .unwrap();
        let rendered = serde_json::to_string(&finding).unwrap();
        assert!(!rendered.to_lowercase().contains("serial"));
    }

    #[test]
    fn the_two_probes_answer_the_same_question_on_different_platforms() {
        // Same finding ids from both, so a runbook entry covers either. And
        // the platforms must not overlap, or one machine would be asked twice
        // and report the same drive as two problems.
        let windows = DiskHealthProbe.meta();
        let linux = SmartHealthProbe.meta();
        assert!(
            windows
                .platforms
                .iter()
                .all(|kind| !linux.platforms.contains(kind))
        );
        assert_eq!(windows.min_tier, linux.min_tier);
        assert!(!linux.requires_tools.is_empty() && linux.requires_elevation);
    }
}
