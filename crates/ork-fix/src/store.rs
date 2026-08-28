//! Where the triage queue, the attempt history, and the audit log live.
//!
//! SQLite, in a single file beside the configuration. Three things need
//! storing and they have genuinely different shapes:
//!
//! * The **queue** is mutable state -- items move from pending to resolved.
//! * The **attempt history** is append-only, and is what lets the tool answer
//!   "this happened before; what worked?" so a repeat problem is fixed on the
//!   first try instead of by working down the list again.
//! * The **audit log** is append-only and never pruned. It is the record of
//!   everything the tool checked, found, attempted, and changed.
//!
//! The database is per-machine by construction, which is what "machine-specific
//! history" means here: a fix that worked on this computer is remembered for
//! this computer.

use std::path::Path;

use anyhow::Context;
use ork_core::Finding;
use ork_core::finding::Severity;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::Result;
use crate::action::FixAction;

/// Where an item has got to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ItemState {
    /// Waiting to be worked.
    Pending,
    /// Fixed, and confirmed fixed by a test.
    Resolved,
    /// Every candidate was tried and none worked. Handed back to the user with
    /// the full record rather than guessed at further.
    Exhausted,
    /// The user chose not to pursue it.
    Dismissed,
}

impl ItemState {
    pub fn as_str(self) -> &'static str {
        match self {
            ItemState::Pending => "pending",
            ItemState::Resolved => "resolved",
            ItemState::Exhausted => "exhausted",
            ItemState::Dismissed => "dismissed",
        }
    }

    fn parse(text: &str) -> Self {
        match text {
            "resolved" => ItemState::Resolved,
            "exhausted" => ItemState::Exhausted,
            "dismissed" => ItemState::Dismissed,
            _ => ItemState::Pending,
        }
    }
}

/// How an attempt turned out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AttemptOutcome {
    /// Applied, and the test afterwards passed.
    Succeeded,
    /// Applied, the test still failed, and the change was rolled back.
    RolledBack,
    /// Could not be applied at all.
    Failed,
    /// Rejected by the safety rules before anything happened.
    Refused,
    /// Needs a person; the tool described it and stopped.
    NeedsAPerson,
}

impl AttemptOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            AttemptOutcome::Succeeded => "succeeded",
            AttemptOutcome::RolledBack => "rolled-back",
            AttemptOutcome::Failed => "failed",
            AttemptOutcome::Refused => "refused",
            AttemptOutcome::NeedsAPerson => "needs-a-person",
        }
    }

    fn parse(text: &str) -> Self {
        match text {
            "succeeded" => AttemptOutcome::Succeeded,
            "rolled-back" => AttemptOutcome::RolledBack,
            "refused" => AttemptOutcome::Refused,
            "needs-a-person" => AttemptOutcome::NeedsAPerson,
            _ => AttemptOutcome::Failed,
        }
    }
}

/// One problem waiting to be worked.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriageItem {
    pub id: i64,
    /// Identifies this exact problem on this exact thing, so the same problem
    /// recurring is recognised as the same problem.
    pub occurrence_key: String,
    pub finding_id: String,
    pub subject: Option<String>,
    pub severity: Severity,
    pub title: String,
    /// The full finding, as captured when it was queued.
    pub finding: Finding,
    pub state: ItemState,
    pub attempts: usize,
    /// When this problem was first put on the queue.
    pub first_seen: String,
    /// When a scan last actually observed it.
    ///
    /// The same as `first_seen` until a second scan finds it again. Kept apart
    /// from the state's own timestamp because "still true" and "somebody
    /// changed its state" are different events, and it is the first one a
    /// person needs when deciding whether to act on a queued problem.
    pub last_seen: String,
    /// The two above, as one sentence for a person to read.
    ///
    /// Built here rather than by each front-end for the reason the audit log
    /// carries its own `readable`: the window and the command line formatted
    /// the same timestamp two different ways once already, badly in both
    /// cases. A queue item shown in two places should say the same thing in
    /// both.
    pub seen: String,
}

/// When a queued problem was last observed, in words.
///
/// Said on every item rather than only on old ones, because there is no
/// threshold at which a problem becomes stale and inventing one would put the
/// tool's guess where the person's knowledge belongs. A queue is a record of
/// what scans have found, and a record that states everything in the present
/// tense is offering to fix things the machine may have stopped having.
fn seen_line(first_seen: &str, last_seen: &str) -> String {
    let last = readable_time(last_seen);
    if first_seen == last_seen {
        format!("seen once, {last}")
    } else {
        format!("first seen {}, last seen {last}", readable_time(first_seen))
    }
}

