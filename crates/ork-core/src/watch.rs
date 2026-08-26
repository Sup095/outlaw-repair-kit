//! Noticing a problem appearing, rather than being asked to look for one.
//!
//! Everything else in this tool answers a question somebody asked. The watcher
//! is the part that asks it repeatedly, and speaks up only when the answer
//! changes.
//!
//! That last clause is the whole design. A watcher that reports what it finds
//! is a watcher that reports the same eleven things every fifteen minutes, and
//! a person who is told the same eleven things every fifteen minutes stops
//! reading -- including on the morning it is twelve. So this module reports
//! *transitions* and nothing else:
//!
//! * A problem that was not there before.
//! * A problem that got worse.
//! * A problem that went away.
//!
//! Three consequences of that fall out, and each one is a test below.
//!
//! **The first run reports nothing.** A machine that already has six problems
//! did not just develop six problems, and opening with six alerts teaches
//! somebody within a minute that these alerts are noise. The first run records
//! what is already true and says how many things it recorded.
//!
//! **A problem that comes and goes is reported once.** Something flapping
//! between present and absent every few minutes would otherwise produce an
//! endless alternation of "appeared" and "cleared". After a few round trips it
//! is reported as flapping -- which is the actual finding, and is more useful
//! than either half of it -- and then held quiet. Held quiet, never hidden:
//! [`Baseline::muted`] lists everything being held and why, and the front-ends
//! show it.
//!
//! **A check that did not run this time clears nothing.** This is the one that
//! would make the watcher lie. If the system file check cannot run because
//! something else holds the lock, its findings are absent from that round's
//! report -- and absent looks exactly like fixed. Reporting "your damaged
//! system files have been repaired" because a check was skipped would be worse
//! than saying nothing at all, so a finding is only ever cleared by a probe
//! that ran to completion and did not find it. See [`compare`].
//!
//! The watcher never fixes anything, never asks for administrator rights, and
//! runs the Quick tier by default. A check heavy enough to be noticeable is a
//! check that should not be running on a timer behind somebody's work.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Context as _;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::Result;
use crate::finding::{Finding, Severity};
use crate::probe::ProbeStatus;
use crate::scan::{ScanReport, Scanner};
use crate::tier::ScanTier;

/// How often to look, when nothing says otherwise.
pub const DEFAULT_INTERVAL: Duration = Duration::from_secs(15 * 60);

/// The shortest interval that will be honoured.
///
/// Not a limitation -- a floor. A scan takes appreciable work, and running one
/// every few seconds would make the tool itself the heaviest thing on the
/// machine, which is a peculiar way to look after somebody's computer.
pub const MINIMUM_INTERVAL: Duration = Duration::from_secs(60);

/// How many times a problem may come and go before it is called flapping and
/// held quiet.
///
/// Two round trips can be coincidence -- a drive crossing a threshold as a
/// download finishes, a service restarting during an update. Three is a
/// pattern.
pub const FLAP_LIMIT: u32 = 3;

/// What the watcher knows about one problem it has seen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Seen {
    /// The finding's kind, e.g. `storage.volume-nearly-full`.
    pub id: String,
    /// The check that produces this kind of finding.
    ///
    /// Kept because it is what makes clearing safe: a problem is only ever
    /// declared gone by the check that would have found it, and that check
    /// has to be identified by name to know whether it ran.
    pub probe: String,
    pub subject: Option<String>,
    /// The last title it carried, so a cleared problem can be named after it
    /// is gone.
    pub title: String,
    pub severity: Severity,
    /// True while the problem is present. A record stays after it clears, so
    /// that the same problem returning is recognised as a return.
    pub present: bool,
    #[serde(with = "time::serde::rfc3339")]
    pub first_seen: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub last_change: OffsetDateTime,
    /// How many separate times this has appeared. Two or more means it came
    /// back after clearing.
    pub appearances: u32,
}

/// One thing that changed between two looks.
///
/// Not comparable for equality, because a [`Finding`] is not: it carries the
/// moment it was observed, so two reports of the same problem a minute apart
/// are different values. Changes are compared by their [`Change::key`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "change", rename_all = "kebab-case")]
pub enum Change {
    /// Not there last time.
    Appeared { finding: Box<Finding> },
    /// There last time, and worse now.
    Worsened {
        finding: Box<Finding>,
        was: Severity,
    },
    /// There last time, and less bad now. Reported because a problem easing on
    /// its own usually means something else changed, and that is worth
    /// knowing.
    Eased {
        finding: Box<Finding>,
        was: Severity,
    },
    /// Gone, and the check that would have found it did run.
    Cleared {
        id: String,
        subject: Option<String>,
        title: String,
        was: Severity,
    },
    /// Has come and gone often enough to be a pattern rather than an event.
    /// Reported once; further appearances are held quiet.
    Flapping {
        finding: Box<Finding>,
        appearances: u32,
    },
}

