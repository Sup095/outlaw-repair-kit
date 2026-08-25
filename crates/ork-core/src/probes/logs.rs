//! Recent errors in the system log.
//!
//! Most instability that a user describes as "it just freezes sometimes" is
//! already written down in the system log, in a form nobody reads. This probe
//! does two things with that log: it recognises specific fault signatures that
//! have a known meaning, and it clusters everything else by how often it
//! repeats, on the theory that a fault occurring two hundred times is a
//! different problem from one that happened once.
//!
//! It deliberately does not try to explain novel errors. That is the AI
//! analysis layer's job, and it works from the structured findings this probe
//! produces rather than from the raw log.

use std::collections::BTreeMap;
use std::time::Duration;

use async_trait::async_trait;

use crate::Result;
use crate::finding::{Category, Finding, Severity, Triage};
use crate::platform::{LogLevel, LogRecord, PlatformKind};
use crate::probe::{Probe, ProbeContext, ProbeMeta};
use crate::tier::ScanTier;

/// How far back a Quick scan looks.
///
/// Three days covers "it crashed over the weekend" without burying a genuinely
/// current problem under a month of history. The Full tier will parse the
/// whole retained log instead.
const QUICK_WINDOW: Duration = Duration::from_secs(72 * 60 * 60);

/// How many repeats of the same unrecognised fault before it is worth
/// reporting on its own. Below this, one-off errors during normal operation
/// are ordinary noise.
const CLUSTER_THRESHOLD: usize = 12;

/// A fault signature with a known meaning.
///
/// A pattern matches when *every* criterion it specifies matches. An empty
/// criterion is not a wildcard match requirement -- it is simply not checked.
struct FaultPattern {
    id: &'static str,
    /// Substrings matched case-insensitively against the log source.
    sources: &'static [&'static str],
    /// Exact event IDs, as the platform reports them.
    event_ids: &'static [&'static str],
    /// Substrings matched case-insensitively against the message.
    messages: &'static [&'static str],
    severity: Severity,
    category: Category,
    /// Short headline, in plain language.
    title: &'static str,
    /// What this actually means for the user.
    explanation: &'static str,
    hint: &'static str,
}

impl FaultPattern {
    fn matches(&self, record: &LogRecord) -> bool {
        let source = record.source.to_ascii_lowercase();
        let message = record.message.to_ascii_lowercase();

        if !self.sources.is_empty()
            && !self
                .sources
                .iter()
                .any(|needle| source.contains(&needle.to_ascii_lowercase()))
        {
            return false;
        }
        if !self.event_ids.is_empty()
            && !self.event_ids.iter().any(|id| {
                record
                    .event_id
                    .as_deref()
                    .is_some_and(|actual| actual == *id)
            })
        {
            return false;
        }
        if !self.messages.is_empty()
            && !self
                .messages
                .iter()
                .any(|needle| message.contains(&needle.to_ascii_lowercase()))
        {
            return false;
        }
        // A pattern with no criteria at all would match everything, which is
        // always a mistake in the table rather than an intended wildcard.
        !self.sources.is_empty() || !self.event_ids.is_empty() || !self.messages.is_empty()
    }
}

