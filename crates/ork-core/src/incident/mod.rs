//! Catching errors and crashes so they can be reported afterwards.
//!
//! A tool that fails on somebody else's computer is very hard to fix, because
//! the one person who saw what happened is the one person who cannot read a
//! backtrace. This module keeps a short record of everything that went wrong,
//! so that afterwards there is something concrete to hand over.
//!
//! Two things are recorded, and they arrive by different routes:
//!
//! * **Errors** -- anything logged at error level, anywhere in the tool,
//!   picked up by a [`tracing`] layer. Nothing has to remember to call this.
//! * **Crashes** -- a panic hook catches what would otherwise be a stack trace
//!   scrolling past in a terminal nobody was watching.
//!
//! Both land in one file, newest last, capped so it cannot grow without limit.
//! Nothing here is sent anywhere: this half only remembers. Turning a record
//! into something postable, with the personal details taken out, is
//! [`report`], and putting it on a bug tracker is a decision only the person
//! at the keyboard makes.

pub mod redact;
pub mod report;

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use tracing::Subscriber;
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::LookupSpan;

pub use redact::Redactor;
pub use report::Report;

/// How many records to keep.
///
/// Enough to cover a session that went wrong repeatedly, few enough that the
/// file stays readable and a report built from it stays postable. The oldest
/// go first: the most recent failure is almost always the one being chased.
const KEEP: usize = 200;

/// What went wrong.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IncidentKind {
    /// Something was logged at error level. The tool carried on.
    Error,
    /// The tool crashed.
    Panic,
}

impl IncidentKind {
    pub fn as_str(self) -> &'static str {
        match self {
            IncidentKind::Error => "error",
            IncidentKind::Panic => "crash",
        }
    }
}

/// One thing that went wrong, as it was recorded at the time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Incident {
    /// RFC 3339, in UTC. A local timestamp would say roughly where somebody
    /// is, and the ordering is what matters here.
    pub at: String,
    pub kind: IncidentKind,
    /// Which part of the tool it came from, e.g. `ork_fix::engine`.
    pub source: String,
    pub message: String,
    /// Where in the source, when that is known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    /// Present for a crash, and only when backtraces were switched on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backtrace: Option<String>,
}

impl Incident {
    fn new(kind: IncidentKind, source: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            at: OffsetDateTime::now_utc()
                .format(&Rfc3339)
                .unwrap_or_else(|_| "unknown".to_string()),
            kind,
            source: source.into(),
            message: message.into(),
            location: None,
            backtrace: None,
        }
    }

    /// A single line, the way it would be read in a terminal.
    pub fn line(&self) -> String {
        match &self.location {
            Some(location) => format!(
                "{}  {}  {}  {} ({location})",
                self.at,
                self.kind.as_str(),
                self.source,
                self.message
            ),
            None => format!(
                "{}  {}  {}  {}",
                self.at,
                self.kind.as_str(),
                self.source,
                self.message
            ),
        }
    }
}

/// Where the record is kept, given the directory the tool keeps its state in.
pub fn log_path(state_dir: &Path) -> PathBuf {
    state_dir.join("incidents.jsonl")
}

/// Append one record.
///
/// Failures to write are swallowed on purpose. This runs on the path where
/// something has *already* gone wrong, and a diagnostic tool that panics while
/// recording a panic is worse than one that quietly loses a line.
pub fn record(state_dir: &Path, incident: &Incident) {
    if let Err(error) = try_record(state_dir, incident) {
        // Deliberately not `error!`: that would come straight back here.
        eprintln!("outlaw: could not record a problem: {error}");
    }
}