impl Change {
    /// The key of the problem this is about.
    pub fn key(&self) -> String {
        match self {
            Change::Appeared { finding }
            | Change::Worsened { finding, .. }
            | Change::Eased { finding, .. }
            | Change::Flapping { finding, .. } => finding.occurrence_key(),
            Change::Cleared { id, subject, .. } => match subject {
                Some(subject) => format!("{id}::{subject}"),
                None => id.clone(),
            },
        }
    }

    /// One line, in the same plain language as everything else.
    pub fn headline(&self) -> String {
        match self {
            Change::Appeared { finding } => format!("new: {}", finding.title),
            Change::Worsened { finding, was } => {
                format!("worse ({was} to {}): {}", finding.severity, finding.title)
            }
            Change::Eased { finding, was } => {
                format!("eased ({was} to {}): {}", finding.severity, finding.title)
            }
            Change::Cleared { title, .. } => format!("cleared: {title}"),
            Change::Flapping {
                finding,
                appearances,
            } => format!(
                "coming and going ({appearances} times), now held quiet: {}",
                finding.title
            ),
        }
    }

    /// How much attention this deserves, for ordering and for deciding whether
    /// to interrupt somebody.
    pub fn severity(&self) -> Severity {
        match self {
            Change::Appeared { finding }
            | Change::Worsened { finding, .. }
            | Change::Eased { finding, .. }
            | Change::Flapping { finding, .. } => finding.severity,
            // Good news, and never a reason to interrupt anybody.
            Change::Cleared { .. } => Severity::Info,
        }
    }

    /// Whether this is worth putting in front of somebody who did not ask.
    pub fn worth_interrupting(&self) -> bool {
        match self {
            Change::Appeared { finding } | Change::Worsened { finding, .. } => {
                finding.severity >= Severity::High
            }
            _ => false,
        }
    }
}

/// Why a problem is being held quiet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Muted {
    pub key: String,
    pub title: String,
    pub reason: String,
    pub appearances: u32,
}

/// What the watcher remembers between looks.
///
/// Small, and written as JSON on purpose: somebody who wants to know what the
/// watcher thinks it knows should be able to open the file and read it, and
/// deleting it should be a complete and obvious reset.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Baseline {
    /// Whether a first look has been recorded. Until it has, nothing is
    /// reported as a change, because nothing is known to have changed.
    #[serde(default)]
    pub established: bool,
    /// Everything ever seen, by occurrence key.
    #[serde(default)]
    pub seen: BTreeMap<String, Seen>,
    /// Problems being held quiet, and why. Listed rather than merely dropped:
    /// a watcher with a private list of things it has decided not to mention
    /// is not a watcher anybody should trust.
    #[serde(default)]
    pub muted: Vec<Muted>,
}

impl Baseline {
    /// Where the watcher keeps what it remembers, beside the configuration.
    pub fn default_path() -> Result<PathBuf> {
        let config = crate::config::Config::default_path()?;
        let dir = config
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        Ok(dir.join("watch-baseline.json"))
    }

    /// Read what is remembered, or start fresh.
    ///
    /// A missing file is a first run. A *corrupt* file is also treated as a
    /// first run rather than as a failure to start: the alternative is a
    /// watcher that refuses to watch because of a file it wrote itself, and
    /// the worst case of starting over is one quiet round.
    pub fn load(path: &Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(text) => match serde_json::from_str(&text) {
                Ok(baseline) => baseline,
                Err(error) => {
                    tracing::warn!(path = %path.display(), %error, "unreadable watch baseline; starting over");
                    Self::default()
                }
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(error) => {
                tracing::warn!(path = %path.display(), %error, "could not read the watch baseline");
                Self::default()
            }
        }
    }

