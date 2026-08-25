//! Reading the Linux system log.
//!
//! `journalctl` is the primary source because it is structured, covers the
//! kernel and userspace together, and survives reboots -- which matters when
//! the thing being diagnosed is a machine that froze and had to be power
//! cycled. Where journald is absent or has no persistent storage, `dmesg` is
//! used instead; it only covers the current boot, and the caller is told so.

use std::time::Duration;

use serde::Deserialize;
use time::OffsetDateTime;

use crate::Result;
use crate::platform::{LogLevel, LogRecord, common};

/// Memory guard, not a time limit. See the Windows equivalent.
const MAX_ENTRIES: usize = 3000;

/// One journald entry. Every field is optional because journald guarantees
/// almost nothing about which fields a given entry carries.
#[derive(Debug, Deserialize)]
struct RawEntry {
    #[serde(rename = "__REALTIME_TIMESTAMP")]
    realtime: Option<String>,
    #[serde(rename = "PRIORITY")]
    priority: Option<String>,
    #[serde(rename = "SYSLOG_IDENTIFIER")]
    syslog_identifier: Option<String>,
    #[serde(rename = "_COMM")]
    comm: Option<String>,
    #[serde(rename = "_SYSTEMD_UNIT")]
    unit: Option<String>,
    #[serde(rename = "MESSAGE")]
    message: Option<serde_json::Value>,
}

/// syslog priorities: 0-2 are emergency through critical, 3 is error, 4 is
/// warning. Anything less severe is filtered out before it reaches us.
fn level_from_priority(priority: Option<&str>) -> LogLevel {
    match priority.and_then(|p| p.parse::<u8>().ok()) {
        Some(0..=2) => LogLevel::Critical,
        Some(4) => LogLevel::Warning,
        _ => LogLevel::Error,
    }
}

/// journald reports time as microseconds since the Unix epoch, as a string.
fn parse_realtime(raw: Option<&str>) -> Option<OffsetDateTime> {
    let micros: i128 = raw?.parse().ok()?;
    OffsetDateTime::from_unix_timestamp_nanos(micros.checked_mul(1000)?).ok()
}

/// journald sometimes renders a message as an array of byte values rather than
/// a string, when the original was not valid UTF-8.
fn message_text(value: Option<&serde_json::Value>) -> String {
    match value {
        Some(serde_json::Value::String(text)) => text.trim().to_string(),
        Some(serde_json::Value::Array(bytes)) => {
            let raw: Vec<u8> = bytes
                .iter()
                .filter_map(|b| b.as_u64())
                .map(|b| b as u8)
                .collect();
            String::from_utf8_lossy(&raw).trim().to_string()
        }
        _ => String::new(),
    }
}

/// `journalctl -o json` emits one JSON object per line, not a JSON array.
fn parse_journal(stdout: &str) -> Vec<LogRecord> {
    stdout
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str::<RawEntry>(line).ok())
        .map(|entry| {
            let source = entry
                .syslog_identifier
                .or(entry.comm)
                .or(entry.unit)
                .unwrap_or_else(|| "kernel".to_string());
            LogRecord {
                timestamp: parse_realtime(entry.realtime.as_deref()),
                level: level_from_priority(entry.priority.as_deref()),
                event_id: Some(source.clone()),
                source,
                message: message_text(entry.message.as_ref()),
            }
        })
        .collect()
}