/// The most audit lines any one request will return.
///
/// Not a cap on what is kept -- the audit table is never pruned, because it is
/// the record of everything the tool did. This is a cap on how much of it is
/// read into memory and drawn at once.
pub const MOST_AUDIT_LINES: usize = 500;

/// One thing that was tried.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttemptRecord {
    pub occurrence_key: String,
    pub summary: String,
    pub outcome: AttemptOutcome,
    pub detail: Option<String>,
    pub at: String,
}

/// One line of the audit log, ready to be shown to somebody.
///
/// `at` stays exactly as it was written -- RFC 3339, in UTC, sortable, and
/// the same string on every machine that reads this database. `readable` is
/// the one a person is meant to look at. Both are here because both front-ends
/// were formatting the raw one themselves, which meant neither was formatting
/// it: the log showed `2026-08-25T23:43:30.4183399Z`, seven digits of fraction
/// and all.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditLine {
    pub at: String,
    /// The same instant, written for a person, and labelled with which clock
    /// it is on. Local time where the machine will tell us, UTC where it will
    /// not -- and never one silently presented as the other.
    pub readable: String,
    pub kind: String,
    pub message: String,
}

/// Turn a stored timestamp into something worth reading.
///
/// Re-exported rather than defined here. It lives in [`ork_core::util`]
/// because the watcher needs exactly the same thing, and a second copy is a
/// second chance for one of them to go back to printing seven decimal places
/// of a UTC instant at somebody -- which is what this was written to stop.
pub use ork_core::util::readable_time;

/// The on-disk store.
pub struct FixStore {
    connection: Connection,
}