    /// Write what is remembered, atomically, so an interrupted write cannot
    /// leave a half-file that reads as a first run.
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("could not create {}", parent.display()))?;
        }
        let text = serde_json::to_string_pretty(self)
            .context("could not serialise what the watcher remembers")?;
        let temporary = path.with_extension("json.tmp");
        std::fs::write(&temporary, text)
            .with_context(|| format!("could not write {}", temporary.display()))?;
        std::fs::rename(&temporary, path)
            .with_context(|| format!("could not replace {}", path.display()))?;
        Ok(())
    }

    /// How many problems are present right now, according to what is
    /// remembered.
    pub fn present_count(&self) -> usize {
        self.seen.values().filter(|seen| seen.present).count()
    }

    /// Whether a problem is being held quiet. For a front-end that wants to
    /// show why something it can see is not being announced.
    pub fn is_muted(&self, key: &str) -> bool {
        self.muted.iter().any(|muted| muted.key == key)
    }
}

/// What one look produced.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Look {
    #[serde(with = "time::serde::rfc3339")]
    pub at: OffsetDateTime,
    /// Empty on the first look, and empty on any look where nothing changed --
    /// which should be most of them.
    pub changes: Vec<Change>,
    /// True when this look was the one that recorded the starting point.
    pub established_baseline: bool,
    /// How many problems were already present when the starting point was
    /// recorded. Only meaningful alongside `established_baseline`.
    pub recorded: usize,
    /// Probes that did not run to completion this time, by name. Nothing they
    /// would have found was cleared, and this says which ones they were so
    /// that a quiet round is never mistaken for a clean one.
    pub did_not_run: Vec<String>,
}

impl Look {
    pub fn quiet(&self) -> bool {
        self.changes.is_empty() && !self.established_baseline
    }
}

/// Whether a check not running is the ordinary state of affairs rather than a
/// gap worth mentioning.
///
/// A watcher looking at the Quick tier is not missing the Full-tier checks; it
/// was never going to run them, and saying so every quarter of an hour is the
/// kind of noise this module exists to avoid. Same for a check belonging to
/// another operating system, or one turned off deliberately.
///
/// A missing tool or refused rights is a different matter: those are gaps
/// somebody can close, and worth saying so when there is anything being said.
///
/// Note that this affects only what is *mentioned*. It has no bearing on what
/// may be cleared -- only a check that ran to completion can do that, whatever
/// the reason the others did not.
fn expected_not_to_run(reason: &crate::probe::SkipReason) -> bool {
    use crate::probe::SkipReason;
    matches!(
        reason,
        SkipReason::UnsupportedPlatform { .. }
            | SkipReason::AboveTier { .. }
            | SkipReason::DisabledByUser
    )
}