/// Parse `dmesg --level=err,crit,alert,emerg,warn` output.
///
/// The fallback path. `dmesg` gives us the kernel ring buffer for the current
/// boot only, which is exactly the wrong scope for diagnosing a machine that
/// froze yesterday -- but it is better than nothing on a system without
/// persistent journald storage.
fn parse_dmesg(stdout: &str) -> Vec<LogRecord> {
    stdout
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            // Lines look like `kern  :err   : nvidia: something went wrong`.
            let (level, rest) = match line.split_once(':') {
                Some((facility, rest)) => {
                    let facility = facility.trim();
                    match rest.split_once(':') {
                        Some((level, message)) => (level.trim().to_string(), message.trim()),
                        None => (facility.to_string(), rest.trim()),
                    }
                }
                None => (String::new(), line.trim()),
            };
            let level = match level.as_str() {
                "emerg" | "alert" | "crit" => LogLevel::Critical,
                "warn" => LogLevel::Warning,
                _ => LogLevel::Error,
            };
            // The subsystem prefix, where the kernel bothered to include one,
            // is what makes repeats groupable.
            let source = rest
                .split_once(':')
                .map(|(prefix, _)| prefix.trim())
                .filter(|prefix| !prefix.is_empty() && prefix.len() < 32)
                .unwrap_or("kernel")
                .to_string();
            LogRecord {
                timestamp: None,
                level,
                event_id: Some(source.clone()),
                source,
                message: rest.to_string(),
            }
        })
        .collect()
}

/// Warning-and-worse entries from the system log.
pub fn recent_errors(since: Duration) -> Result<Vec<LogRecord>> {
    let seconds = format!("-{}s", since.as_secs().max(1));
    let max = MAX_ENTRIES.to_string();

    if common::tool_on_path("journalctl") {
        let output = common::run_capture(
            "journalctl",
            &[
                "--priority=4",
                "--since",
                &seconds,
                "--lines",
                &max,
                "--output=json",
                "--no-pager",
            ],
        )?;
        if output.success {
            return Ok(parse_journal(&output.stdout));
        }
        tracing::debug!(stderr = %output.stderr.trim(), "journalctl failed, falling back to dmesg");
    }

    if common::tool_on_path("dmesg") {
        let output =
            common::run_capture("dmesg", &["--level=emerg,alert,crit,err,warn", "--decode"])?;
        if output.success {
            return Ok(parse_dmesg(&output.stdout));
        }
        // Unprivileged dmesg is blocked outright on many hardened kernels
        // (kernel.dmesg_restrict). That is a permissions problem, and saying so
        // is more useful than reporting a clean log.
        anyhow::bail!(
            "could not read the kernel log: neither journalctl nor dmesg returned data \
             (dmesg may be restricted to root by kernel.dmesg_restrict)"
        );
    }

    anyhow::bail!("no system log reader available (looked for journalctl and dmesg)")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn journal_lines_parse_independently() {
        let stdout = concat!(
            r#"{"__REALTIME_TIMESTAMP":"1756060980000000","PRIORITY":"3","SYSLOG_IDENTIFIER":"kernel","MESSAGE":"NVRM: Xid (PCI:0000:01:00): 79"}"#,
            "\n",
            "not json at all\n",
            r#"{"PRIORITY":"4","_COMM":"steam","MESSAGE":"segfault at 0"}"#,
            "\n"
        );
        let records = parse_journal(stdout);
        // The unparseable line is skipped without costing us the other two.
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].level, LogLevel::Error);
        assert_eq!(records[0].source, "kernel");
        assert!(records[0].timestamp.is_some());
        assert_eq!(records[1].level, LogLevel::Warning);
        assert_eq!(records[1].source, "steam");
    }

    #[test]
    fn priorities_map_to_levels() {
        assert_eq!(level_from_priority(Some("0")), LogLevel::Critical);
        assert_eq!(level_from_priority(Some("2")), LogLevel::Critical);
        assert_eq!(level_from_priority(Some("3")), LogLevel::Error);
        assert_eq!(level_from_priority(Some("4")), LogLevel::Warning);
        assert_eq!(level_from_priority(None), LogLevel::Error);
    }

    #[test]
    fn non_utf8_messages_survive_as_byte_arrays() {
        let value = serde_json::json!([104, 105, 255]);
        assert_eq!(message_text(Some(&value)), "hi\u{fffd}");
    }

    #[test]
    fn dmesg_lines_yield_a_subsystem() {
        let records = parse_dmesg("kern  :err   : nvidia: GPU has fallen off the bus\n");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].level, LogLevel::Error);
        assert_eq!(records[0].source, "nvidia");
    }
}