impl FixStore {
    /// Open, creating the file and schema if needed.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("could not create {}", parent.display()))?;
        }
        let connection =
            Connection::open(path).with_context(|| format!("could not open {}", path.display()))?;
        let store = Self { connection };
        store.migrate()?;
        Ok(store)
    }

    /// An in-memory store, for tests.
    pub fn in_memory() -> Result<Self> {
        let store = Self {
            connection: Connection::open_in_memory()?,
        };
        store.migrate()?;
        Ok(store)
    }

    fn migrate(&self) -> Result<()> {
        self.connection.execute_batch(
            "
            PRAGMA journal_mode = WAL;
            PRAGMA foreign_keys = ON;

            CREATE TABLE IF NOT EXISTS queue (
                id              INTEGER PRIMARY KEY,
                occurrence_key  TEXT NOT NULL UNIQUE,
                finding_id      TEXT NOT NULL,
                subject         TEXT,
                severity        TEXT NOT NULL,
                title           TEXT NOT NULL,
                finding_json    TEXT NOT NULL,
                state           TEXT NOT NULL,
                created_at      TEXT NOT NULL,
                updated_at      TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS attempts (
                id              INTEGER PRIMARY KEY,
                occurrence_key  TEXT NOT NULL,
                finding_id      TEXT NOT NULL,
                action_json     TEXT NOT NULL,
                summary         TEXT NOT NULL,
                outcome         TEXT NOT NULL,
                detail          TEXT,
                snapshot_id     TEXT,
                at              TEXT NOT NULL
            );

            -- Never pruned. This is the record of everything the tool did.
            CREATE TABLE IF NOT EXISTS audit (
                id       INTEGER PRIMARY KEY,
                at       TEXT NOT NULL,
                kind     TEXT NOT NULL,
                message  TEXT NOT NULL,
                data     TEXT
            );

            CREATE INDEX IF NOT EXISTS attempts_by_key ON attempts(occurrence_key);
            CREATE INDEX IF NOT EXISTS attempts_by_finding ON attempts(finding_id);
            ",
        )?;

        // When the problem was last actually observed, as opposed to when the
        // row was first written or its state last changed. Added separately
        // because databases from earlier versions already exist and a
        // `CREATE TABLE IF NOT EXISTS` does nothing to those.
        //
        // Backfilled from `created_at` rather than left null or set to now:
        // the first sighting is the only sighting anybody recorded, and
        // stamping old rows with today's date would make every stale item
        // look like it had just been seen, which is the exact claim this
        // column exists to stop the tool making.
        if self.add_column_if_missing("queue", "last_seen_at", "TEXT")? {
            self.connection
                .execute("UPDATE queue SET last_seen_at = created_at", [])?;
        }
        Ok(())
    }

    /// Add a column unless it is already there. Answers whether it added one.
    fn add_column_if_missing(&self, table: &str, column: &str, decl: &str) -> Result<bool> {
        let mut statement = self
            .connection
            .prepare(&format!("PRAGMA table_info({table})"))?;
        let present = statement
            .query_map([], |row| row.get::<_, String>("name"))?
            .collect::<rusqlite::Result<Vec<_>>>()?
            .iter()
            .any(|name| name == column);
        if present {
            return Ok(false);
        }
        self.connection.execute(
            &format!("ALTER TABLE {table} ADD COLUMN {column} {decl}"),
            [],
        )?;
        Ok(true)
    }

    fn now() -> String {
        time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default()
    }

    /// Put a finding on the queue, or leave the existing entry alone.
    ///
    /// The same problem found by two scans is one queue item, not two. An item
    /// already resolved or dismissed is not resurrected by a fresh scan seeing
    /// the symptom again mid-fix.
    pub fn enqueue(&self, finding: &Finding) -> Result<bool> {
        let key = finding.occurrence_key();
        let existing: Option<String> = self
            .connection
            .query_row(
                "SELECT state FROM queue WHERE occurrence_key = ?1",
                params![key],
                |row| row.get(0),
            )
            .optional()?;

        let now = Self::now();

        if existing.is_some() {
            // Not a new item, but it *was* just seen, and that is the whole
            // difference between a problem the machine still has and one it
            // had a fortnight ago. Without this the queue states a stale
            // finding in the present tense and `outlaw fix` offers to act on
            // it.
            self.connection.execute(
                "UPDATE queue SET last_seen_at = ?1 WHERE occurrence_key = ?2",
                params![now, key],
            )?;
            return Ok(false);
        }

        self.connection.execute(
            "INSERT INTO queue
                (occurrence_key, finding_id, subject, severity, title, finding_json,
                 state, created_at, updated_at, last_seen_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8, ?8)",
            params![
                key,
                finding.id,
                finding.subject,
                finding.severity.as_str(),
                finding.title,
                serde_json::to_string(finding)?,
                ItemState::Pending.as_str(),
                now,
            ],
        )?;
        self.audit(
            "queued",
            // Not "queued `{title}`": the kind column beside this already
            // says `queued`, and finding titles carry backticks of their own,
            // so wrapping one produced ``` ``node.exe` has been ... ` ```.
            &finding.title,
            None,
        )?;
        Ok(true)
    }

    fn row_to_item(row: &rusqlite::Row<'_>) -> rusqlite::Result<TriageItem> {
        let finding_json: String = row.get("finding_json")?;
        let first_seen: String = row.get("created_at")?;
        // Null only for a row written before the column existed and somehow
        // missed by the backfill; the first sighting is the honest answer in
        // that case, not today.
        let last_seen: String = row
            .get::<_, Option<String>>("last_seen_at")?
            .unwrap_or_else(|| first_seen.clone());
        let state: String = row.get("state")?;
        let severity: String = row.get("severity")?;
        Ok(TriageItem {
            id: row.get("id")?,
            occurrence_key: row.get("occurrence_key")?,
            finding_id: row.get("finding_id")?,
            subject: row.get("subject")?,
            severity: match severity.as_str() {
                "critical" => Severity::Critical,
                "high" => Severity::High,
                "medium" => Severity::Medium,
                "low" => Severity::Low,
                _ => Severity::Info,
            },
            title: row.get("title")?,
            finding: serde_json::from_str(&finding_json).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })?,
            state: ItemState::parse(&state),
            attempts: 0,
            seen: seen_line(&first_seen, &last_seen),
            first_seen,
            last_seen,
        })
    }
}

