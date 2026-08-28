//! What the running processes say about the machine's health.
//!
//! This probe is deliberately conservative. A process using a lot of CPU or
//! memory is usually a process doing its job, and a tool that shouts about
//! every busy compiler teaches its user to ignore it. Each check here is
//! therefore paired with a qualifier that separates "working hard" from
//! "stuck or leaking": how long it has been running, or how much of the
//! machine it has taken.

use std::collections::BTreeMap;

use async_trait::async_trait;

use crate::Result;
use crate::finding::{Category, Finding, Severity, Triage};
use crate::platform::{MemoryInfo, PlatformKind, ProcessInfo, ProcessState};
use crate::probe::{Probe, ProbeContext, ProbeMeta};
use crate::tier::ScanTier;
use crate::util::format_bytes;

/// A process must hold at least this share of total memory before it is worth
/// a word. Below this, it is simply a large program.
const MEMORY_HOG_FRACTION: f64 = 0.40;

/// Sustained CPU use, as a percentage of a single core.
const HIGH_CPU_PERCENT: f32 = 90.0;

/// How long a process must have been pinned before "busy" becomes "stuck".
/// Ten minutes clears builds, video encodes, and game loading, while still
/// catching a spin-loop long before the user gives up and reboots.
const SUSTAINED_SECS: u64 = 10 * 60;

/// Zombies below this count are ordinary scheduling transience.
const ZOMBIE_THRESHOLD: usize = 20;

fn memory_hog(processes: &[ProcessInfo], memory: &MemoryInfo) -> Option<Finding> {
    if memory.total_bytes == 0 {
        return None;
    }
    let worst = processes
        .iter()
        .max_by_key(|process| process.memory_bytes)?;
    let fraction = worst.memory_bytes as f64 / memory.total_bytes as f64;
    if fraction < MEMORY_HOG_FRACTION {
        return None;
    }

    Some(
        Finding::builder("system.processes", "process.memory-hog")
            .subject(&worst.name)
            .severity(if fraction >= 0.75 {
                Severity::Medium
            } else {
                Severity::Low
            })
            .category(Category::Memory)
            .title(format!(
                "`{}` is holding {} of memory ({:.0}% of the machine)",
                worst.name,
                format_bytes(worst.memory_bytes),
                fraction * 100.0
            ))
            .detail(format!(
                "One process is holding {:.0}% of this machine's memory. That is normal for \
                 a virtual machine, a database, or a large editing job, and abnormal for \
                 something that is supposed to sit in the background. If this program has \
                 been running for a long time and its memory use only ever grows, that is \
                 the shape of a memory leak.",
                fraction * 100.0
            ))
            .evidence("process", &worst.name)
            .evidence("pid", worst.pid.to_string())
            .evidence("memory_bytes", worst.memory_bytes.to_string())
            .evidence("memory_percent", format!("{:.2}", fraction * 100.0))
            .evidence("run_time_secs", worst.run_time_secs.to_string())
            .evidence(
                "executable",
                worst
                    .executable
                    .clone()
                    .unwrap_or_else(|| "unknown".to_string()),
            )
            .remediation_hint(
                "Check whether this is expected for the program. If its memory use grows \
                 without bound over hours, restart it and watch whether the pattern repeats.",
            )
            .triage(Triage::Queue)
            .build(),
    )
}