/// Compare a scan against what is remembered, updating what is remembered.
///
/// The rule that matters is in the first few lines: only probes that ran to
/// completion get a say in what is still true. A probe that was skipped,
/// failed, or was cancelled contributes nothing this round -- neither new
/// problems nor the absence of old ones. Absence of evidence from a check that
/// did not run is not evidence of a repair.
pub fn compare(baseline: &mut Baseline, report: &ScanReport) -> Look {
    let at = OffsetDateTime::now_utc();

    let mut ran: BTreeSet<&str> = BTreeSet::new();
    let mut did_not_run: Vec<String> = Vec::new();
    for outcome in &report.outcomes {
        match &outcome.status {
            ProbeStatus::Completed => {
                ran.insert(outcome.probe.as_str());
            }
            ProbeStatus::Skipped(reason) if expected_not_to_run(reason) => {}
            _ => did_not_run.push(outcome.name.clone()),
        }
    }

    let mut current: BTreeMap<String, &Finding> = BTreeMap::new();
    for outcome in &report.outcomes {
        if !matches!(outcome.status, ProbeStatus::Completed) {
            continue;
        }
        for finding in &outcome.findings {
            current.insert(finding.occurrence_key(), finding);
        }
    }

    // The first look establishes the starting point. Everything already wrong
    // with the machine is recorded as the way the machine is, and reported as
    // a count rather than as a wall of alerts about problems that predate the
    // watcher being asked to watch.
    if !baseline.established {
        for (key, finding) in &current {
            baseline.seen.insert(
                key.clone(),
                Seen {
                    id: finding.id.clone(),
                    probe: finding.probe.clone(),
                    subject: finding.subject.clone(),
                    title: finding.title.clone(),
                    severity: finding.severity,
                    present: true,
                    first_seen: at,
                    last_change: at,
                    appearances: 1,
                },
            );
        }
        baseline.established = true;
        return Look {
            at,
            changes: Vec::new(),
            established_baseline: true,
            recorded: current.len(),
            did_not_run,
        };
    }

    let mut changes = Vec::new();
    let mut newly_muted: Vec<Muted> = Vec::new();
    // Taken once, up front, because the loop below holds a mutable borrow of
    // what is remembered and cannot ask it questions while it does.
    let already_muted: BTreeSet<String> = baseline
        .muted
        .iter()
        .map(|entry| entry.key.clone())
        .collect();

    for (key, finding) in &current {
        match baseline.seen.get_mut(key) {
            Some(seen) if seen.present => {
                if finding.severity != seen.severity {
                    let was = seen.severity;
                    if !already_muted.contains(key) {
                        changes.push(if finding.severity > was {
                            Change::Worsened {
                                finding: Box::new((*finding).clone()),
                                was,
                            }
                        } else {
                            Change::Eased {
                                finding: Box::new((*finding).clone()),
                                was,
                            }
                        });
                    }
                    seen.severity = finding.severity;
                    seen.last_change = at;
                }
                seen.title = finding.title.clone();
            }
            Some(seen) => {
                // Back after having cleared.
                seen.present = true;
                seen.severity = finding.severity;
                seen.title = finding.title.clone();
                seen.last_change = at;
                seen.appearances = seen.appearances.saturating_add(1);

                if already_muted.contains(key) {
                    // Already known to flap. Recorded, not announced.
                } else if seen.appearances >= FLAP_LIMIT {
                    changes.push(Change::Flapping {
                        finding: Box::new((*finding).clone()),
                        appearances: seen.appearances,
                    });
                    newly_muted.push(Muted {
                        key: key.clone(),
                        title: finding.title.clone(),
                        reason: format!(
                            "came and went {} times; held quiet so it does not drown out everything else",
                            seen.appearances
                        ),
                        appearances: seen.appearances,
                    });
                } else {
                    changes.push(Change::Appeared {
                        finding: Box::new((*finding).clone()),
                    });
                }
            }
            None => {
                baseline.seen.insert(
                    key.clone(),
                    Seen {
                        id: finding.id.clone(),
                        probe: finding.probe.clone(),
                        subject: finding.subject.clone(),
                        title: finding.title.clone(),
                        severity: finding.severity,
                        present: true,
                        first_seen: at,
                        last_change: at,
                        appearances: 1,
                    },
                );
                changes.push(Change::Appeared {
                    finding: Box::new((*finding).clone()),
                });
            }
        }
    }

    // Anything remembered as present, whose check ran this time, and which
    // that check did not report, has genuinely gone.
    let muted_keys: BTreeSet<String> = already_muted
        .iter()
        .cloned()
        .chain(newly_muted.iter().map(|entry| entry.key.clone()))
        .collect();

    for (key, seen) in baseline.seen.iter_mut() {
        if !seen.present || current.contains_key(key) || !ran.contains(seen.probe.as_str()) {
            continue;
        }
        seen.present = false;
        seen.last_change = at;
        if !muted_keys.contains(key) {
            changes.push(Change::Cleared {
                id: seen.id.clone(),
                subject: seen.subject.clone(),
                title: seen.title.clone(),
                was: seen.severity,
            });
        }
    }

    baseline.muted.extend(newly_muted);

    // Worst first, so a glance at the top of the list is a glance at the worst
    // thing that happened.
    changes.sort_by_key(|change| std::cmp::Reverse(change.severity()));

    Look {
        at,
        changes,
        established_baseline: false,
        recorded: 0,
        did_not_run,
    }
}

/// Progress from a running watcher, for a live UI or a terminal.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "kebab-case")]
pub enum WatchEvent {
    /// The watcher started. `known` is how many problems it already remembers
    /// being present.
    Started { interval_secs: u64, known: usize },
    /// A look is beginning.
    Looking,
    /// A look finished. Most of these carry nothing, which is the point.
    Looked { look: Box<Look> },
    /// A look could not be taken. The watcher keeps watching -- one failed
    /// round is not a reason to stop looking at somebody's computer.
    Trouble { error: String },
    /// The watcher was asked to stop.
    Stopped,
}

/// Looks repeatedly, and reports what changed.
pub struct Watcher {
    tier: ScanTier,
    interval: Duration,
    baseline_path: Option<PathBuf>,
    cancel: CancellationToken,
    events: Option<mpsc::UnboundedSender<WatchEvent>>,
}

impl Watcher {
    pub fn new() -> Self {
        Self {
            tier: ScanTier::Quick,
            interval: DEFAULT_INTERVAL,
            baseline_path: None,
            cancel: CancellationToken::new(),
            events: None,
        }
    }

