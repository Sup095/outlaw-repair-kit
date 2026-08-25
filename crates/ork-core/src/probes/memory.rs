//! Memory and swap pressure.
//!
//! This is a snapshot taken at one moment, and it says so in the findings it
//! produces. A machine that is briefly at 94% memory during a build is fine; a
//! machine that is at 94% and swapping heavily is not. The two signals are
//! therefore read together rather than separately, and sustained pressure over
//! time is left to the Full tier, which samples rather than snapshots.

use async_trait::async_trait;

use crate::Result;
use crate::finding::{Category, Finding, Severity, Triage};
use crate::platform::{MemoryInfo, PlatformKind};
use crate::probe::{Probe, ProbeContext, ProbeMeta};
use crate::tier::ScanTier;
use crate::util::format_bytes;

/// Memory use above this is worth mentioning at all.
const ELEVATED: f64 = 0.90;
/// Above this, an allocation failure is plausibly imminent.
const SEVERE: f64 = 0.96;
/// Swap use above this, combined with memory pressure, means the machine is
/// actively paging to keep going -- which is what users feel as the whole
/// system becoming unresponsive.
const SWAP_PRESSURE: f64 = 0.50;

fn assess(memory: &MemoryInfo) -> Option<Finding> {
    let used_fraction = memory.used_fraction()?;
    let swap_fraction = memory.swap_used_fraction().unwrap_or(0.0);

    // Heavy swapping alongside high memory use is the genuinely bad state, and
    // it is worse than either signal alone.
    let thrashing = used_fraction >= ELEVATED && swap_fraction >= SWAP_PRESSURE;

    let severity = if thrashing || used_fraction >= SEVERE {
        Severity::High
    } else if used_fraction >= ELEVATED {
        Severity::Medium
    } else {
        return None;
    };

    let detail = if thrashing {
        format!(
            "At the moment of the scan, {:.0}% of memory was in use and {:.0}% of swap was \
             occupied. The machine is moving memory to disk to keep running, which is why \
             everything feels slow at once rather than one program being slow. Note that \
             this is a single snapshot -- if a large job was running, this may be normal \
             for that job.",
            used_fraction * 100.0,
            swap_fraction * 100.0,
        )
    } else {
        format!(
            "At the moment of the scan, {:.0}% of memory was in use, leaving {}. Programs \
             may start failing to allocate memory, and on Linux the kernel may terminate \
             one to free space. This is a single snapshot and may simply reflect what was \
             running at the time.",
            used_fraction * 100.0,
            format_bytes(memory.available_bytes),
        )
    };

    Some(
        Finding::builder("memory.pressure", "memory.high-pressure")
            .severity(severity)
            .category(Category::Memory)
            .title(format!(
                "Memory is {:.0}% used -- {} free",
                used_fraction * 100.0,
                format_bytes(memory.available_bytes)
            ))
            .detail(detail)
            .evidence("total_bytes", memory.total_bytes.to_string())
            .evidence("available_bytes", memory.available_bytes.to_string())
            .evidence("used_percent", format!("{:.2}", used_fraction * 100.0))
            .evidence("swap_total_bytes", memory.swap_total_bytes.to_string())
            .evidence("swap_used_bytes", memory.swap_used_bytes.to_string())
            .evidence("swap_used_percent", format!("{:.2}", swap_fraction * 100.0))
            .remediation_hint(
                "Find what is holding the memory before changing anything -- a leak and a \
                 legitimately large workload look identical from a single snapshot.",
            )
            // One snapshot is not enough to know what the right action is, so
            // this is investigated rather than fixed inline.
            .triage(Triage::Queue)
            .build(),
    )
}

#[derive(Debug, Default)]
pub struct MemoryPressureProbe;

#[async_trait]
impl Probe for MemoryPressureProbe {
    fn meta(&self) -> ProbeMeta {
        ProbeMeta {
            id: "memory.pressure",
            name: "Memory pressure",
            description: "Checks whether the machine is short of memory or swapping heavily.",
            category: Category::Memory,
            min_tier: ScanTier::Quick,
            platforms: &[
                PlatformKind::Windows,
                PlatformKind::Linux,
                PlatformKind::MacOs,
            ],
            requires_tools: &[],
            requires_elevation: false,
        }
    }

    async fn run(&self, ctx: &ProbeContext) -> Result<Vec<Finding>> {
        let memory = ctx.blocking(|platform| platform.memory()).await?;
        Ok(assess(&memory).into_iter().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GIB: u64 = 1024 * 1024 * 1024;

    fn memory(total: u64, available: u64, swap_total: u64, swap_used: u64) -> MemoryInfo {
        MemoryInfo {
            total_bytes: total,
            available_bytes: available,
            swap_total_bytes: swap_total,
            swap_used_bytes: swap_used,
        }
    }

    #[test]
    fn ordinary_memory_use_is_not_a_finding() {
        assert!(assess(&memory(64 * GIB, 32 * GIB, 8 * GIB, 0)).is_none());
        // 88% used is busy, not broken.
        assert!(assess(&memory(64 * GIB, 8 * GIB, 8 * GIB, 0)).is_none());
    }

    #[test]
    fn high_use_without_swapping_is_medium() {
        let finding = assess(&memory(64 * GIB, 4 * GIB, 8 * GIB, 0)).expect("expected a finding");
        assert_eq!(finding.severity, Severity::Medium);
    }

    #[test]
    fn nearly_exhausted_memory_is_high() {
        let finding = assess(&memory(64 * GIB, GIB, 8 * GIB, 0)).expect("expected a finding");
        assert_eq!(finding.severity, Severity::High);
    }

    #[test]
    fn high_use_plus_heavy_swapping_is_high_even_below_the_severe_threshold() {
        // 91% memory and 60% swap: not extreme on either axis alone, but this
        // is the state where the whole machine feels frozen.
        let thrashing = memory(64 * GIB, 6 * GIB, 16 * GIB, 10 * GIB);
        let finding = assess(&thrashing).expect("expected a finding");
        assert_eq!(finding.severity, Severity::High);
        assert!(finding.detail.contains("swap"));
    }

    #[test]
    fn a_machine_with_no_swap_is_judged_on_memory_alone() {
        // Dividing by a zero swap total must not panic or fabricate pressure.
        let finding = assess(&memory(16 * GIB, 1024 * 1024, 0, 0)).expect("expected a finding");
        assert_eq!(finding.severity, Severity::High);
    }

    #[test]
    fn a_zero_sized_machine_is_not_a_finding() {
        assert!(assess(&memory(0, 0, 0, 0)).is_none());
    }
}