fn stuck_processes(processes: &[ProcessInfo]) -> Vec<Finding> {
    processes
        .iter()
        .filter(|process| {
            process.cpu_percent >= HIGH_CPU_PERCENT && process.run_time_secs >= SUSTAINED_SECS
        })
        .map(|process| {
            let hours = process.run_time_secs as f64 / 3600.0;
            Finding::builder("system.processes", "process.sustained-high-cpu")
                .subject(&process.name)
                .severity(Severity::Low)
                .category(Category::Cpu)
                .title(format!(
                    "`{}` has been using {:.0}% of a CPU core for {hours:.1} hours",
                    process.name, process.cpu_percent
                ))
                .detail(format!(
                    "This process has held a processor core at {:.0}% for {hours:.1} hours. \
                     That is entirely normal for a long render, a compile, or a background \
                     indexer, and it is what a stuck program looks like too -- the two are \
                     indistinguishable from outside. It is reported so you can tell which \
                     one this is.",
                    process.cpu_percent
                ))
                .evidence("process", &process.name)
                .evidence("pid", process.pid.to_string())
                .evidence("cpu_percent", format!("{:.1}", process.cpu_percent))
                .evidence("run_time_secs", process.run_time_secs.to_string())
                .evidence(
                    "executable",
                    process
                        .executable
                        .clone()
                        .unwrap_or_else(|| "unknown".to_string()),
                )
                .remediation_hint(
                    "Confirm whether this program is meant to be working right now. If it \
                     is not, it is stuck.",
                )
                .triage(Triage::Queue)
                .build()
        })
        .collect()
}

fn zombies(processes: &[ProcessInfo]) -> Option<Finding> {
    let zombies: Vec<&ProcessInfo> = processes
        .iter()
        .filter(|process| process.state == ProcessState::Zombie)
        .collect();
    if zombies.len() < ZOMBIE_THRESHOLD {
        return None;
    }

    // Zombies are the parent's fault, not their own, so the parent is the
    // useful thing to name.
    let mut by_parent: BTreeMap<u32, usize> = BTreeMap::new();
    for zombie in &zombies {
        *by_parent.entry(zombie.parent_pid.unwrap_or(0)).or_default() += 1;
    }
    let worst_parent = by_parent
        .iter()
        .max_by_key(|(_, count)| **count)
        .map(|(pid, _)| *pid);
    let parent_name = worst_parent
        .and_then(|pid| processes.iter().find(|process| process.pid == pid))
        .map(|process| process.name.clone())
        .unwrap_or_else(|| "an unknown process".to_string());

    Some(
        Finding::builder("system.processes", "process.zombie-buildup")
            .severity(if zombies.len() >= ZOMBIE_THRESHOLD * 5 {
                Severity::Medium
            } else {
                Severity::Low
            })
            .category(Category::Performance)
            .title(format!(
                "{} finished processes have not been cleaned up",
                zombies.len()
            ))
            .detail(format!(
                "{} processes have exited but are still occupying entries in the process \
                 table, because whatever started them never collected their exit status. \
                 Most of them belong to `{parent_name}`. A few of these at any moment is \
                 normal; this many means a program is leaking them, and left alone it will \
                 eventually exhaust the process table and stop the machine starting anything \
                 new.",
                zombies.len()
            ))
            .evidence("zombie_count", zombies.len().to_string())
            .evidence("worst_parent", &parent_name)
            .evidence(
                "worst_parent_pid",
                worst_parent
                    .map(|pid| pid.to_string())
                    .unwrap_or_else(|| "unknown".to_string()),
            )
            .remediation_hint(
                "Restarting the parent program clears them. If they come back, that program \
                 has a bug worth reporting.",
            )
            .triage(Triage::Queue)
            .build(),
    )
}

#[derive(Debug, Default)]
pub struct ProcessesProbe;

#[async_trait]
impl Probe for ProcessesProbe {
    fn meta(&self) -> ProbeMeta {
        ProbeMeta {
            id: "system.processes",
            name: "Running processes",
            description: "Looks for processes that are stuck, leaking memory, or piling up unreaped.",
            category: Category::Performance,
            min_tier: ScanTier::Quick,
            platforms: &[
                PlatformKind::Windows,
                PlatformKind::Linux,
                PlatformKind::MacOs,
            ],
            requires_tools: &[],
            emits: &[
                "process.memory-hog",
                "process.sustained-high-cpu",
                "process.zombie-buildup",
            ],
            requires_elevation: false,
        }
    }

