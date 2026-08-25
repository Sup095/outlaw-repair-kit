//! Reading the Windows Event Log.
//!
//! This shells out to PowerShell's `Get-WinEvent` rather than binding
//! `EvtQuery` directly. That is a deliberate trade: `Get-WinEvent` is the
//! mature, well-understood interface to the same data, it is present on every
//! supported Windows version, and it keeps this file free of unsafe COM
//! marshalling for a read-only diagnostic. If profiling later shows the
//! process spawn is a real cost, the native API can be swapped in behind this
//! same function without touching a single probe.

use std::time::Duration;

use anyhow::Context;
use serde::Deserialize;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::Result;
use crate::platform::{LogLevel, LogRecord, common};

/// Upper bound on events pulled in one query.
///
/// This is not a time limit -- it is a memory guard. A machine that has been
/// screaming into its event log for a month can hold hundreds of thousands of
/// entries, and the newest few thousand tell the same story as all of them.
const MAX_EVENTS: usize = 3000;

/// Raw shape of one event as the PowerShell shim emits it.
#[derive(Debug, Deserialize)]
struct RawEvent {
    t: Option<String>,
    id: Option<i64>,
    lvl: Option<i64>,
    src: Option<String>,
    msg: Option<String>,
}

/// Windows event levels: 1 is Critical, 2 is Error, 3 is Warning.
fn level_from_windows(level: Option<i64>) -> LogLevel {
    match level {
        Some(1) => LogLevel::Critical,
        Some(3) => LogLevel::Warning,
        // Anything else that got past the query filter is an error-class event.
        _ => LogLevel::Error,
    }
}

fn parse_events(json: &str) -> Result<Vec<LogRecord>> {
    let json = json.trim();
    if json.is_empty() {
        return Ok(Vec::new());
    }

    // ConvertTo-Json emits a bare object rather than a one-element array when
    // exactly one event matched. Both shapes have to be accepted.
    let raw: Vec<RawEvent> = match serde_json::from_str::<Vec<RawEvent>>(json) {
        Ok(events) => events,
        Err(_) => vec![
            serde_json::from_str::<RawEvent>(json)
                .context("could not parse Get-WinEvent output as JSON")?,
        ],
    };

    Ok(raw
        .into_iter()
        .map(|event| LogRecord {
            timestamp: event
                .t
                .as_deref()
                .and_then(|stamp| OffsetDateTime::parse(stamp, &Rfc3339).ok()),
            source: event.src.unwrap_or_else(|| "unknown".to_string()),
            level: level_from_windows(event.lvl),
            event_id: event.id.map(|id| id.to_string()),
            message: event.msg.unwrap_or_default().trim().to_string(),
        })
        .collect())
}

/// Warning-and-worse entries from the System and Application logs.
pub fn recent_errors(since: Duration) -> Result<Vec<LogRecord>> {
    let seconds = since.as_secs().max(1);
    let script = format!(
        "$ErrorActionPreference='SilentlyContinue'; \
         $start=(Get-Date).AddSeconds(-{seconds}); \
         Get-WinEvent -FilterHashtable @{{LogName='System','Application';Level=1,2,3;StartTime=$start}} \
         -MaxEvents {MAX_EVENTS} | ForEach-Object {{ [pscustomobject]@{{ \
         t=$_.TimeCreated.ToUniversalTime().ToString('o'); id=$_.Id; lvl=$_.Level; \
         src=$_.ProviderName; msg=$_.Message }} }} | ConvertTo-Json -Compress -Depth 2"
    );

    let output = common::run_capture(
        "powershell",
        &["-NoProfile", "-NonInteractive", "-Command", &script],
    )?;

    // Get-WinEvent exits non-zero when nothing matched the filter, which is a
    // clean machine rather than a failure.
    if !output.success && output.stdout.trim().is_empty() {
        tracing::debug!(stderr = %output.stderr.trim(), "Get-WinEvent returned no events");
        return Ok(Vec::new());
    }

    parse_events(&output.stdout)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_output_is_an_empty_log_not_an_error() {
        assert!(parse_events("").unwrap().is_empty());
        assert!(parse_events("   \n").unwrap().is_empty());
    }

    #[test]
    fn a_single_event_arrives_as_a_bare_object() {
        // This is the shape ConvertTo-Json produces for exactly one match, and
        // getting it wrong would silently drop the only event on a machine
        // that has just one thing wrong with it.
        let json = r#"{"t":"2026-08-24T19:03:00.1234567Z","id":41,"lvl":1,
                       "src":"Microsoft-Windows-Kernel-Power","msg":"The system rebooted."}"#;
        let events = parse_events(json).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].level, LogLevel::Critical);
        assert_eq!(events[0].event_id.as_deref(), Some("41"));
        assert_eq!(events[0].signature(), "Microsoft-Windows-Kernel-Power/41");
        assert!(events[0].timestamp.is_some());
    }

    #[test]
    fn multiple_events_arrive_as_an_array() {
        let json = r#"[{"t":null,"id":1001,"lvl":2,"src":"BugCheck","msg":" crash "},
                       {"t":null,"id":7,"lvl":3,"src":"disk","msg":"bad block"}]"#;
        let events = parse_events(json).unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].level, LogLevel::Error);
        assert_eq!(events[0].message, "crash", "message should be trimmed");
        assert_eq!(events[1].level, LogLevel::Warning);
    }

    #[test]
    fn windows_level_numbers_map_to_our_levels() {
        assert_eq!(level_from_windows(Some(1)), LogLevel::Critical);
        assert_eq!(level_from_windows(Some(2)), LogLevel::Error);
        assert_eq!(level_from_windows(Some(3)), LogLevel::Warning);
        assert_eq!(level_from_windows(None), LogLevel::Error);
    }
}
