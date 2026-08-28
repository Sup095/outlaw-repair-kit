//! The Deep tier's check: is the operating system still made of what it was
//! made of?
//!
//! Two probes, one per platform, because the two ask different tools the same
//! question -- and because Windows needs administrator rights to ask and Linux
//! does not.
//!
//! This is the only check in the tool that reports when it *could not* run to
//! completion. Everywhere else, "could not tell" produces nothing, because
//! reporting every USB enclosure that declines to talk about itself would bury
//! the drive that is genuinely dying. Here it is the other way round: somebody
//! who asked for a deep scan asked for this specific check, it is most of what
//! makes a deep scan take longer than a full one, and a deep scan that
//! silently skipped it would look exactly like a deep scan that ran it and
//! found nothing.
//!
//! See [`crate::platform::integrity`] for what each platform actually runs and
//! why an interrupted check is never a pass.

use async_trait::async_trait;

use crate::Result;
use crate::finding::{Category, Finding, Severity, Triage};
use crate::platform::PlatformKind;
use crate::platform::integrity::{IntegrityReport, IntegrityVerdict};
use crate::probe::{Probe, ProbeContext, ProbeMeta};
use crate::tier::ScanTier;

const PROBE_ID: &str = "system.files";

/// What either half of this check can report. Shared for the same reason as
/// the disk-health pair: one question, two ways of asking it.
const INTEGRITY_FINDINGS: &[&str] = &[
    "system.files-altered",
    "system.config-altered",
    "system.files-unverified",
];

/// How many damaged files to name in the finding itself.
///
/// The rest are counted. A finding with nine hundred paths in it is not a
/// finding anybody reads, and the full list is a command away in every case.
const NAMED: usize = 20;

/// Turn one integrity report into whatever findings it deserves.
pub fn findings_for(report: &IntegrityReport) -> Vec<Finding> {
    let mut findings = Vec::new();

    match &report.verdict {
        IntegrityVerdict::Intact => {}
        IntegrityVerdict::Damaged { files } => findings.push(damage(report, files)),
        IntegrityVerdict::CouldNotCheck { reason } => findings.push(could_not(report, reason)),
    }

    if !report.altered_config.is_empty() {
        findings.push(altered_config(report));
    }

    findings
}

fn damage(report: &IntegrityReport, files: &[String]) -> Finding {
    let detail = if files.is_empty() {
        format!(
            "`{}` reported that files belonging to the operating system no longer match what \
             installed them, but writes the names to its own log rather than to the screen.",
            report.checked_with
        )
    } else {
        format!(
            "{} belonging to installed packages no longer match what installed them. \
             This can mean a half-finished update, a disk that corrupted a file it was holding, \
             a program that overwrote a shared library with its own copy -- or something that \
             replaced a system file deliberately.\n\nBe aware that anything you edited yourself \
             counts as altered by this check, and on this platform the packaging tools do not \
             mark which files you were meant to edit. Recognising your own changes in this list \
             is the first thing to do with it.",
            crate::util::counted(files.len(), "file")
        )
    };

    let mut builder = Finding::builder(PROBE_ID, "system.files-altered")
        .severity(Severity::High)
        .category(Category::Configuration)
        .title("System files no longer match their packages")
        .detail(detail)
        .remediation_hint(hint_for(report))
        // No automatic action. Putting a different file over a system file is
        // exactly the kind of change that belongs in front of a person.
        .triage(Triage::Queue)
        .evidence("checked with", &report.checked_with);

    if let Some(log) = &report.log_hint {
        builder = builder.evidence("full detail in", log);
    }
    for path in files.iter().take(NAMED) {
        builder = builder.evidence("altered", path);
    }
    if files.len() > NAMED {
        builder = builder.evidence("and", format!("{} more", files.len() - NAMED));
    }

    builder.build()
}

fn hint_for(report: &IntegrityReport) -> String {
    if report.checked_with.starts_with("sfc") {
        "Run `sfc /scannow` from an administrator prompt. That is the repairing version of the \
         same check -- this tool deliberately runs only the verifying one. If it cannot fix \
         them, `DISM /Online /Cleanup-Image /RestoreHealth` repairs the store it draws \
         replacements from, and then `sfc /scannow` again."
            .to_string()
    } else if report.checked_with.starts_with("pacman") {
        "Work out which package owns each file with `pacman -Qo <file>`, then reinstall those \
         packages with `pacman -S <package>`. Leave anything you edited on purpose alone."
            .to_string()
    } else if report.checked_with.starts_with("rpm") {
        "Find the package with `rpm -qf <file>`, then reinstall it with \
         `dnf reinstall <package>`."
            .to_string()
    } else {
        "Find which package owns each file, and reinstall that package.".to_string()
    }
}