    async fn run(&self, ctx: &ProbeContext) -> Result<Vec<Finding>> {
        let processes = ctx.blocking(|platform| platform.processes()).await?;
        let memory = ctx.blocking(|platform| platform.memory()).await?;
        tracing::debug!(processes = processes.len(), "inspected processes");

        let mut findings = Vec::new();
        findings.extend(memory_hog(&processes, &memory));
        findings.extend(zombies(&processes));
        findings.extend(stuck_processes(&processes));
        Ok(findings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GIB: u64 = 1024 * 1024 * 1024;

    fn process(name: &str, memory: u64, cpu: f32, run_time: u64) -> ProcessInfo {
        ProcessInfo {
            pid: 100,
            parent_pid: Some(1),
            name: name.to_string(),
            executable: Some(format!("/usr/bin/{name}")),
            memory_bytes: memory,
            cpu_percent: cpu,
            run_time_secs: run_time,
            state: ProcessState::Running,
        }
    }

    fn memory_info(total: u64) -> MemoryInfo {
        MemoryInfo {
            total_bytes: total,
            available_bytes: total / 2,
            swap_total_bytes: 0,
            swap_used_bytes: 0,
        }
    }

    #[test]
    fn an_ordinary_workload_produces_nothing() {
        let processes = vec![process("editor", 2 * GIB, 12.0, 3600)];
        assert!(memory_hog(&processes, &memory_info(64 * GIB)).is_none());
        assert!(stuck_processes(&processes).is_empty());
        assert!(zombies(&processes).is_none());
    }

    #[test]
    fn a_process_holding_most_of_memory_is_reported() {
        let processes = vec![process("vm", 40 * GIB, 5.0, 600)];
        let finding = memory_hog(&processes, &memory_info(64 * GIB)).expect("expected a finding");
        assert_eq!(finding.id, "process.memory-hog");
        assert_eq!(finding.severity, Severity::Low);
    }

    #[test]
    fn holding_almost_all_of_memory_is_worse() {
        let processes = vec![process("leaky", 50 * GIB, 5.0, 600)];
        let finding = memory_hog(&processes, &memory_info(64 * GIB)).expect("expected a finding");
        assert_eq!(finding.severity, Severity::Medium);
    }

    #[test]
    fn a_brief_cpu_spike_is_not_a_stuck_process() {
        // 100% CPU for thirty seconds is a program doing its job.
        let processes = vec![process("compiler", GIB, 100.0, 30)];
        assert!(stuck_processes(&processes).is_empty());
    }

    #[test]
    fn a_long_pinned_process_is_reported_but_only_as_context() {
        let processes = vec![process("spinner", GIB, 99.0, 4 * 3600)];
        let findings = stuck_processes(&processes);
        assert_eq!(findings.len(), 1);
        // Indistinguishable from legitimate work, so it must not be alarming.
        assert_eq!(findings[0].severity, Severity::Low);
        assert!(findings[0].detail.contains("normal"));
    }

    #[test]
    fn a_few_zombies_are_normal() {
        let processes: Vec<ProcessInfo> = (0..5)
            .map(|_| {
                let mut zombie = process("defunct", 0, 0.0, 10);
                zombie.state = ProcessState::Zombie;
                zombie
            })
            .collect();
        assert!(zombies(&processes).is_none());
    }

    #[test]
    fn a_pile_of_zombies_names_the_parent_responsible() {
        let mut processes = vec![process("badparent", GIB, 1.0, 100)];
        processes[0].pid = 42;
        processes.extend((0..30).map(|_| {
            let mut zombie = process("defunct", 0, 0.0, 10);
            zombie.state = ProcessState::Zombie;
            zombie.parent_pid = Some(42);
            zombie
        }));

        let finding = zombies(&processes).expect("expected a finding");
        assert_eq!(finding.id, "process.zombie-buildup");
        assert!(finding.detail.contains("badparent"));
    }

    #[test]
    fn a_machine_reporting_no_memory_does_not_panic() {
        let processes = vec![process("anything", GIB, 1.0, 10)];
        assert!(memory_hog(&processes, &memory_info(0)).is_none());
    }
}