/// Fault signatures worth recognising by name.
///
/// This table is deliberately small and high-confidence. Anything speculative
/// belongs in the runbook library, where entries can be added and revised
/// without a rebuild -- not here, where a false positive is compiled in.
const PATTERNS: &[FaultPattern] = &[
    FaultPattern {
        id: "logs.unexpected-shutdown",
        sources: &["Kernel-Power"],
        event_ids: &["41"],
        messages: &[],
        severity: Severity::High,
        category: Category::Logs,
        title: "The system shut down without warning",
        explanation: "Windows recorded that the machine lost power or stopped responding without \
             shutting down cleanly. This is what a freeze, a hard lock-up, or a blue screen \
             looks like from the log's point of view. Repeated occurrences usually mean a \
             driver conflict, failing memory, an overheating component, or an unstable \
             power supply -- not a software bug in whatever was on screen at the time.",
        hint: "Correlate against recent driver or firmware changes, then test memory and \
               check temperatures under load.",
    },
    FaultPattern {
        id: "logs.bugcheck",
        sources: &["BugCheck"],
        event_ids: &[],
        messages: &[],
        severity: Severity::High,
        category: Category::Logs,
        title: "The system crashed with a blue screen",
        explanation: "Windows recorded a bug check -- a blue screen. The stop code in the message \
             identifies which component failed, and it is very often a driver rather than \
             the hardware it drives.",
        hint: "Identify the stop code and the faulting module, then check whether that \
               driver was recently installed or updated.",
    },
    FaultPattern {
        id: "logs.hardware-error",
        sources: &["WHEA-Logger"],
        event_ids: &[],
        messages: &[],
        severity: Severity::Critical,
        category: Category::Cpu,
        title: "The hardware reported a machine-check error",
        explanation: "The processor's own error reporting recorded a fault. This comes from the \
             hardware itself, not from software interpreting it, which makes it one of the \
             strongest signals available that a component is genuinely failing or is \
             unstable at its current settings.",
        hint: "Check for overclocking or undervolting, verify cooling and power delivery, \
               then test memory and the processor individually.",
    },
    FaultPattern {
        id: "logs.display-driver-timeout",
        sources: &["Display", "nvlddmkm", "amdkmdap"],
        event_ids: &[],
        messages: &[
            "stopped responding",
            "has been recovered",
            "timeout detection",
        ],
        severity: Severity::High,
        category: Category::Gpu,
        title: "The graphics driver stopped responding and had to restart",
        explanation: "The display driver stopped responding for long enough that the operating \
             system reset it. Users see this as the screen going black for a second, a \
             game crashing to the desktop, or a full freeze. It is usually a driver \
             version problem, an unstable GPU overclock, or a power delivery issue -- and \
             it is one of the clearest signs of a driver/system mismatch.",
        hint: "Check which driver version is installed and when it changed, and test with \
               any GPU overclock removed.",
    },
    FaultPattern {
        id: "logs.kernel-panic",
        sources: &[],
        event_ids: &[],
        messages: &[
            "kernel panic",
            "oops:",
            "bug: unable to handle",
            "soft lockup",
            "hard lockup",
        ],
        severity: Severity::Critical,
        category: Category::Logs,
        title: "The kernel crashed",
        explanation: "The Linux kernel hit a fault it could not recover from. The machine either              froze or rebooted at that moment. The module named in the message is the              place to start, and a kernel or driver update shortly beforehand is the most              common cause.",
        hint: "Check which kernel and driver versions were installed around the time of                the crash, and whether an older combination was stable.",
    },
    FaultPattern {
        id: "logs.gpu-fault",
        sources: &[],
        event_ids: &[],
        messages: &[
            "nvrm: xid",
            "gpu has fallen off the bus",
            "amdgpu: gpu reset",
            "ring gfx timeout",
        ],
        severity: Severity::High,
        category: Category::Gpu,
        title: "The graphics card reported a fault",
        explanation: "The GPU driver recorded a hardware-level fault. On NVIDIA hardware these are              Xid errors, and the number identifies the class of fault. This is the log              signature behind freezes and crashes that look like they belong to whatever              application happened to be running.",
        hint: "Note the exact fault code, check the installed driver against the one the                distribution recommends for this card, and test with any overclock removed.",
    },
    FaultPattern {
        id: "logs.oom-kill",
        sources: &[],
        event_ids: &[],
        messages: &["out of memory: killed", "oom-killer", "killed process"],
        severity: Severity::High,
        category: Category::Memory,
        title: "The system ran out of memory and killed a program",
        explanation: "The kernel ran out of usable memory and terminated a process to stay alive.              The program that died is usually the biggest one running rather than the one              at fault, so this often looks like a random application crash.",
        hint: "Identify what consumed the memory, and check whether swap is configured and                large enough for this workload.",
    },
    FaultPattern {
        id: "logs.storage-error",
        sources: &["disk", "ntfs", "nvme", "ata"],
        event_ids: &[],
        messages: &[
            "i/o error",
            "bad block",
            "unrecoverable",
            "medium error",
            "failed command",
        ],
        severity: Severity::Critical,
        category: Category::Storage,
        title: "A drive reported read or write errors",
        explanation: "The storage layer reported errors reaching a drive. Unlike most log noise,              this one predicts data loss: drives that report I/O errors frequently go on              to fail outright.",
        hint: "Back up anything irreplaceable from this drive first, then read its SMART                health data before doing anything else.",
    },
];

/// A group of log records that all mean the same thing.
struct Cluster<'a> {
    records: Vec<&'a LogRecord>,
}

impl<'a> Cluster<'a> {
    fn count(&self) -> usize {
        self.records.len()
    }

    fn worst_level(&self) -> LogLevel {
        self.records
            .iter()
            .map(|record| record.level)
            .max()
            .unwrap_or(LogLevel::Warning)
    }

    /// A representative message, kept short enough to read.
    fn sample(&self) -> String {
        let message = self
            .records
            .first()
            .map(|record| record.message.as_str())
            .unwrap_or_default()
            .replace(['\r', '\n'], " ");
        let message = message.split_whitespace().collect::<Vec<_>>().join(" ");
        if message.chars().count() > 300 {
            let truncated: String = message.chars().take(300).collect();
            format!("{truncated}...")
        } else {
            message
        }
    }

    fn newest(&self) -> Option<&'a LogRecord> {
        self.records
            .iter()
            .copied()
            .max_by_key(|record| record.timestamp)
    }
}