    /// Which tier to run each time. Quick by default, and deliberately: a
    /// check heavy enough to be felt is a check that should be asked for, not
    /// one that arrives behind somebody's work every quarter of an hour.
    pub fn tier(mut self, tier: ScanTier) -> Self {
        self.tier = tier;
        self
    }

    /// How long to wait between looks. Anything below [`MINIMUM_INTERVAL`] is
    /// raised to it.
    pub fn interval(mut self, interval: Duration) -> Self {
        self.interval = interval.max(MINIMUM_INTERVAL);
        self
    }

    pub fn baseline_path(mut self, path: PathBuf) -> Self {
        self.baseline_path = Some(path);
        self
    }

    pub fn with_events(mut self, sender: mpsc::UnboundedSender<WatchEvent>) -> Self {
        self.events = Some(sender);
        self
    }

    /// The token that stops the watcher. As everywhere else in this tool,
    /// stopping is the user's decision and nothing else's.
    pub fn cancel_token(&self) -> CancellationToken {
        self.cancel.clone()
    }

    fn emit(&self, event: WatchEvent) {
        if let Some(sender) = &self.events {
            let _ = sender.send(event);
        }
    }

    fn path(&self) -> Result<PathBuf> {
        match &self.baseline_path {
            Some(path) => Ok(path.clone()),
            None => Baseline::default_path(),
        }
    }

    /// Take one look, record what was learned, and return.
    ///
    /// For a scheduled task, where the operating system's own scheduler
    /// decides when to look and this decides what changed since last time. It
    /// reads and writes the same memory the running watcher does, so the two
    /// are interchangeable and a machine can be moved between them without
    /// losing its history or getting a fresh wall of alerts.
    pub async fn look_once(&self) -> Result<Look> {
        let path = self.path()?;
        let mut baseline = Baseline::load(&path);
        let look = self.look(&mut baseline).await?;
        baseline.save(&path)?;
        Ok(look)
    }

    /// Watch until cancelled.
    pub async fn run(&self) -> Result<()> {
        let path = self.path()?;
        let mut baseline = Baseline::load(&path);

        self.emit(WatchEvent::Started {
            interval_secs: self.interval.as_secs(),
            known: baseline.present_count(),
        });

        loop {
            if self.cancel.is_cancelled() {
                break;
            }

            self.emit(WatchEvent::Looking);

            match self.look(&mut baseline).await {
                Ok(look) => {
                    if let Err(error) = baseline.save(&path) {
                        // Failing to remember is not failing to watch. The
                        // consequence is that a change may be reported twice,
                        // which is a great deal better than stopping.
                        tracing::warn!(%error, "could not save what the watcher remembers");
                    }
                    self.emit(WatchEvent::Looked {
                        look: Box::new(look),
                    });
                }
                Err(error) => self.emit(WatchEvent::Trouble {
                    error: format!("{error:#}"),
                }),
            }

            tokio::select! {
                _ = self.cancel.cancelled() => break,
                _ = tokio::time::sleep(self.interval) => {}
            }
        }

        self.emit(WatchEvent::Stopped);
        Ok(())
    }

    async fn look(&self, baseline: &mut Baseline) -> Result<Look> {
        let scanner = Scanner::new()?;
        // A scan started by the watcher stops when the watcher does, so
        // stopping is immediate rather than "after this round".
        let scan_cancel = scanner.cancel_token();
        let watcher_cancel = self.cancel.clone();
        let relay = tokio::spawn(async move {
            watcher_cancel.cancelled().await;
            scan_cancel.cancel();
        });

        let report = scanner.run(self.tier).await;
        relay.abort();
        let report = report?;

        // A cancelled scan is a partial view of the machine, and comparing a
        // partial view against a complete one manufactures changes that did
        // not happen.
        if report.cancelled {
            return Ok(Look {
                at: OffsetDateTime::now_utc(),
                changes: Vec::new(),
                established_baseline: false,
                recorded: 0,
                did_not_run: vec!["the look was stopped before it finished".to_string()],
            });
        }

        Ok(compare(baseline, &report))
    }
}

impl Default for Watcher {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::finding::{Category, Triage};
    use crate::platform::HostInfo;
    use crate::probe::ProbeOutcome;

    fn finding(probe: &str, id: &str, subject: Option<&str>, severity: Severity) -> Finding {
        let mut builder = Finding::builder(probe, id)
            .severity(severity)
            .category(Category::Storage)
            .title(format!("{id} on {}", subject.unwrap_or("this machine")))
            .detail("detail")
            .triage(Triage::None);
        if let Some(subject) = subject {
            builder = builder.subject(subject);
        }
        builder.build()
    }

