//! Free space on every fixed volume.
//!
//! A full system volume is one of the few problems that causes wildly
//! unrelated symptoms -- applications that will not start, updates that fail
//! halfway, logs that stop being written -- so it is worth checking before
//! anything else and worth reporting loudly.

use async_trait::async_trait;

use crate::Result;
use crate::finding::{Category, Finding, Severity, Triage};
use crate::platform::PlatformKind;
use crate::platform::{Volume, VolumeRole};
use crate::probe::{Probe, ProbeContext, ProbeMeta};
use crate::tier::ScanTier;
use crate::util::format_bytes;

const GIB: u64 = 1024 * 1024 * 1024;

/// Above this much free space, a low *percentage* is not urgent. A 4 TB drive
/// sitting at 8% free still has 320 GB of headroom, and warning about it
/// trains the user to ignore the tool.
const AMPLE_ABSOLUTE_HEADROOM: u64 = 64 * GIB;

#[derive(Debug, Default)]
pub struct DiskSpaceProbe;

/// Decide how bad a volume's free space is, or `None` if it is fine.
///
/// Both an absolute floor and a percentage are used, because either one alone
/// gets it wrong: a percentage misjudges very large and very small drives, and
/// an absolute floor misjudges a small SSD that is genuinely running out.
fn assess(volume: &Volume) -> Option<Severity> {
    // Removable media being full is the user's business, not a fault. A
    // read-only mount cannot be freed up, so flagging it is pure noise.
    if volume.role == VolumeRole::Removable || volume.read_only {
        return None;
    }

    let free_fraction = volume.free_fraction()?;
    let available = volume.available_bytes;
    let system = volume.role == VolumeRole::System;

    // Absolute floors. At these levels the machine is in trouble whatever the
    // drive's size is.
    if available < if system { 2 * GIB } else { GIB } {
        return Some(if system {
            Severity::Critical
        } else {
            Severity::High
        });
    }
    if available < if system { 10 * GIB } else { 5 * GIB } {
        return Some(if system {
            Severity::High
        } else {
            Severity::Medium
        });
    }

    // Percentage-based warnings, suppressed when there is plenty of room in
    // absolute terms.
    if available >= AMPLE_ABSOLUTE_HEADROOM {
        return None;
    }
    if system {
        if free_fraction < 0.05 {
            return Some(Severity::High);
        }
        if free_fraction < 0.10 {
            return Some(Severity::Medium);
        }
    } else {
        if free_fraction < 0.03 {
            return Some(Severity::Medium);
        }
        if free_fraction < 0.07 {
            return Some(Severity::Low);
        }
    }
    None
}

fn describe(volume: &Volume, severity: Severity) -> Finding {
    let free_percent = volume.free_fraction().unwrap_or(0.0) * 100.0;
    let role = match volume.role {
        VolumeRole::System => "the drive Windows or Linux itself runs from",
        VolumeRole::Data => "a data drive",
        VolumeRole::Removable => "removable media",
    };

    let detail = if volume.role == VolumeRole::System {
        format!(
            "{} is {role}, and only {} of {} is left ({free_percent:.1}% free). \
             When this drive fills up, programs fail to start, updates break \
             part-way through, and the system stops being able to write logs -- \
             usually showing up as problems that look unrelated to storage.",
            volume.mount_point,
            format_bytes(volume.available_bytes),
            format_bytes(volume.total_bytes),
        )
    } else {
        format!(
            "{} is {role} with only {} of {} left ({free_percent:.1}% free). \
             Anything writing to this drive will start failing once it is full.",
            volume.mount_point,
            format_bytes(volume.available_bytes),
            format_bytes(volume.total_bytes),
        )
    };

    Finding::builder("storage.disk-space", "storage.volume-low-on-space")
        .subject(&volume.mount_point)
        .severity(severity)
        .category(Category::Storage)
        .title(format!(
            "{} is running out of space -- {} free",
            volume.mount_point,
            format_bytes(volume.available_bytes)
        ))
        .detail(detail)
        .evidence("mount_point", &volume.mount_point)
        .evidence("device", &volume.device)
        .evidence("filesystem", &volume.filesystem)
        .evidence("total_bytes", volume.total_bytes.to_string())
        .evidence("available_bytes", volume.available_bytes.to_string())
        .evidence("used_bytes", volume.used_bytes().to_string())
        .evidence("free_percent", format!("{free_percent:.2}"))
        .evidence("role", format!("{:?}", volume.role))
        .remediation_hint(
            "Clear temporary files, caches, and old system update files, then \
             review the largest directories on the volume.",
        )
        // Reclaiming space has a known, safe, deterministic first step, so
        // this does not need to go through the triage queue.
        .triage(Triage::Inline)
        .build()
}

