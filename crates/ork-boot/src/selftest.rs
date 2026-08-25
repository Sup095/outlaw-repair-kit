//! Checking that the tool itself is in working order before it is trusted.
//!
//! A diagnostic tool that is quietly broken is worse than no diagnostic tool,
//! because its clean bill of health will be believed. So before doing anything
//! else, it checks itself: can it read the machine, is its own state file
//! intact, did its runbook library parse, can it write where it needs to.
//!
//! Every check is fast. This runs on every start, and a startup sequence that
//! takes real time is one people learn to skip.
//!
//! A check can pass, warn, or fail, and the distinction matters. A missing
//! credential store is a warning -- most of the tool works without one. A
//! corrupt state database is a failure, because the fix engine would lose its
//! audit trail.

use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// How a single check turned out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CheckState {
    Pass,
    /// Works, but something is degraded or unavailable.
    Warn,
    /// Something the tool depends on is broken.
    Fail,
}

impl CheckState {
    pub fn as_str(self) -> &'static str {
        match self {
            CheckState::Pass => "ok",
            CheckState::Warn => "warn",
            CheckState::Fail => "fail",
        }
    }
}

/// The result of one self-check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckResult {
    pub name: String,
    pub state: CheckState,
    pub detail: String,
    pub duration: Duration,
}

impl CheckResult {
    fn new(
        name: &'static str,
        state: CheckState,
        detail: impl Into<String>,
        started: Instant,
    ) -> Self {
        Self {
            name: name.to_string(),
            state,
            detail: detail.into(),
            duration: started.elapsed(),
        }
    }
}

/// Everything the self-test found.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelfTestReport {
    pub checks: Vec<CheckResult>,
    pub duration: Duration,
}

impl SelfTestReport {
    pub fn worst(&self) -> CheckState {
        self.checks
            .iter()
            .map(|check| check.state)
            .max()
            .unwrap_or(CheckState::Pass)
    }

    pub fn passed(&self) -> bool {
        self.worst() != CheckState::Fail
    }

    pub fn failures(&self) -> impl Iterator<Item = &CheckResult> {
        self.checks
            .iter()
            .filter(|check| check.state == CheckState::Fail)
    }

    pub fn warnings(&self) -> impl Iterator<Item = &CheckResult> {
        self.checks
            .iter()
            .filter(|check| check.state == CheckState::Warn)
    }
}

/// How many checks there are, so a progress bar can be sized before starting.
pub const CHECK_COUNT: usize = 6;

fn check_platform() -> CheckResult {
    let started = Instant::now();
    match ork_core::platform::detect().and_then(|platform| {
        let host = platform.host()?;
        Ok((platform.kind(), host))
    }) {
        Ok((kind, host)) => CheckResult::new(
            "platform layer",
            CheckState::Pass,
            format!("{kind} -- {} on {}", host.os_name, host.cpu_brand),
            started,
        ),
        Err(error) => CheckResult::new(
            "platform layer",
            CheckState::Fail,
            format!("cannot read this machine: {error:#}"),
            started,
        ),
    }
}

fn check_probes() -> CheckResult {
    let started = Instant::now();
    let metas = ork_core::probes::all_meta();

    if metas.is_empty() {
        return CheckResult::new(
            "diagnostic checks",
            CheckState::Fail,
            "no checks are registered",
            started,
        );
    }

    // Two probes sharing an id would silently overwrite each other's history
    // and runbook matches -- the kind of fault that only surfaces much later
    // as "why did it suggest that".
    let mut ids: Vec<&str> = metas.iter().map(|meta| meta.id).collect();
    ids.sort_unstable();
    let total = ids.len();
    ids.dedup();
    if ids.len() != total {
        return CheckResult::new(
            "diagnostic checks",
            CheckState::Fail,
            "two checks share an identifier",
            started,
        );
    }

    CheckResult::new(
        "diagnostic checks",
        CheckState::Pass,
        format!("{total} registered"),
        started,
    )
}

fn check_configuration() -> CheckResult {
    let started = Instant::now();
    match ork_core::Config::default_path() {
        Ok(path) => match ork_core::Config::load_or_default(&path) {
            Ok(_) => {
                let state = if path.exists() {
                    "loaded"
                } else {
                    "using defaults"
                };
                CheckResult::new("configuration", CheckState::Pass, state, started)
            }
            // A broken settings file is a warning, not a failure: the tool
            // runs on defaults, and saying so beats refusing to start.
            Err(error) => CheckResult::new(
                "configuration",
                CheckState::Warn,
                format!("unreadable, using defaults: {error:#}"),
                started,
            ),
        },
        Err(error) => CheckResult::new(
            "configuration",
            CheckState::Warn,
            format!("cannot locate settings: {error:#}"),
            started,
        ),
    }
}

fn check_runbooks() -> CheckResult {
    let started = Instant::now();
    let user_dir = ork_core::Config::default_path()
        .ok()
        .and_then(|path| path.parent().map(|dir| dir.join("runbooks")));

    match ork_ai::runbook::RunbookLibrary::load(user_dir.as_deref()) {
        Ok(library) if library.is_empty() => CheckResult::new(
            "runbook library",
            CheckState::Fail,
            "the library is empty",
            started,
        ),
        Ok(library) => CheckResult::new(
            "runbook library",
            CheckState::Pass,
            format!("{} entries", library.len()),
            started,
        ),
        Err(error) => CheckResult::new(
            "runbook library",
            CheckState::Fail,
            format!("{error:#}"),
            started,
        ),
    }
}