/// How bad an unrecognised but frequently repeating fault is.
///
/// Frequency is the only signal available here, so it is the only one used.
/// The AI analysis layer gets these findings and can raise the priority once
/// it correlates them with everything else the scan found.
fn cluster_severity(count: usize, level: LogLevel) -> Severity {
    match (count, level) {
        (c, LogLevel::Critical) if c >= CLUSTER_THRESHOLD => Severity::High,
        (c, _) if c >= CLUSTER_THRESHOLD * 8 => Severity::High,
        (c, _) if c >= CLUSTER_THRESHOLD * 3 => Severity::Medium,
        _ => Severity::Low,
    }
}

fn known_fault_finding(pattern: &FaultPattern, cluster: &Cluster<'_>) -> Finding {
    let count = cluster.count();
    let occurrences = if count == 1 {
        "once in the last three days".to_string()
    } else {
        format!("{count} times in the last three days")
    };

    let mut builder = Finding::builder("logs.recent-errors", pattern.id)
        // Grouping by fault class rather than by individual event keeps a
        // machine that crashed forty times from producing forty findings.
        .subject(pattern.id)
        .severity(pattern.severity)
        .category(pattern.category)
        .title(format!("{} ({occurrences})", pattern.title))
        .detail(format!("{} Recorded {occurrences}.", pattern.explanation))
        .evidence("occurrences", count.to_string())
        .evidence("sample_message", cluster.sample())
        .remediation_hint(pattern.hint)
        // These need investigation and a tested fix rather than one known
        // correct action, so they go to the triage queue.
        .triage(Triage::Queue);

    if let Some(newest) = cluster.newest() {
        builder = builder.evidence("source", &newest.source);
        if let Some(id) = &newest.event_id {
            builder = builder.evidence("event_id", id);
        }
        if let Some(timestamp) = newest.timestamp {
            builder = builder.evidence("last_seen", timestamp.to_string());
        }
    }

    builder.build()
}

fn repeated_error_finding(signature: &str, cluster: &Cluster<'_>) -> Finding {
    let count = cluster.count();
    let severity = cluster_severity(count, cluster.worst_level());

    Finding::builder("logs.recent-errors", "logs.repeated-error")
        .subject(signature)
        .severity(severity)
        .category(Category::Logs)
        .title(format!(
            "`{signature}` has logged {count} errors in the last three days"
        ))
        .detail(format!(
            "Something is going wrong repeatedly and being written to the system log. \
             This is not a recognised fault signature, so what it means depends on the \
             component involved -- but {count} occurrences in three days is a pattern \
             rather than a one-off. Most recent message: {}",
            cluster.sample()
        ))
        .evidence("signature", signature)
        .evidence("occurrences", count.to_string())
        .evidence("worst_level", format!("{:?}", cluster.worst_level()))
        .evidence("sample_message", cluster.sample())
        .remediation_hint(
            "Identify which component this source belongs to and whether it changed \
             recently.",
        )
        .triage(Triage::Queue)
        .build()
}

/// Turn a window of log records into findings.
fn analyse(records: &[LogRecord]) -> Vec<Finding> {
    let mut findings = Vec::new();

    // Known signatures first, so a recognised fault is reported by name rather
    // than as an anonymous cluster.
    let mut recognised: Vec<bool> = vec![false; records.len()];
    for pattern in PATTERNS {
        let mut cluster = Cluster {
            records: Vec::new(),
        };
        for (index, record) in records.iter().enumerate() {
            if pattern.matches(record) {
                recognised[index] = true;
                cluster.records.push(record);
            }
        }
        if !cluster.records.is_empty() {
            findings.push(known_fault_finding(pattern, &cluster));
        }
    }

    // Everything else, grouped by source, and reported only when it repeats
    // often enough to be a pattern. BTreeMap keeps the output stable between
    // runs on an unchanged machine.
    let mut clusters: BTreeMap<String, Cluster<'_>> = BTreeMap::new();
    for (index, record) in records.iter().enumerate() {
        if recognised[index] || record.level == LogLevel::Warning {
            continue;
        }
        clusters
            .entry(record.signature())
            .or_insert_with(|| Cluster {
                records: Vec::new(),
            })
            .records
            .push(record);
    }

    for (signature, cluster) in &clusters {
        if cluster.count() >= CLUSTER_THRESHOLD {
            findings.push(repeated_error_finding(signature, cluster));
        }
    }

    findings
}

#[derive(Debug, Default)]
pub struct RecentLogErrorsProbe;

#[async_trait]
impl Probe for RecentLogErrorsProbe {
    fn meta(&self) -> ProbeMeta {
        ProbeMeta {
            id: "logs.recent-errors",
            name: "Recent system log errors",
            description: "Reads the system log for crashes, driver faults, and hardware errors, and \
                 groups repeats together.",
            category: Category::Logs,
            min_tier: ScanTier::Quick,
            platforms: &[PlatformKind::Windows, PlatformKind::Linux],
            requires_tools: &[],
            requires_elevation: false,
        }
    }