#[async_trait]
impl Probe for DiskSpaceProbe {
    fn meta(&self) -> ProbeMeta {
        ProbeMeta {
            id: "storage.disk-space",
            name: "Disk space",
            description: "Checks every fixed drive for dangerously low free space.",
            category: Category::Storage,
            min_tier: ScanTier::Quick,
            platforms: &[
                PlatformKind::Windows,
                PlatformKind::Linux,
                PlatformKind::MacOs,
            ],
            requires_tools: &[],
            emits: &["storage.volume-low-on-space"],
            requires_elevation: false,
        }
    }

    async fn run(&self, ctx: &ProbeContext) -> Result<Vec<Finding>> {
        let volumes = ctx.blocking(|platform| platform.volumes()).await?;
        Ok(volumes
            .iter()
            .filter_map(|volume| assess(volume).map(|severity| describe(volume, severity)))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn volume(role: VolumeRole, total: u64, available: u64) -> Volume {
        Volume {
            mount_point: "/test".to_string(),
            device: "test".to_string(),
            filesystem: "ext4".to_string(),
            total_bytes: total,
            available_bytes: available,
            role,
            read_only: false,
        }
    }

    #[test]
    fn healthy_volumes_are_not_flagged() {
        assert_eq!(
            assess(&volume(VolumeRole::System, 500 * GIB, 300 * GIB)),
            None
        );
        assert_eq!(assess(&volume(VolumeRole::Data, 100 * GIB, 50 * GIB)), None);
    }

    #[test]
    fn a_nearly_full_system_volume_is_critical() {
        assert_eq!(
            assess(&volume(VolumeRole::System, 500 * GIB, GIB)),
            Some(Severity::Critical)
        );
    }

    #[test]
    fn a_large_drive_with_ample_absolute_headroom_is_not_flagged_on_percentage() {
        // 8% free, but that is 320 GiB. Warning here would be noise.
        let big = volume(VolumeRole::System, 4000 * GIB, 320 * GIB);
        assert_eq!(assess(&big), None);
    }

    #[test]
    fn a_small_drive_low_on_percentage_is_still_flagged() {
        // 6% free of a 120 GiB SSD is about 7 GiB -- genuinely tight.
        let small = volume(VolumeRole::Data, 120 * GIB, 7 * GIB);
        assert_eq!(small.free_fraction().map(|f| f < 0.07), Some(true));
        assert!(assess(&small).is_some());
    }

    #[test]
    fn removable_and_read_only_volumes_are_ignored() {
        assert_eq!(assess(&volume(VolumeRole::Removable, 8 * GIB, 1024)), None);

        let mut read_only = volume(VolumeRole::Data, 8 * GIB, 1024);
        read_only.read_only = true;
        assert_eq!(assess(&read_only), None);
    }

    #[test]
    fn system_volumes_are_judged_more_strictly_than_data_volumes() {
        let total = 200 * GIB;
        let available = 8 * GIB;
        let system = assess(&volume(VolumeRole::System, total, available));
        let data = assess(&volume(VolumeRole::Data, total, available));
        assert!(
            system > data,
            "system {system:?} should outrank data {data:?}"
        );
    }
}