fn check_state_store() -> CheckResult {
    let started = Instant::now();
    let path = match ork_core::Config::default_path() {
        Ok(path) => path.with_file_name("state.db"),
        Err(error) => {
            return CheckResult::new(
                "state database",
                CheckState::Warn,
                format!("cannot locate: {error:#}"),
                started,
            );
        }
    };

    match ork_fix::store::FixStore::open(&path) {
        Ok(store) => match store.pending() {
            Ok(pending) => CheckResult::new(
                "state database",
                CheckState::Pass,
                format!("{} item(s) in the triage queue", pending.len()),
                started,
            ),
            // The file opened but its contents are unreadable. The audit trail
            // is what is at risk, so this is a failure.
            Err(error) => CheckResult::new(
                "state database",
                CheckState::Fail,
                format!("opened but unreadable: {error:#}"),
                started,
            ),
        },
        Err(error) => CheckResult::new(
            "state database",
            CheckState::Fail,
            format!("{error:#}"),
            started,
        ),
    }
}

fn check_snapshot_area() -> CheckResult {
    let started = Instant::now();
    let dir = match ork_core::Config::default_path() {
        Ok(path) => path.with_file_name("snapshots"),
        Err(error) => {
            return CheckResult::new(
                "snapshot area",
                CheckState::Warn,
                format!("cannot locate: {error:#}"),
                started,
            );
        }
    };

    // Actually write something. A directory that exists but cannot be written
    // to means rollback failing silently at the worst possible moment.
    let probe = dir.join(".write-test");
    let writable = std::fs::create_dir_all(&dir)
        .and_then(|_| std::fs::write(&probe, b"ok"))
        .and_then(|_| std::fs::remove_file(&probe));

    match writable {
        Ok(()) => {
            let support = ork_fix::snapshot::detect_system_snapshot_support();
            let note = if support.available {
                "writable; a system-level snapshot tool is also present"
            } else {
                "writable; no system-level snapshot tool found"
            };
            CheckResult::new("snapshot area", CheckState::Pass, note, started)
        }
        // Without this, "roll back on failure" is not a promise the tool can
        // keep, so it is not reported as a mere inconvenience.
        Err(error) => CheckResult::new(
            "snapshot area",
            CheckState::Fail,
            format!("cannot write backups, so changes could not be undone: {error}"),
            started,
        ),
    }
}

/// Run every self-check in order, reporting each one as it completes.
///
/// The callback receives the result, its 1-based index, and the total, which
/// is what a progress bar and a rolling log pane both need. Checks are run in
/// order rather than in parallel: the later ones are cheap, and a boot screen
/// that reports out of order reads as broken.
pub fn run(mut on_result: impl FnMut(&CheckResult, usize, usize)) -> SelfTestReport {
    let started = Instant::now();
    let checks: [fn() -> CheckResult; CHECK_COUNT] = [
        check_platform,
        check_probes,
        check_configuration,
        check_runbooks,
        check_state_store,
        check_snapshot_area,
    ];

    let mut results = Vec::with_capacity(CHECK_COUNT);
    for (index, check) in checks.into_iter().enumerate() {
        let result = check();
        on_result(&result, index + 1, CHECK_COUNT);
        results.push(result);
    }

    SelfTestReport {
        checks: results,
        duration: started.elapsed(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_check_count_matches_what_run_actually_runs() {
        // A progress bar sized from CHECK_COUNT would either stall short of the
        // end or overflow if these ever drifted apart.
        let report = run(|_, _, _| {});
        assert_eq!(report.checks.len(), CHECK_COUNT);
    }

    #[test]
    fn every_check_reports_progress_exactly_once_in_order() {
        let mut seen = Vec::new();
        let report = run(|result, index, total| {
            assert_eq!(total, CHECK_COUNT);
            seen.push((index, result.name.clone()));
        });

        let names: Vec<&str> = report
            .checks
            .iter()
            .map(|check| check.name.as_str())
            .collect();
        let reported: Vec<&str> = seen.iter().map(|(_, name)| name.as_str()).collect();
        assert_eq!(names, reported);
        assert_eq!(
            seen.iter().map(|(index, _)| *index).collect::<Vec<_>>(),
            (1..=CHECK_COUNT).collect::<Vec<_>>()
        );
    }

    #[test]
    fn the_platform_and_probe_checks_pass_on_this_machine() {
        // These two have no external dependencies at all -- if they fail, the
        // build itself is wrong, not the machine it is running on.
        assert_eq!(check_platform().state, CheckState::Pass);
        assert_eq!(check_probes().state, CheckState::Pass);
    }

    #[test]
    fn the_worst_state_is_what_decides_whether_the_run_passed() {
        let mut report = SelfTestReport {
            checks: Vec::new(),
            duration: Duration::ZERO,
        };
        assert_eq!(report.worst(), CheckState::Pass);
        assert!(report.passed());

        report
            .checks
            .push(CheckResult::new("a", CheckState::Warn, "", Instant::now()));
        assert_eq!(report.worst(), CheckState::Warn);
        assert!(
            report.passed(),
            "a warning must not stop the tool from starting"
        );
        assert_eq!(report.warnings().count(), 1);

        report
            .checks
            .push(CheckResult::new("b", CheckState::Fail, "", Instant::now()));
        assert_eq!(report.worst(), CheckState::Fail);
        assert!(!report.passed());
        assert_eq!(report.failures().count(), 1);
    }
}