fn try_record(state_dir: &Path, incident: &Incident) -> std::io::Result<()> {
    std::fs::create_dir_all(state_dir)?;
    let path = log_path(state_dir);

    let mut line = serde_json::to_string(incident)
        .unwrap_or_else(|_| r#"{"kind":"error","message":"unserialisable"}"#.to_string());
    line.push('\n');

    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    file.write_all(line.as_bytes())?;
    drop(file);

    trim(&path)
}

/// Keep the file to [`KEEP`] records.
///
/// Checked by line count rather than by size, so one enormous backtrace cannot
/// evict the whole history behind it.
fn trim(path: &Path) -> std::io::Result<()> {
    let lines: Vec<String> = BufReader::new(std::fs::File::open(path)?)
        .lines()
        .map_while(Result::ok)
        .collect();
    if lines.len() <= KEEP {
        return Ok(());
    }

    // Written beside the original and renamed over it, so an interruption
    // halfway through leaves the old file intact rather than a truncated one.
    let temporary = path.with_extension("jsonl.trimming");
    {
        let mut file = std::fs::File::create(&temporary)?;
        for line in &lines[lines.len() - KEEP..] {
            writeln!(file, "{line}")?;
        }
    }
    std::fs::rename(&temporary, path)
}

/// Everything recorded, oldest first.
///
/// Unreadable lines are skipped rather than failing the read. A record written
/// by an older version, or half-written when the power went out, should not
/// stop somebody reporting the crash that happened afterwards.
pub fn all(state_dir: &Path) -> Vec<Incident> {
    let path = log_path(state_dir);
    let Ok(file) = std::fs::File::open(&path) else {
        return Vec::new();
    };
    BufReader::new(file)
        .lines()
        .map_while(Result::ok)
        .filter_map(|line| serde_json::from_str(&line).ok())
        .collect()
}

/// The most recent records, oldest first within the window.
pub fn recent(state_dir: &Path, limit: usize) -> Vec<Incident> {
    let mut all = all(state_dir);
    if all.len() > limit {
        all.drain(..all.len() - limit);
    }
    all
}

/// Forget everything recorded so far.
pub fn clear(state_dir: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(log_path(state_dir)) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

/// Start recording crashes.
///
/// The previous hook is kept and still called, so the usual panic message
/// still reaches the terminal. Somebody watching a crash happen should not
/// have to go looking in a file to find out what it said.
pub fn catch_crashes(state_dir: PathBuf) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let mut incident =
            Incident::new(IncidentKind::Panic, "outlaw", panic_message(info.payload()));
        incident.location = info.location().map(|at| at.to_string());

        // Only when the user asked for backtraces. Capturing one regardless
        // would slow every crash down and fill the record with frames nobody
        // switched on.
        if std::env::var("RUST_BACKTRACE").is_ok_and(|value| value != "0") {
            incident.backtrace = Some(std::backtrace::Backtrace::force_capture().to_string());
        }

        record(&state_dir, &incident);
        previous(info);
    }));
}

/// What a panic said, whichever way it said it.
///
/// `panic!("text")` carries a `&str` and `panic!("{x}")` carries a `String`,
/// and a payload from somewhere else may be neither. All three have to
/// produce something a person can read.
fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|text| (*text).to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "a crash with no message".to_string())
}

/// A [`tracing`] layer that records everything logged at error level.
///
/// Attached to the subscriber, so no code has to remember to report itself.
/// The alternative -- calling [`record`] at each failure site -- would be
/// forgotten at exactly the sites that matter most, which are the ones nobody
/// expected to fail.
pub struct IncidentLayer {
    state_dir: PathBuf,
    /// Guards against a storm: an error inside the recording path would
    /// otherwise log an error, which would record it, and so on.
    recording: Arc<Mutex<bool>>,
}

impl IncidentLayer {
    pub fn new(state_dir: PathBuf) -> Self {
        Self {
            state_dir,
            recording: Arc::new(Mutex::new(false)),
        }
    }
}

impl<S> Layer<S> for IncidentLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
        if *event.metadata().level() != tracing::Level::ERROR {
            return;
        }

        let Ok(mut recording) = self.recording.lock() else {
            return;
        };
        if *recording {
            return;
        }
        *recording = true;

        let mut visitor = Message::default();
        event.record(&mut visitor);

        let mut incident = Incident::new(
            IncidentKind::Error,
            event.metadata().target(),
            visitor.finish(),
        );
        incident.location = match (event.metadata().file(), event.metadata().line()) {
            (Some(file), Some(line)) => Some(format!("{file}:{line}")),
            _ => None,
        };
        record(&self.state_dir, &incident);

        *recording = false;
    }
}