    fn host() -> HostInfo {
        HostInfo {
            hostname: "test".into(),
            os_name: "test".into(),
            os_version: "0".into(),
            kernel_version: "0".into(),
            arch: "x86_64".into(),
            cpu_brand: "test".into(),
            physical_cores: Some(1),
            logical_cores: 1,
            total_memory_bytes: 1,
        }
    }

    /// A report where `probes` each ran to completion with the findings given.
    fn report(probes: &[(&str, Vec<Finding>)]) -> ScanReport {
        ScanReport {
            tier: ScanTier::Quick,
            host: host(),
            started_at: OffsetDateTime::now_utc(),
            duration: Duration::ZERO,
            cancelled: false,
            elevated: false,
            outcomes: probes
                .iter()
                .map(|(probe, findings)| ProbeOutcome {
                    probe: (*probe).to_string(),
                    name: format!("the {probe} check"),
                    status: ProbeStatus::Completed,
                    skipped_because: None,
                    findings: findings.clone(),
                    duration: Duration::ZERO,
                })
                .collect(),
        }
    }

    #[test]
    fn the_first_look_reports_nothing_and_says_what_it_recorded() {
        // A machine with six existing problems did not just develop six
        // problems. Opening with six alerts is how somebody learns, within a
        // minute of turning this on, that the alerts are not worth reading.
        let mut baseline = Baseline::default();
        let scan = report(&[(
            "storage",
            vec![
                finding("storage", "storage.full", Some("C:"), Severity::High),
                finding("storage", "storage.full", Some("D:"), Severity::Medium),
            ],
        )]);

        let look = compare(&mut baseline, &scan);

        assert!(look.established_baseline);
        assert_eq!(look.recorded, 2);
        assert!(look.changes.is_empty(), "{:?}", look.changes);
        assert_eq!(baseline.present_count(), 2);
    }

    #[test]
    fn an_unchanged_machine_produces_a_silent_look() {
        let mut baseline = Baseline::default();
        let scan = report(&[(
            "storage",
            vec![finding(
                "storage",
                "storage.full",
                Some("C:"),
                Severity::High,
            )],
        )]);
        compare(&mut baseline, &scan);

        let look = compare(&mut baseline, &scan);

        assert!(look.quiet(), "{:?}", look.changes);
    }

    #[test]
    fn a_new_problem_is_reported_once_and_then_not_again() {
        let mut baseline = Baseline::default();
        compare(&mut baseline, &report(&[("storage", vec![])]));

        let appeared = finding("storage", "storage.full", Some("C:"), Severity::High);
        let scan = report(&[("storage", vec![appeared])]);

        let first = compare(&mut baseline, &scan);
        assert_eq!(first.changes.len(), 1);
        assert!(matches!(first.changes[0], Change::Appeared { .. }));

        // The same problem, still there. Saying so again buys nobody anything.
        let second = compare(&mut baseline, &scan);
        assert!(second.quiet(), "{:?}", second.changes);
    }

    #[test]
    fn getting_worse_and_getting_better_are_both_reported() {
        let mut baseline = Baseline::default();
        compare(
            &mut baseline,
            &report(&[(
                "storage",
                vec![finding(
                    "storage",
                    "storage.full",
                    Some("C:"),
                    Severity::Low,
                )],
            )]),
        );

        let worse = compare(
            &mut baseline,
            &report(&[(
                "storage",
                vec![finding(
                    "storage",
                    "storage.full",
                    Some("C:"),
                    Severity::Critical,
                )],
            )]),
        );
        assert!(
            matches!(&worse.changes[..], [Change::Worsened { was, .. }] if *was == Severity::Low),
            "{:?}",
            worse.changes
        );

        let better = compare(
            &mut baseline,
            &report(&[(
                "storage",
                vec![finding(
                    "storage",
                    "storage.full",
                    Some("C:"),
                    Severity::Low,
                )],
            )]),
        );
        assert!(
            matches!(&better.changes[..], [Change::Eased { was, .. }] if *was == Severity::Critical),
            "{:?}",
            better.changes
        );
    }