fn could_not(report: &IntegrityReport, reason: &str) -> Finding {
    let checked_with = if report.checked_with.is_empty() {
        "This machine has nothing installed that could answer.".to_string()
    } else {
        format!("The check ran `{}`.", report.checked_with)
    };

    Finding::builder(PROBE_ID, "system.files-unverified")
        .severity(Severity::Info)
        .category(Category::Configuration)
        .title("System files could not be verified")
        .detail(format!(
            "{reason}\n\n{checked_with} This is reported rather than passed over because it is \
             most of what a deep scan does that a full one does not, and a deep scan that \
             quietly skipped it would read exactly like one that ran it and found nothing wrong."
        ))
        .remediation_hint(
            "Nothing is known to be wrong. Nothing is known to be right either -- if you asked \
             for a deep scan because you suspect something has been tampered with, this is the \
             check you wanted, and it is worth getting it to run.",
        )
        .triage(Triage::None)
        .evidence("checked with", &report.checked_with)
        .build()
}

fn altered_config(report: &IntegrityReport) -> Finding {
    let mut builder = Finding::builder(PROBE_ID, "system.config-altered")
        .severity(Severity::Info)
        .category(Category::Configuration)
        .title(format!(
            "{} differ from the packaged version",
            crate::util::counted(report.altered_config.len(), "configuration file")
        ))
        .detail(
            "These are files the packages themselves marked as configuration -- files you are \
             meant to edit. Differing is normal and expected, and this is listed separately from \
             damage for exactly that reason. It is here because after an update it is the \
             quickest way to see which of your own settings a package may have replaced.",
        )
        .triage(Triage::None);

    for path in report.altered_config.iter().take(NAMED) {
        builder = builder.evidence("edited", path);
    }
    if report.altered_config.len() > NAMED {
        builder = builder.evidence(
            "and",
            format!("{} more", report.altered_config.len() - NAMED),
        );
    }

    builder.build()
}

async fn check(ctx: &ProbeContext) -> Result<Vec<Finding>> {
    let cancel = ctx.cancel_token().clone();
    let report = ctx
        .blocking(move |_platform: &dyn crate::Platform| crate::platform::integrity::check(&cancel))
        .await?;

    tracing::debug!(
        checked_with = report.checked_with,
        intact = report.verdict.is_intact(),
        "verified system files"
    );

    Ok(findings_for(&report))
}

/// The Windows half. `sfc` is administrator-only, and says so rather than
/// working around it.
#[derive(Debug, Default)]
pub struct SystemFilesProbe;

#[async_trait]
impl Probe for SystemFilesProbe {
    fn meta(&self) -> ProbeMeta {
        ProbeMeta {
            id: PROBE_ID,
            name: "System file integrity",
            description: "Verifies that Windows' own files still match what installed them. \
                          Reads and hashes a large part of the operating system, so it takes \
                          minutes rather than seconds -- which is what a deep scan is for.",
            category: Category::Configuration,
            min_tier: ScanTier::Deep,
            platforms: &[PlatformKind::Windows],
            requires_tools: &[],
            emits: INTEGRITY_FINDINGS,
            requires_elevation: true,
        }
    }

    async fn run(&self, ctx: &ProbeContext) -> Result<Vec<Finding>> {
        check(ctx).await
    }
}

/// The Linux half: the same question, asked of whichever package manager this
/// distribution uses.
///
/// Deliberately declares no required tool. Naming one would skip the check on
/// every distribution that uses a different one, and this probe reports what
/// it could not do instead of vanishing.
#[derive(Debug, Default)]
pub struct PackageFilesProbe;

#[async_trait]
impl Probe for PackageFilesProbe {
    fn meta(&self) -> ProbeMeta {
        ProbeMeta {
            id: "system.files-packaged",
            name: "System file integrity",
            description: "Asks the package manager whether the files it installed are still the \
                          files it installed. Reads and hashes most of what is installed, so it \
                          takes minutes rather than seconds.",
            category: Category::Configuration,
            min_tier: ScanTier::Deep,
            platforms: &[PlatformKind::Linux],
            requires_tools: &[],
            emits: INTEGRITY_FINDINGS,
            requires_elevation: false,
        }
    }