impl FixStore {
    /// Everything still waiting, worst first.
    ///
    /// Ordering is severity first, then how long it has been waiting.
    /// Stability and security problems get worked before application
    /// annoyances, however long the annoyance has been sitting there.
    pub fn pending(&self) -> Result<Vec<TriageItem>> {
        let mut statement = self.connection.prepare(
            "SELECT * FROM queue WHERE state = 'pending'
             ORDER BY CASE severity
                        WHEN 'critical' THEN 0
                        WHEN 'high'     THEN 1
                        WHEN 'medium'   THEN 2
                        WHEN 'low'      THEN 3
                        ELSE 4
                      END,
                      created_at",
        )?;
        let items = statement.query_map([], Self::row_to_item)?;

        let mut result = Vec::new();
        for item in items {
            let mut item = item?;
            item.attempts = self.attempt_count(&item.occurrence_key)?;
            result.push(item);
        }
        Ok(result)
    }

    /// Every item, whatever its state.
    pub fn all(&self) -> Result<Vec<TriageItem>> {
        let mut statement = self
            .connection
            .prepare("SELECT * FROM queue ORDER BY created_at DESC")?;
        let items = statement.query_map([], Self::row_to_item)?;
        Ok(items.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn set_state(&self, occurrence_key: &str, state: ItemState) -> Result<()> {
        self.connection.execute(
            "UPDATE queue SET state = ?1, updated_at = ?2 WHERE occurrence_key = ?3",
            params![state.as_str(), Self::now(), occurrence_key],
        )?;
        self.audit(
            "state-change",
            &format!("`{occurrence_key}` is now {}", state.as_str()),
            None,
        )?;
        Ok(())
    }

    fn attempt_count(&self, occurrence_key: &str) -> Result<usize> {
        let count: i64 = self.connection.query_row(
            "SELECT COUNT(*) FROM attempts WHERE occurrence_key = ?1",
            params![occurrence_key],
            |row| row.get(0),
        )?;
        Ok(count as usize)
    }

    /// Record something that was tried.
    pub fn record_attempt(
        &self,
        occurrence_key: &str,
        finding_id: &str,
        action: &FixAction,
        outcome: AttemptOutcome,
        detail: Option<&str>,
        snapshot_id: Option<&str>,
    ) -> Result<()> {
        let summary = action.describe();

        // Advice is not an event. Offering it again on the next run is not a
        // second event either, and `outlaw fix` with no `--apply` is a preview
        // somebody may run a dozen times while deciding. Each of those runs was
        // writing the whole instruction -- a full paragraph of prose -- into
        // the audit log, twice over, under the heading `attempt`, for something
        // that attempted nothing and changed nothing.
        //
        // The record still has to be complete, so the first time a piece of
        // advice is given for a problem it is recorded. The identical advice
        // for the identical problem, already sitting in the record, is not
        // news. Anything that actually touched the machine has a different
        // outcome and is never caught by this.
        if outcome == AttemptOutcome::NeedsAPerson && self.already_said(occurrence_key, &summary)? {
            return Ok(());
        }

        self.connection.execute(
            "INSERT INTO attempts
                (occurrence_key, finding_id, action_json, summary, outcome, detail,
                 snapshot_id, at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                occurrence_key,
                finding_id,
                serde_json::to_string(action)?,
                summary,
                outcome.as_str(),
                detail,
                snapshot_id,
                Self::now(),
            ],
        )?;
        // Advice gets its own heading, because a log that files "here is what
        // you could do" under the same word as "this is what I did to your
        // machine" is answering the wrong question.
        let (kind, message) = match outcome {
            AttemptOutcome::NeedsAPerson => ("advice", summary),
            _ => ("attempt", format!("{summary} -- {}", outcome.as_str())),
        };
        self.audit(kind, &message, Some(occurrence_key))?;
        Ok(())
    }

    /// Whether this exact advice for this exact problem is already recorded.
    fn already_said(&self, occurrence_key: &str, summary: &str) -> Result<bool> {
        let found: Option<i64> = self
            .connection
            .query_row(
                "SELECT 1 FROM attempts
                 WHERE occurrence_key = ?1 AND summary = ?2 AND outcome = ?3
                 LIMIT 1",
                params![
                    occurrence_key,
                    summary,
                    AttemptOutcome::NeedsAPerson.as_str()
                ],
                |row| row.get(0),
            )
            .optional()?;
        Ok(found.is_some())
    }

    /// Everything tried for one problem, oldest first.
    pub fn attempts_for(&self, occurrence_key: &str) -> Result<Vec<AttemptRecord>> {
        let mut statement = self.connection.prepare(
            "SELECT occurrence_key, summary, outcome, detail, at
             FROM attempts WHERE occurrence_key = ?1 ORDER BY id",
        )?;
        let rows = statement.query_map(params![occurrence_key], |row| {
            let outcome: String = row.get(2)?;
            Ok(AttemptRecord {
                occurrence_key: row.get(0)?,
                summary: row.get(1)?,
                outcome: AttemptOutcome::parse(&outcome),
                detail: row.get(3)?,
                at: row.get(4)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Actions already tried without success for this problem.
    ///
    /// Used to skip candidates that have already failed, so a second run does
    /// not repeat the first one's dead ends.
    pub fn already_failed(&self, occurrence_key: &str) -> Result<Vec<String>> {
        let mut statement = self.connection.prepare(
            "SELECT DISTINCT summary FROM attempts
             WHERE occurrence_key = ?1 AND outcome IN ('rolled-back', 'failed')",
        )?;
        let rows = statement.query_map(params![occurrence_key], |row| row.get::<_, String>(0))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// What has worked before for this kind of problem on this machine.
    ///
    /// This is what makes a repeat occurrence resolve on the first try instead
    /// of working down the candidate list again. Ordered by how often each
    /// action has succeeded.
    pub fn known_good(&self, finding_id: &str) -> Result<Vec<String>> {
        let mut statement = self.connection.prepare(
            "SELECT summary, COUNT(*) AS wins FROM attempts
             WHERE finding_id = ?1 AND outcome = 'succeeded'
             GROUP BY summary ORDER BY wins DESC",
        )?;
        let rows = statement.query_map(params![finding_id], |row| row.get::<_, String>(0))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Append to the audit log.
    pub fn audit(&self, kind: &str, message: &str, data: Option<&str>) -> Result<()> {
        self.connection.execute(
            "INSERT INTO audit (at, kind, message, data) VALUES (?1, ?2, ?3, ?4)",
            params![Self::now(), kind, message, data],
        )?;
        Ok(())
    }

    /// The most recent audit entries, newest first.
    pub fn audit_log(&self, limit: usize) -> Result<Vec<AuditLine>> {
        // Clamped here rather than by each caller. Asking for none returned
        // none, which the command line then reported as "Nothing has been
        // recorded yet" -- a true statement about the answer and a false one
        // about the machine, on a screen whose entire job is to say what the
        // tool has done. The window clamped and the terminal did not, which is
        // the usual sign that a rule is in the wrong place.
        let limit = limit.clamp(1, MOST_AUDIT_LINES);
        let mut statement = self
            .connection
            .prepare("SELECT at, kind, message FROM audit ORDER BY id DESC LIMIT ?1")?;
        let rows = statement.query_map(params![limit as i64], |row| {
            let at: String = row.get(0)?;
            Ok(AuditLine {
                readable: readable_time(&at),
                at,
                kind: row.get(1)?,
                message: row.get(2)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ork_core::finding::Category;

    fn finding(id: &str, subject: &str, severity: Severity) -> Finding {
        Finding::builder("test.probe", id)
            .subject(subject)
            .severity(severity)
            .category(Category::Application)
            .title(format!("{id} on {subject}"))
            .detail("something is wrong")
            .build()
    }

    fn action() -> FixAction {
        FixAction::RestartService {
            service: "steam".to_string(),
        }
    }

    #[test]
    fn the_same_advice_is_recorded_once_however_often_it_is_offered() {
        // `outlaw fix` without `--apply` is a preview, and somebody deciding
        // what to do may run it a dozen times. Each run was writing the whole
        // instruction into the audit log under the heading `attempt`, for
        // something that attempted nothing.
        let store = FixStore::in_memory().unwrap();
        let problem = finding("app.launch-failed", "steam", Severity::High);
        let key = problem.occurrence_key();
        store.enqueue(&problem).unwrap();
        let advice = FixAction::Manual {
            instruction: "Try turning it off and on again, at length, in prose.".to_string(),
        };

        for _ in 0..5 {
            store
                .record_attempt(
                    &key,
                    "app.launch-failed",
                    &advice,
                    AttemptOutcome::NeedsAPerson,
                    None,
                    None,
                )
                .unwrap();
        }

        assert_eq!(store.attempts_for(&key).unwrap().len(), 1);
        let advice_lines = store
            .audit_log(100)
            .unwrap()
            .into_iter()
            .filter(|line| line.kind == "advice")
            .count();
        assert_eq!(advice_lines, 1, "the same advice was logged more than once");
    }

    #[test]
    fn different_advice_for_the_same_problem_is_all_recorded() {
        // The dedupe must not swallow a second, different suggestion.
        let store = FixStore::in_memory().unwrap();
        let problem = finding("app.launch-failed", "steam", Severity::High);
        let key = problem.occurrence_key();
        store.enqueue(&problem).unwrap();

        for text in ["first idea", "second idea"] {
            store
                .record_attempt(
                    &key,
                    "app.launch-failed",
                    &FixAction::Manual {
                        instruction: text.to_string(),
                    },
                    AttemptOutcome::NeedsAPerson,
                    None,
                    None,
                )
                .unwrap();
        }
        assert_eq!(store.attempts_for(&key).unwrap().len(), 2);
    }

    #[test]
    fn something_that_touched_the_machine_is_always_recorded() {
        // The dedupe is only ever allowed to drop advice. Two identical real
        // attempts are two things that happened to somebody's computer, and
        // the audit log is the only record that they did.
        let store = FixStore::in_memory().unwrap();
        let problem = finding("service.stopped", "steam", Severity::High);
        let key = problem.occurrence_key();
        store.enqueue(&problem).unwrap();

        for _ in 0..3 {
            store
                .record_attempt(
                    &key,
                    "service.stopped",
                    &action(),
                    AttemptOutcome::Failed,
                    None,
                    None,
                )
                .unwrap();
        }
        assert_eq!(store.attempts_for(&key).unwrap().len(), 3);
        let attempts = store
            .audit_log(100)
            .unwrap()
            .into_iter()
            .filter(|line| line.kind == "attempt")
            .count();
        assert_eq!(attempts, 3);
    }

    #[test]
    fn asking_for_no_audit_lines_gives_one_rather_than_none() {
        // `outlaw audit --limit 0` printed "Nothing has been recorded yet",
        // which is true of the answer and false of the machine -- on the one
        // screen whose job is to say what the tool has done.
        let store = FixStore::in_memory().unwrap();
        store.audit("queued", "something happened", None).unwrap();
        store.audit("queued", "and another thing", None).unwrap();

        assert_eq!(store.audit_log(0).unwrap().len(), 1);
        assert_eq!(store.audit_log(1).unwrap().len(), 1);
        assert_eq!(store.audit_log(50).unwrap().len(), 2);
    }

    #[test]
    fn asking_for_more_audit_lines_than_the_cap_gives_the_cap() {
        let store = FixStore::in_memory().unwrap();
        for _ in 0..3 {
            store.audit("queued", "something happened", None).unwrap();
        }
        // Not the number itself -- that a huge request is bounded at all.
        assert!(store.audit_log(usize::MAX).unwrap().len() <= MOST_AUDIT_LINES);
    }

    #[test]
    fn seeing_a_queued_problem_again_records_that_it_is_still_there() {
        // The difference between a problem the machine still has and one it
        // had a fortnight ago. Without this the queue states a stale finding
        // in the present tense, and `outlaw fix` offers to act on it.
        let store = FixStore::in_memory().unwrap();
        let problem = finding("app.launch-failed", "steam", Severity::High);
        store.enqueue(&problem).unwrap();

        let first = store.pending().unwrap().remove(0);
        assert_eq!(
            first.first_seen, first.last_seen,
            "one sighting means both times are the same"
        );

        // Reach past the clock rather than waiting a second for it, and set
        // the stored time back so that a second sighting has to move it.
        store
            .connection
            .execute(
                "UPDATE queue SET created_at = ?1, last_seen_at = ?1",
                params!["2020-01-01T00:00:00Z"],
            )
            .unwrap();

        assert!(!store.enqueue(&problem).unwrap(), "still one item");

        let again = store.pending().unwrap().remove(0);
        assert_eq!(
            again.first_seen, "2020-01-01T00:00:00Z",
            "the first sighting does not move"
        );
        assert_ne!(
            again.last_seen, "2020-01-01T00:00:00Z",
            "seeing it again should have been recorded"
        );
    }

    #[test]
    fn a_database_written_before_the_column_existed_still_opens() {
        // The shape of every queue database already out there. Opening one
        // must add the column and fill it from the first sighting -- not from
        // today, which would make every stale item look freshly seen and is
        // the exact claim the column exists to stop the tool making.
        let store = FixStore::in_memory().unwrap();
        store
            .connection
            .execute_batch("ALTER TABLE queue DROP COLUMN last_seen_at")
            .unwrap();
        store
            .connection
            .execute(
                "INSERT INTO queue
                    (occurrence_key, finding_id, subject, severity, title, finding_json,
                     state, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
                params![
                    "old-key",
                    "app.launch-failed",
                    "steam",
                    "high",
                    "an old problem",
                    serde_json::to_string(&finding("app.launch-failed", "steam", Severity::High))
                        .unwrap(),
                    ItemState::Pending.as_str(),
                    "2019-06-01T00:00:00Z",
                ],
            )
            .unwrap();

        store.migrate().unwrap();

        let item = store.pending().unwrap().remove(0);
        assert_eq!(item.first_seen, "2019-06-01T00:00:00Z");
        assert_eq!(
            item.last_seen, "2019-06-01T00:00:00Z",
            "an old row was last seen when it was seen, not today"
        );
    }

    #[test]
    fn the_same_problem_seen_twice_is_one_queue_item() {
        let store = FixStore::in_memory().unwrap();
        let problem = finding("app.launch-failed", "steam", Severity::High);

        assert!(
            store.enqueue(&problem).unwrap(),
            "first sighting should queue"
        );
        assert!(
            !store.enqueue(&problem).unwrap(),
            "second sighting should not re-queue"
        );
        assert_eq!(store.pending().unwrap().len(), 1);
    }

    #[test]
    fn a_resolved_item_is_not_resurrected_by_a_later_scan() {
        // A scan running while a fix is in progress will see the symptom
        // again. That must not undo the resolution.
        let store = FixStore::in_memory().unwrap();
        let problem = finding("app.launch-failed", "steam", Severity::High);
        store.enqueue(&problem).unwrap();
        store
            .set_state(&problem.occurrence_key(), ItemState::Resolved)
            .unwrap();

        assert!(!store.enqueue(&problem).unwrap());
        assert!(store.pending().unwrap().is_empty());
    }

    #[test]
    fn the_queue_is_worked_worst_first() {
        // Stability and security before application annoyances, however long
        // the annoyance has been waiting.
        let store = FixStore::in_memory().unwrap();
        store
            .enqueue(&finding("app.launch-failed", "steam", Severity::Low))
            .unwrap();
        store
            .enqueue(&finding("logs.hardware-error", "cpu", Severity::Critical))
            .unwrap();
        store
            .enqueue(&finding("device.driver-mismatch", "gpu", Severity::High))
            .unwrap();

        let order: Vec<Severity> = store
            .pending()
            .unwrap()
            .iter()
            .map(|item| item.severity)
            .collect();
        assert_eq!(
            order,
            vec![Severity::Critical, Severity::High, Severity::Low]
        );
    }

    #[test]
    fn two_drives_with_the_same_problem_are_two_items() {
        // The subject is part of the identity, or fixing one drive would mark
        // the other as done.
        let store = FixStore::in_memory().unwrap();
        store
            .enqueue(&finding(
                "storage.volume-low-on-space",
                "C:",
                Severity::High,
            ))
            .unwrap();
        store
            .enqueue(&finding(
                "storage.volume-low-on-space",
                "D:",
                Severity::High,
            ))
            .unwrap();
        assert_eq!(store.pending().unwrap().len(), 2);
    }

    #[test]
    fn a_queued_item_keeps_the_whole_finding_including_its_evidence() {
        // The fix layer needs the evidence, and re-running the scan to get it
        // back would be both slow and possibly a different answer.
        let store = FixStore::in_memory().unwrap();
        let problem = Finding::builder("test.probe", "app.launch-failed")
            .subject("steam")
            .title("steam will not start")
            .detail("it failed")
            .evidence(
                "stderr",
                "error while loading shared libraries: libfoo.so.1",
            )
            .build();
        store.enqueue(&problem).unwrap();

        let restored = &store.pending().unwrap()[0].finding;
        assert_eq!(restored.evidence.len(), 1);
        assert!(restored.evidence[0].value.contains("libfoo.so.1"));
    }

    #[test]
    fn attempts_are_recorded_and_can_be_read_back_in_order() {
        let store = FixStore::in_memory().unwrap();
        let problem = finding("app.launch-failed", "steam", Severity::High);
        let key = problem.occurrence_key();
        store.enqueue(&problem).unwrap();

        store
            .record_attempt(
                &key,
                &problem.id,
                &action(),
                AttemptOutcome::RolledBack,
                Some("no change"),
                Some("s1"),
            )
            .unwrap();
        store
            .record_attempt(
                &key,
                &problem.id,
                &action(),
                AttemptOutcome::Succeeded,
                None,
                Some("s2"),
            )
            .unwrap();

        let attempts = store.attempts_for(&key).unwrap();
        assert_eq!(attempts.len(), 2);
        assert_eq!(attempts[0].outcome, AttemptOutcome::RolledBack);
        assert_eq!(attempts[1].outcome, AttemptOutcome::Succeeded);
        assert_eq!(store.pending().unwrap()[0].attempts, 2);
    }

    #[test]
    fn candidates_that_already_failed_are_remembered() {
        // A second run should not repeat the first run's dead ends.
        let store = FixStore::in_memory().unwrap();
        let problem = finding("app.launch-failed", "steam", Severity::High);
        let key = problem.occurrence_key();

        store
            .record_attempt(
                &key,
                &problem.id,
                &action(),
                AttemptOutcome::RolledBack,
                None,
                None,
            )
            .unwrap();

        let failed = store.already_failed(&key).unwrap();
        assert_eq!(failed, vec![action().describe()]);
    }

    #[test]
    fn what_worked_before_is_remembered_for_next_time() {
        // This is what makes a repeat problem resolve on the first try.
        let store = FixStore::in_memory().unwrap();
        let problem = finding("app.launch-hung", "steam", Severity::High);
        store
            .record_attempt(
                &problem.occurrence_key(),
                &problem.id,
                &action(),
                AttemptOutcome::Succeeded,
                None,
                None,
            )
            .unwrap();

        assert_eq!(
            store.known_good("app.launch-hung").unwrap(),
            vec![action().describe()]
        );
        assert!(store.known_good("something.else").unwrap().is_empty());
    }

    #[test]
    fn everything_that_happens_lands_in_the_audit_log() {
        let store = FixStore::in_memory().unwrap();
        let problem = finding("app.launch-failed", "steam", Severity::High);
        store.enqueue(&problem).unwrap();
        store
            .record_attempt(
                &problem.occurrence_key(),
                &problem.id,
                &action(),
                AttemptOutcome::Succeeded,
                None,
                None,
            )
            .unwrap();
        store
            .set_state(&problem.occurrence_key(), ItemState::Resolved)
            .unwrap();

        let log = store.audit_log(50).unwrap();
        let kinds: Vec<&str> = log.iter().map(|entry| entry.kind.as_str()).collect();
        assert!(kinds.contains(&"queued"));
        assert!(kinds.contains(&"attempt"));
        assert!(kinds.contains(&"state-change"));
    }

    #[test]
    fn a_queued_entry_does_not_repeat_what_the_kind_column_already_says() {
        // This line used to read `queued \`{title}\` for triage`, printed beside
        // a column already saying "queued" -- and finding titles carry
        // backticks of their own, so a real one came out as
        // ``queued  ``node.exe` has been using 167% ... ` for triage``.
        let mut problem = finding("system.processes", "node.exe", Severity::Low);
        problem.title = "`node.exe` has been using 167% of a CPU core".to_string();
        let store = FixStore::in_memory().unwrap();
        store.enqueue(&problem).unwrap();

        let entry = store
            .audit_log(50)
            .unwrap()
            .into_iter()
            .find(|entry| entry.kind == "queued")
            .expect("queueing is audited");

        assert_eq!(entry.message, problem.title);
        assert!(!entry.message.contains("``"), "{}", entry.message);
        assert!(!entry.message.starts_with("queued"), "{}", entry.message);
    }

    #[test]
    fn the_store_survives_being_closed_and_reopened() {
        let dir = std::env::temp_dir().join(format!("ork-store-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("state.db");

        let problem = finding("app.launch-failed", "steam", Severity::High);
        {
            let store = FixStore::open(&path).unwrap();
            store.enqueue(&problem).unwrap();
        }
        {
            let store = FixStore::open(&path).unwrap();
            assert_eq!(store.pending().unwrap().len(), 1);
        }

        let _ = std::fs::remove_dir_all(&dir);
    }
}