    #[test]
    fn a_problem_that_goes_away_is_reported_as_cleared() {
        let mut baseline = Baseline::default();
        compare(
            &mut baseline,
            &report(&[(
                "storage",
                vec![finding(
                    "storage",
                    "storage.full",
                    Some("C:"),
                    Severity::High,
                )],
            )]),
        );

        let look = compare(&mut baseline, &report(&[("storage", vec![])]));

        assert!(
            matches!(&look.changes[..], [Change::Cleared { subject, was, .. }]
                if subject.as_deref() == Some("C:") && *was == Severity::High),
            "{:?}",
            look.changes
        );
        assert_eq!(baseline.present_count(), 0);
    }

    #[test]
    fn a_check_that_did_not_run_clears_nothing() {
        // The one that would make the watcher lie. The system file check
        // cannot run, so its findings are absent from this round -- and absent
        // looks exactly like fixed. "Your damaged system files have been
        // repaired" because a check was skipped is worse than silence.
        let mut baseline = Baseline::default();
        compare(
            &mut baseline,
            &report(&[(
                "system-files",
                vec![finding(
                    "system-files",
                    "system-files.damaged",
                    None,
                    Severity::Critical,
                )],
            )]),
        );

        let mut broken = report(&[("system-files", vec![])]);
        broken.outcomes[0].status = ProbeStatus::Failed {
            error: "another process holds the lock".into(),
        };

        let look = compare(&mut baseline, &broken);

        assert!(look.changes.is_empty(), "{:?}", look.changes);
        assert!(
            baseline.seen.values().all(|seen| seen.present),
            "a skipped check was allowed to clear a finding"
        );
        assert_eq!(look.did_not_run, vec!["the system-files check".to_string()]);
    }

    #[test]
    fn a_check_that_was_never_going_to_run_is_not_reported_as_a_gap() {
        // A watcher on the Quick tier is not missing the Full-tier checks. It
        // was never going to run them, and saying so every quarter of an hour
        // is exactly the noise this module exists to avoid. A missing tool is
        // a different matter -- that is a gap somebody can close.
        let mut baseline = Baseline::default();
        let mut scan = report(&[
            ("storage", vec![]),
            ("disks", vec![]),
            ("system-files", vec![]),
            ("smart", vec![]),
        ]);
        scan.outcomes[1].status = ProbeStatus::Skipped(crate::probe::SkipReason::AboveTier {
            min_tier: ScanTier::Full,
        });
        scan.outcomes[2].status =
            ProbeStatus::Skipped(crate::probe::SkipReason::UnsupportedPlatform {
                platform: crate::platform::PlatformKind::Linux,
            });
        scan.outcomes[3].status = ProbeStatus::Skipped(crate::probe::SkipReason::MissingTool {
            tool: "smartctl".into(),
        });

        let look = compare(&mut baseline, &scan);

        assert_eq!(
            look.did_not_run,
            vec!["the smart check".to_string()],
            "a gap somebody can close is the only one worth mentioning"
        );
    }

    #[test]
    fn a_check_above_the_tier_still_clears_nothing() {
        // The quieting above must not become permission to clear. A Full-tier
        // check not running in a Quick round is expected, and expected is not
        // the same as "reported nothing, so it must be fixed".
        let mut baseline = Baseline::default();
        compare(
            &mut baseline,
            &report(&[(
                "disks",
                vec![finding(
                    "disks",
                    "disks.failing",
                    Some("C:"),
                    Severity::Critical,
                )],
            )]),
        );

        let mut quick = report(&[("disks", vec![])]);
        quick.outcomes[0].status = ProbeStatus::Skipped(crate::probe::SkipReason::AboveTier {
            min_tier: ScanTier::Full,
        });

        let look = compare(&mut baseline, &quick);

        assert!(look.changes.is_empty(), "{:?}", look.changes);
        assert!(look.did_not_run.is_empty(), "{:?}", look.did_not_run);
        assert!(
            baseline.seen.values().all(|seen| seen.present),
            "a failing drive was declared fixed by not being looked at"
        );
    }

    #[test]
    fn something_that_comes_and_goes_is_reported_once_and_then_held_quiet() {
        let mut baseline = Baseline::default();
        let present = report(&[(
            "storage",
            vec![finding(
                "storage",
                "storage.full",
                Some("C:"),
                Severity::High,
            )],
        )]);
        let absent = report(&[("storage", vec![])]);

        compare(&mut baseline, &absent); // establish
        let mut announcements = 0;

        for _ in 0..6 {
            for look in [
                compare(&mut baseline, &present),
                compare(&mut baseline, &absent),
            ] {
                announcements += look.changes.len();
            }
        }

        // Appeared, cleared, appeared, cleared, flapping -- and then nothing,
        // however long it goes on. Twelve round trips, five announcements.
        // Without the hold this is twelve, which is the whole problem.
        assert_eq!(
            announcements, 5,
            "a flapping problem produced {announcements} announcements over twelve round trips"
        );
        assert_eq!(baseline.muted.len(), 1);
        assert!(
            baseline.muted[0].reason.contains("came and went"),
            "{:?}",
            baseline.muted[0]
        );
    }

