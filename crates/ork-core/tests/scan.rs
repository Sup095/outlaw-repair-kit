//! End-to-end tests for the scan orchestrator.
//!
//! These drive the real `Scanner` and the real probes against a fake machine,
//! which is the only way to test the states that matter -- a disk about to
//! fill, a probe that fails, a platform a probe does not support -- without
//! waiting for a real computer to break.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use ork_core::finding::{Category, Finding, Severity};
use ork_core::platform::{
    DeviceIssue, HostInfo, LogRecord, MemoryInfo, Platform, PlatformKind, ProcessInfo, Volume,
    VolumeRole,
};
use ork_core::probe::{Probe, ProbeContext, ProbeMeta, ProbeStatus, SkipReason};
use ork_core::probes::disk_space::DiskSpaceProbe;
use ork_core::{ScanTier, Scanner};

const GIB: u64 = 1024 * 1024 * 1024;

/// A machine we can put into any state we like.
struct FakeMachine {
    kind: PlatformKind,
    volumes: Vec<Volume>,
    available_tools: Vec<&'static str>,
}

impl FakeMachine {
    fn new(kind: PlatformKind) -> Self {
        Self {
            kind,
            volumes: Vec::new(),
            available_tools: Vec::new(),
        }
    }

    fn with_volume(mut self, mount: &str, total: u64, available: u64, role: VolumeRole) -> Self {
        self.volumes.push(Volume {
            mount_point: mount.to_string(),
            device: "fake".to_string(),
            filesystem: "ext4".to_string(),
            total_bytes: total,
            available_bytes: available,
            role,
            read_only: false,
        });
        self
    }
}

impl Platform for FakeMachine {
    fn kind(&self) -> PlatformKind {
        self.kind
    }

    fn host(&self) -> ork_core::Result<HostInfo> {
        Ok(HostInfo {
            hostname: "fake-machine".to_string(),
            os_name: "Fake OS".to_string(),
            os_version: "1.0".to_string(),
            kernel_version: "1.0".to_string(),
            arch: "x86_64".to_string(),
            cpu_brand: "Fake CPU".to_string(),
            physical_cores: Some(4),
            logical_cores: 8,
            total_memory_bytes: 16 * GIB,
        })
    }

    fn volumes(&self) -> ork_core::Result<Vec<Volume>> {
        Ok(self.volumes.clone())
    }

    fn processes(&self) -> ork_core::Result<Vec<ProcessInfo>> {
        Ok(Vec::new())
    }

    fn memory(&self) -> ork_core::Result<MemoryInfo> {
        Ok(MemoryInfo {
            total_bytes: 16 * GIB,
            available_bytes: 8 * GIB,
            swap_total_bytes: 0,
            swap_used_bytes: 0,
        })
    }

    fn recent_log_errors(&self, _since: Duration) -> ork_core::Result<Vec<LogRecord>> {
        Ok(Vec::new())
    }

    fn device_issues(&self) -> ork_core::Result<Vec<DeviceIssue>> {
        Ok(Vec::new())
    }

    fn locate_tool(&self, tool: &str) -> Option<std::path::PathBuf> {
        self.available_tools
            .contains(&tool)
            .then(|| std::path::PathBuf::from(tool))
    }
}

/// A probe that always breaks, to prove one failure does not lose a scan.
struct BrokenProbe;

