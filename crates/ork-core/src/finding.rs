use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

/// How bad a finding is, used to order what the user sees and to decide what
/// the fix layer works on first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// Worth knowing, not a problem.
    Info,
    /// A minor annoyance or an early warning.
    Low,
    /// Should be addressed, but nothing is failing yet.
    Medium,
    /// Something is failing, or is about to.
    High,
    /// Data loss, a security compromise, or an unusable system.
    Critical,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Info => "info",
            Severity::Low => "low",
            Severity::Medium => "medium",
            Severity::High => "high",
            Severity::Critical => "critical",
        }
    }
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The subsystem a finding belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Category {
    Storage,
    Memory,
    Cpu,
    Gpu,
    Drivers,
    Packages,
    Logs,
    Malware,
    Application,
    Network,
    Configuration,
    Performance,
}

impl Category {
    pub fn as_str(self) -> &'static str {
        match self {
            Category::Storage => "storage",
            Category::Memory => "memory",
            Category::Cpu => "cpu",
            Category::Gpu => "gpu",
            Category::Drivers => "drivers",
            Category::Packages => "packages",
            Category::Logs => "logs",
            Category::Malware => "malware",
            Category::Application => "application",
            Category::Network => "network",
            Category::Configuration => "configuration",
            Category::Performance => "performance",
        }
    }
}

impl std::fmt::Display for Category {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// How the fix layer should handle this finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Triage {
    /// Nothing to fix -- purely informational.
    None,
    /// Simple and deterministic: there is one known correct action, and it can
    /// be applied during the scan without blocking it.
    Inline,
    /// Complex or ambiguous: goes on the triage queue with full context, to be
    /// worked one candidate fix at a time after the scan completes.
    Queue,
}

/// A single piece of supporting data for a finding.
///
/// Evidence is what makes a finding auditable and what the AI analysis layer
/// actually reasons over -- it never gets raw system access, only this.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Evidence {
    /// Machine-readable key, e.g. `free_bytes` or `exit_code`.
    pub label: String,
    /// The observed value, rendered as text.
    pub value: String,
}

impl Evidence {
    pub fn new(label: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
        }
    }
}

/// One thing a probe noticed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    /// Stable slug identifying the *kind* of problem, e.g.
    /// `storage.volume-nearly-full`. Runbook entries and learned symptom-to-fix
    /// mappings key off this, so it must not change casually.
    pub id: String,
    /// The probe that produced this finding.
    pub probe: String,
    /// What this finding is about -- a drive letter, a package name, an
    /// application. Combined with `id`, this identifies a specific occurrence.
    pub subject: Option<String>,
    pub severity: Severity,
    pub category: Category,
    /// One line, in plain language, that a non-expert can act on.
    pub title: String,
    /// The fuller plain-language explanation.
    pub detail: String,
    pub evidence: Vec<Evidence>,
    /// A short note on what would likely fix this. Not a command, and not a
    /// promise -- the fix layer decides what actually gets attempted.
    pub remediation_hint: Option<String>,
    pub triage: Triage,
    #[serde(with = "time::serde::rfc3339")]
    pub observed_at: OffsetDateTime,
}

impl Finding {
    /// Start building a finding. `id` should be a stable dotted slug.
    pub fn builder(probe: impl Into<String>, id: impl Into<String>) -> FindingBuilder {
        FindingBuilder {
            finding: Finding {
                id: id.into(),
                probe: probe.into(),
                subject: None,
                severity: Severity::Info,
                category: Category::Configuration,
                title: String::new(),
                detail: String::new(),
                evidence: Vec::new(),
                remediation_hint: None,
                triage: Triage::None,
                observed_at: OffsetDateTime::now_utc(),
            },
        }
    }

    /// A stable key for "this exact problem on this exact thing", used to
    /// deduplicate across scans and to look up what worked last time.
    pub fn occurrence_key(&self) -> String {
        match &self.subject {
            Some(subject) => format!("{}::{subject}", self.id),
            None => self.id.clone(),
        }
    }
}

/// Builder for [`Finding`], so probes read as a description of the problem
/// rather than a pile of struct fields.
#[derive(Debug, Clone)]
pub struct FindingBuilder {
    finding: Finding,
}

impl FindingBuilder {
    pub fn subject(mut self, subject: impl Into<String>) -> Self {
        self.finding.subject = Some(subject.into());
        self
    }

    pub fn severity(mut self, severity: Severity) -> Self {
        self.finding.severity = severity;
        self
    }

    pub fn category(mut self, category: Category) -> Self {
        self.finding.category = category;
        self
    }

    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.finding.title = title.into();
        self
    }

    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        self.finding.detail = detail.into();
        self
    }

    pub fn evidence(mut self, label: impl Into<String>, value: impl Into<String>) -> Self {
        self.finding.evidence.push(Evidence::new(label, value));
        self
    }

    pub fn remediation_hint(mut self, hint: impl Into<String>) -> Self {
        self.finding.remediation_hint = Some(hint.into());
        self
    }

    pub fn triage(mut self, triage: Triage) -> Self {
        self.finding.triage = triage;
        self
    }

    pub fn build(self) -> Finding {
        debug_assert!(
            !self.finding.title.is_empty(),
            "finding {} has no title",
            self.finding.id
        );
        self.finding
    }
}