    async fn run(&self, ctx: &ProbeContext) -> Result<Vec<Finding>> {
        check(ctx).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(verdict: IntegrityVerdict) -> IntegrityReport {
        IntegrityReport {
            verdict,
            checked_with: "pacman -Qkk".to_string(),
            log_hint: None,
            altered_config: Vec::new(),
        }
    }

    #[test]
    fn an_intact_system_produces_nothing() {
        assert!(findings_for(&report(IntegrityVerdict::Intact)).is_empty());
    }

    #[test]
    fn damage_is_high_and_names_the_files() {
        let findings = findings_for(&report(IntegrityVerdict::Damaged {
            files: vec!["/usr/bin/sudo".to_string()],
        }));
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].id, "system.files-altered");
        assert_eq!(findings[0].severity, Severity::High);
        assert_eq!(findings[0].triage, Triage::Queue);
        assert!(
            findings[0]
                .evidence
                .iter()
                .any(|item| item.value.contains("/usr/bin/sudo"))
        );
    }

    #[test]
    fn a_long_list_is_cut_short_and_says_so() {
        // A finding with nine hundred paths in it is not a finding anybody
        // reads -- but the count must survive the trim, or the report
        // understates the problem.
        let files: Vec<String> = (0..500).map(|n| format!("/usr/bin/thing{n}")).collect();
        let findings = findings_for(&report(IntegrityVerdict::Damaged { files }));
        let evidence = &findings[0].evidence;
        assert!(evidence.len() < 30, "{} items is too many", evidence.len());
        assert!(
            evidence
                .iter()
                .any(|item| item.label == "and" && item.value == "480 more")
        );
        assert!(findings[0].detail.contains("500"));
    }

    #[test]
    fn a_check_that_could_not_run_is_still_reported() {
        // Everywhere else in the tool, "could not tell" produces nothing.
        // Here it must not: this check is most of what a deep scan does, and
        // skipping it silently would look identical to running it cleanly.
        let findings = findings_for(&report(IntegrityVerdict::CouldNotCheck {
            reason: "nothing here knows what the system files should look like".to_string(),
        }));
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].id, "system.files-unverified");
        assert_eq!(findings[0].severity, Severity::Info);
        assert_eq!(findings[0].triage, Triage::None);
    }

    #[test]
    fn edited_configuration_is_a_separate_finding_from_damage() {
        let mut with_config = report(IntegrityVerdict::Damaged {
            files: vec!["/usr/bin/sudo".to_string()],
        });
        with_config.altered_config = vec!["/etc/sudoers".to_string()];

        let findings = findings_for(&with_config);
        assert_eq!(findings.len(), 2);
        let config = findings
            .iter()
            .find(|finding| finding.id == "system.config-altered")
            .expect("edited configuration gets its own finding");
        assert_eq!(config.severity, Severity::Info);
        // The damage finding must not have absorbed it.
        let damage = findings
            .iter()
            .find(|finding| finding.id == "system.files-altered")
            .unwrap();
        assert!(!damage.evidence.iter().any(|e| e.value.contains("sudoers")));
    }

    #[test]
    fn the_advice_matches_the_tool_that_found_the_problem() {
        // Telling somebody on Arch to run `sfc /scannow` is worse than telling
        // them nothing.
        let mut windows = report(IntegrityVerdict::Damaged { files: Vec::new() });
        windows.checked_with = "sfc /verifyonly".to_string();
        let hint = findings_for(&windows)[0]
            .remediation_hint
            .clone()
            .unwrap_or_default();
        assert!(hint.contains("sfc /scannow"), "{hint}");

        let arch = findings_for(&report(IntegrityVerdict::Damaged {
            files: vec!["/usr/bin/sudo".to_string()],
        }))[0]
            .remediation_hint
            .clone()
            .unwrap_or_default();
        assert!(arch.contains("pacman"), "{arch}");
        assert!(!arch.contains("sfc"), "{arch}");
    }

    #[test]
    fn this_is_the_deep_tier_and_the_two_platforms_do_not_overlap() {
        // The Deep tier existed with nothing in it for a long time. This is
        // the test that says it no longer does.
        let windows = SystemFilesProbe.meta();
        let linux = PackageFilesProbe.meta();
        assert_eq!(windows.min_tier, ScanTier::Deep);
        assert_eq!(linux.min_tier, ScanTier::Deep);
        assert!(
            windows
                .platforms
                .iter()
                .all(|kind| !linux.platforms.contains(kind))
        );
        // sfc is administrator-only; the package managers are not.
        assert!(windows.requires_elevation);
        assert!(!linux.requires_elevation);
    }
}