#[async_trait]
impl Probe for BrokenProbe {
    fn meta(&self) -> ProbeMeta {
        ProbeMeta {
            id: "test.broken",
            name: "Broken probe",
            description: "Always fails.",
            category: Category::Configuration,
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

    async fn run(&self, _ctx: &ProbeContext) -> ork_core::Result<Vec<Finding>> {
        anyhow::bail!("this probe is deliberately broken")
    }
}

/// A probe that needs a tool the fake machine does not have.
struct NeedsMissingToolProbe;

#[async_trait]
impl Probe for NeedsMissingToolProbe {
    fn meta(&self) -> ProbeMeta {
        ProbeMeta {
            id: "test.needs-tool",
            name: "Needs a missing tool",
            description: "Requires something that is not installed.",
            category: Category::Storage,
            min_tier: ScanTier::Quick,
            platforms: &[
                PlatformKind::Windows,
                PlatformKind::Linux,
                PlatformKind::MacOs,
            ],
            requires_tools: &["definitely-not-installed"],
            requires_elevation: false,
        }
    }

    async fn run(&self, _ctx: &ProbeContext) -> ork_core::Result<Vec<Finding>> {
        panic!("a probe missing its required tool must never be run");
    }
}

fn scanner_for(machine: FakeMachine, probes: Vec<Box<dyn Probe>>) -> Scanner {
    Scanner::with_probes(Arc::new(machine), probes)
}

#[tokio::test]
async fn a_full_system_volume_is_reported_as_critical() {
    let machine = FakeMachine::new(PlatformKind::Linux)
        // 500 MiB left on the drive the OS runs from.
        .with_volume("/", 256 * GIB, 512 * 1024 * 1024, VolumeRole::System)
        .with_volume("/data", 4000 * GIB, 2000 * GIB, VolumeRole::Data);

    let report = scanner_for(machine, vec![Box::new(DiskSpaceProbe)])
        .run(ScanTier::Quick)
        .await
        .unwrap();

    let findings = report.findings();
    assert_eq!(
        findings.len(),
        1,
        "only the system volume should be flagged"
    );
    assert_eq!(findings[0].severity, Severity::Critical);
    assert_eq!(findings[0].subject.as_deref(), Some("/"));
    assert_eq!(report.worst_severity(), Some(Severity::Critical));

    // The finding has to carry enough for a person and for the AI layer.
    assert!(!findings[0].title.is_empty());
    assert!(!findings[0].detail.is_empty());
    assert!(findings[0].remediation_hint.is_some());
    assert!(
        findings[0]
            .evidence
            .iter()
            .any(|item| item.label == "available_bytes")
    );
}

#[tokio::test]
async fn one_broken_probe_does_not_cost_the_rest_of_the_scan() {
    let machine = FakeMachine::new(PlatformKind::Linux).with_volume(
        "/",
        256 * GIB,
        512 * 1024 * 1024,
        VolumeRole::System,
    );

    let report = scanner_for(
        machine,
        vec![Box::new(BrokenProbe), Box::new(DiskSpaceProbe)],
    )
    .run(ScanTier::Quick)
    .await
    .unwrap();

    // The disk finding survived the broken probe that ran before it.
    assert_eq!(report.findings().len(), 1);
    assert_eq!(report.failed().count(), 1);
    let failed = report.failed().next().unwrap();
    assert!(matches!(&failed.status, ProbeStatus::Failed { error } if error.contains("broken")));
}

#[tokio::test]
async fn a_probe_missing_its_tool_is_skipped_visibly_not_silently() {
    // The whole point: a scan that covered less than the user thinks it did is
    // worse than one that says so.
    let machine = FakeMachine::new(PlatformKind::Linux);

    let report = scanner_for(machine, vec![Box::new(NeedsMissingToolProbe)])
        .run(ScanTier::Quick)
        .await
        .unwrap();

    assert_eq!(report.findings().len(), 0);
    let skipped: Vec<_> = report.skipped().collect();
    assert_eq!(skipped.len(), 1);
    match &skipped[0].status {
        ProbeStatus::Skipped(SkipReason::MissingTool { tool }) => {
            assert_eq!(tool, "definitely-not-installed");
            // The reason has to be sayable to a person, since it is shown in
            // the report rather than logged and forgotten.
            let explanation = SkipReason::MissingTool { tool: tool.clone() }.to_string();
            assert!(
                explanation.contains("definitely-not-installed"),
                "got {explanation}"
            );
        }
        other => panic!("expected a missing-tool skip, got {other:?}"),
    }
}

#[tokio::test]
async fn a_probe_is_skipped_on_a_platform_it_does_not_support() {
    struct LinuxOnlyProbe;

    #[async_trait]
    impl Probe for LinuxOnlyProbe {
        fn meta(&self) -> ProbeMeta {
            ProbeMeta {
                id: "test.linux-only",
                name: "Linux only",
                description: "Runs on Linux and nowhere else.",
                category: Category::Logs,
                min_tier: ScanTier::Quick,
                platforms: &[PlatformKind::Linux],
                requires_tools: &[],
                requires_elevation: false,
            }
        }

        async fn run(&self, _ctx: &ProbeContext) -> ork_core::Result<Vec<Finding>> {
            panic!("must not run on an unsupported platform");
        }
    }

    let report = scanner_for(
        FakeMachine::new(PlatformKind::Windows),
        vec![Box::new(LinuxOnlyProbe)],
    )
    .run(ScanTier::Quick)
    .await
    .unwrap();

    assert!(matches!(
        report.skipped().next().map(|outcome| &outcome.status),
        Some(ProbeStatus::Skipped(SkipReason::UnsupportedPlatform { .. }))
    ));
}

#[tokio::test]
async fn a_probe_above_the_requested_tier_does_not_run() {
    struct DeepProbe;

    #[async_trait]
    impl Probe for DeepProbe {
        fn meta(&self) -> ProbeMeta {
            ProbeMeta {
                id: "test.deep",
                name: "Deep only",
                description: "Only runs in a deep scan.",
                category: Category::Cpu,
                min_tier: ScanTier::Deep,
                platforms: &[
                    PlatformKind::Windows,
                    PlatformKind::Linux,
                    PlatformKind::MacOs,
                ],
                requires_tools: &[],
                requires_elevation: false,
            }
        }

        async fn run(&self, _ctx: &ProbeContext) -> ork_core::Result<Vec<Finding>> {
            Ok(vec![
                Finding::builder("test.deep", "test.deep-ran")
                    .title("the deep probe ran")
                    .build(),
            ])
        }
    }

    let quick = scanner_for(
        FakeMachine::new(PlatformKind::Linux),
        vec![Box::new(DeepProbe)],
    )
    .run(ScanTier::Quick)
    .await
    .unwrap();
    assert_eq!(
        quick.skipped().count(),
        1,
        "a deep probe must not run during a quick scan"
    );
    assert!(quick.findings().is_empty());

    // ...and the same probe does run when the tier is high enough.
    let deep = scanner_for(
        FakeMachine::new(PlatformKind::Linux),
        vec![Box::new(DeepProbe)],
    )
    .run(ScanTier::Deep)
    .await
    .unwrap();
    assert_eq!(deep.skipped().count(), 0);
    assert_eq!(deep.findings().len(), 1);
}

#[tokio::test]
async fn a_healthy_machine_produces_no_findings() {
    let machine = FakeMachine::new(PlatformKind::Linux).with_volume(
        "/",
        512 * GIB,
        300 * GIB,
        VolumeRole::System,
    );

    let report = scanner_for(machine, vec![Box::new(DiskSpaceProbe)])
        .run(ScanTier::Quick)
        .await
        .unwrap();

    assert!(report.findings().is_empty());
    assert_eq!(report.worst_severity(), None);
    assert!(!report.cancelled);
}