    #[test]
    fn what_is_held_quiet_is_listed_rather_than_hidden() {
        // A watcher with a private list of things it has decided not to
        // mention is not a watcher anybody should trust.
        let mut baseline = Baseline::default();
        let present = report(&[(
            "storage",
            vec![finding(
                "storage",
                "storage.full",
                Some("C:"),
                Severity::High,
            )],
        )]);
        let absent = report(&[("storage", vec![])]);
        compare(&mut baseline, &absent);
        for _ in 0..4 {
            compare(&mut baseline, &present);
            compare(&mut baseline, &absent);
        }

        assert_eq!(baseline.muted.len(), 1);
        assert_eq!(baseline.muted[0].key, "storage.full::C:");
        assert!(baseline.muted[0].appearances >= FLAP_LIMIT);
    }

    #[test]
    fn two_drives_with_the_same_problem_are_two_changes() {
        let mut baseline = Baseline::default();
        compare(&mut baseline, &report(&[("storage", vec![])]));

        let look = compare(
            &mut baseline,
            &report(&[(
                "storage",
                vec![
                    finding("storage", "storage.full", Some("C:"), Severity::High),
                    finding("storage", "storage.full", Some("D:"), Severity::High),
                ],
            )]),
        );

        assert_eq!(look.changes.len(), 2, "{:?}", look.changes);
    }

    #[test]
    fn the_worst_change_is_first() {
        let mut baseline = Baseline::default();
        compare(&mut baseline, &report(&[("storage", vec![])]));

        let look = compare(
            &mut baseline,
            &report(&[(
                "storage",
                vec![
                    finding("storage", "storage.a", Some("one"), Severity::Low),
                    finding("storage", "storage.b", Some("two"), Severity::Critical),
                ],
            )]),
        );

        assert_eq!(look.changes[0].severity(), Severity::Critical);
    }

    #[test]
    fn only_a_serious_new_problem_is_worth_interrupting_somebody_for() {
        let mut baseline = Baseline::default();
        compare(&mut baseline, &report(&[("storage", vec![])]));

        let look = compare(
            &mut baseline,
            &report(&[(
                "storage",
                vec![
                    finding("storage", "storage.a", Some("one"), Severity::Low),
                    finding("storage", "storage.b", Some("two"), Severity::Critical),
                ],
            )]),
        );

        assert!(look.changes[0].worth_interrupting());
        assert!(!look.changes[1].worth_interrupting());

        // Good news never interrupts anybody.
        let cleared = compare(&mut baseline, &report(&[("storage", vec![])]));
        assert!(
            cleared
                .changes
                .iter()
                .all(|change| !change.worth_interrupting())
        );
    }

    #[test]
    fn an_interval_below_the_floor_is_raised_to_it() {
        let watcher = Watcher::new().interval(Duration::from_secs(1));
        assert_eq!(watcher.interval, MINIMUM_INTERVAL);
        let watcher = Watcher::new().interval(Duration::from_secs(3600));
        assert_eq!(watcher.interval, Duration::from_secs(3600));
    }

    #[test]
    fn what_is_remembered_survives_being_written_and_read_back() {
        let dir = std::env::temp_dir().join("ork-watch-baseline-test");
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("watch-baseline.json");

        let mut baseline = Baseline::default();
        compare(
            &mut baseline,
            &report(&[(
                "storage",
                vec![finding(
                    "storage",
                    "storage.full",
                    Some("C:"),
                    Severity::High,
                )],
            )]),
        );
        baseline.save(&path).unwrap();

        let read_back = Baseline::load(&path);
        assert_eq!(read_back, baseline);
        assert!(
            read_back.established,
            "a saved baseline read back as a first run"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_unreadable_memory_starts_over_rather_than_refusing_to_watch() {
        let dir = std::env::temp_dir().join("ork-watch-corrupt-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("watch-baseline.json");
        std::fs::write(&path, "this is not JSON").unwrap();

        let baseline = Baseline::load(&path);

        assert_eq!(baseline, Baseline::default());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