/// Flattens a log event's fields into one line.
///
/// The `message` field leads, because that is the sentence a person wrote;
/// everything else follows as `key=value`.
#[derive(Default)]
struct Message {
    message: String,
    fields: Vec<String>,
}

impl Message {
    fn finish(self) -> String {
        let mut out = self.message;
        for field in self.fields {
            if !out.is_empty() {
                out.push_str(", ");
            }
            out.push_str(&field);
        }
        if out.is_empty() {
            "an error with no message".to_string()
        } else {
            out
        }
    }
}

impl tracing::field::Visit for Message {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.message = format!("{value:?}");
            // Debug on a string literal adds quotes that are noise in a
            // report.
            if self.message.len() >= 2
                && self.message.starts_with('"')
                && self.message.ends_with('"')
            {
                self.message = self.message[1..self.message.len() - 1].to_string();
            }
        } else {
            self.fields.push(format!("{}={value:?}", field.name()));
        }
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.message = value.to_string();
        } else {
            self.fields.push(format!("{}={value}", field.name()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ork-incident-{}-{name}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn error(message: &str) -> Incident {
        Incident::new(IncidentKind::Error, "test", message)
    }

    #[test]
    fn a_recorded_problem_can_be_read_back() {
        let dir = scratch("roundtrip");
        record(&dir, &error("the snapshot directory is read-only"));

        let all = all(&dir);
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].message, "the snapshot directory is read-only");
        assert_eq!(all[0].kind, IncidentKind::Error);
        assert!(!all[0].at.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn records_are_kept_in_the_order_they_happened() {
        let dir = scratch("order");
        for index in 0..5 {
            record(&dir, &error(&format!("problem {index}")));
        }
        let messages: Vec<String> = all(&dir).into_iter().map(|item| item.message).collect();
        assert_eq!(messages.first().unwrap(), "problem 0");
        assert_eq!(messages.last().unwrap(), "problem 4");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_oldest_records_are_dropped_rather_than_the_newest() {
        // The most recent failure is nearly always the one being chased.
        let dir = scratch("trim");
        for index in 0..(KEEP + 30) {
            record(&dir, &error(&format!("problem {index}")));
        }

        let all = all(&dir);
        assert_eq!(all.len(), KEEP);
        assert_eq!(all[0].message, "problem 30");
        assert_eq!(
            all.last().unwrap().message,
            format!("problem {}", KEEP + 29)
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_damaged_line_does_not_stop_the_rest_being_read() {
        // A half-written record must not be able to block reporting the crash
        // that happened right after it.
        let dir = scratch("damaged");
        record(&dir, &error("before"));
        {
            let mut file = std::fs::OpenOptions::new()
                .append(true)
                .open(log_path(&dir))
                .unwrap();
            writeln!(file, "{{not valid json").unwrap();
        }
        record(&dir, &error("after"));

        let messages: Vec<String> = all(&dir).into_iter().map(|item| item.message).collect();
        assert_eq!(messages, vec!["before", "after"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reading_a_machine_that_has_never_had_a_problem_is_not_an_error() {
        let dir = scratch("empty");
        assert!(all(&dir).is_empty());
        assert!(recent(&dir, 10).is_empty());
        assert!(clear(&dir).is_ok(), "clearing nothing must not fail");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn recent_returns_the_newest_window_in_order() {
        let dir = scratch("recent");
        for index in 0..10 {
            record(&dir, &error(&format!("problem {index}")));
        }
        let window: Vec<String> = recent(&dir, 3)
            .into_iter()
            .map(|item| item.message)
            .collect();
        assert_eq!(window, vec!["problem 7", "problem 8", "problem 9"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_crash_message_is_read_whichever_way_it_was_written() {
        // `panic!("text")` carries a &str; `panic!("{x}")` carries a String;
        // a payload from elsewhere may be neither, and must still say
        // something rather than producing an empty report.
        assert_eq!(panic_message(&"a literal"), "a literal");
        assert_eq!(panic_message(&"formatted 1".to_string()), "formatted 1");
        assert_eq!(panic_message(&42u32), "a crash with no message");
    }

    #[test]
    fn clearing_removes_everything() {
        let dir = scratch("clear");
        record(&dir, &error("something"));
        clear(&dir).unwrap();
        assert!(all(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