    async fn run(&self, ctx: &ProbeContext) -> Result<Vec<Finding>> {
        let records = ctx
            .blocking(|platform| platform.recent_log_errors(QUICK_WINDOW))
            .await?;
        tracing::debug!(records = records.len(), "read system log");
        Ok(analyse(&records))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(source: &str, event_id: Option<&str>, level: LogLevel, message: &str) -> LogRecord {
        LogRecord {
            timestamp: None,
            source: source.to_string(),
            level,
            event_id: event_id.map(str::to_string),
            message: message.to_string(),
        }
    }

    fn ids(findings: &[Finding]) -> Vec<&str> {
        findings.iter().map(|finding| finding.id.as_str()).collect()
    }

    #[test]
    fn no_pattern_matches_everything() {
        // A pattern with every criterion empty would match every log line ever
        // written. Guard the table against that mistake.
        let harmless = record("something", None, LogLevel::Error, "an ordinary message");
        for pattern in PATTERNS {
            assert!(
                !pattern.matches(&harmless),
                "pattern {} matches an unrelated message",
                pattern.id
            );
        }
    }

    #[test]
    fn a_windows_unexpected_shutdown_is_recognised() {
        let records = vec![record(
            "Microsoft-Windows-Kernel-Power",
            Some("41"),
            LogLevel::Critical,
            "reboot",
        )];
        let findings = analyse(&records);
        assert_eq!(ids(&findings), vec!["logs.unexpected-shutdown"]);
        assert_eq!(findings[0].severity, Severity::High);
        assert_eq!(findings[0].triage, Triage::Queue);
    }

    #[test]
    fn an_nvidia_xid_fault_is_recognised_on_linux() {
        let records = vec![record(
            "kernel",
            Some("kernel"),
            LogLevel::Error,
            "NVRM: Xid (PCI:0000:01:00): 79, GPU has fallen off the bus",
        )];
        let findings = analyse(&records);
        assert!(ids(&findings).contains(&"logs.gpu-fault"));
    }

    #[test]
    fn repeats_of_one_fault_produce_one_finding_not_many() {
        let records: Vec<LogRecord> = (0..40)
            .map(|_| record("BugCheck", Some("1001"), LogLevel::Error, "0x0000009f"))
            .collect();
        let findings = analyse(&records);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].title.contains("40 times"));
    }

    #[test]
    fn an_unrecognised_error_needs_to_repeat_before_it_is_reported() {
        let once = vec![record("SomeService", Some("7"), LogLevel::Error, "failed")];
        assert!(
            analyse(&once).is_empty(),
            "a single unknown error is noise, not a finding"
        );

        let many: Vec<LogRecord> = (0..CLUSTER_THRESHOLD)
            .map(|_| record("SomeService", Some("7"), LogLevel::Error, "failed"))
            .collect();
        let findings = analyse(&many);
        assert_eq!(ids(&findings), vec!["logs.repeated-error"]);
    }

    #[test]
    fn warnings_alone_never_become_a_cluster() {
        let warnings: Vec<LogRecord> = (0..200)
            .map(|_| {
                record(
                    "ChattyService",
                    Some("1"),
                    LogLevel::Warning,
                    "just so you know",
                )
            })
            .collect();
        assert!(analyse(&warnings).is_empty());
    }

    #[test]
    fn a_recognised_fault_is_not_also_counted_as_an_anonymous_cluster() {
        let records: Vec<LogRecord> = (0..50)
            .map(|_| {
                record(
                    "WHEA-Logger",
                    Some("18"),
                    LogLevel::Critical,
                    "machine check",
                )
            })
            .collect();
        let findings = analyse(&records);
        assert_eq!(ids(&findings), vec!["logs.hardware-error"]);
    }

    #[test]
    fn severity_rises_with_repetition() {
        assert_eq!(cluster_severity(12, LogLevel::Error), Severity::Low);
        assert_eq!(cluster_severity(40, LogLevel::Error), Severity::Medium);
        assert_eq!(cluster_severity(200, LogLevel::Error), Severity::High);
        assert_eq!(cluster_severity(12, LogLevel::Critical), Severity::High);
    }

    #[test]
    fn long_messages_are_truncated_for_display() {
        let long = "x".repeat(1000);
        let records = [record("Svc", Some("1"), LogLevel::Error, &long)];
        let cluster = Cluster {
            records: records.iter().collect(),
        };
        let sample = cluster.sample();
        assert!(
            sample.chars().count() <= 303,
            "sample was {} chars",
            sample.chars().count()
        );
        assert!(sample.ends_with("..."));
    }
}
