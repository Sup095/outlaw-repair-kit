//! Stopping a process, and saying honestly what happened.
//!
//! Separate from the classifier next door on purpose. `standing.rs` decides
//! whether something *may* be stopped and never touches anything;
//! this does the touching and decides nothing. Keeping them apart means the
//! judgement can be read, tested and argued about without going anywhere near
//! the code that ends a program.
//!
//! Stage three of `docs/proposals/process-control.md`. The orchestration --
//! judging each target again at the moment of acting, one at a time, and
//! writing every attempt down -- lives in `ork-fix`, which is the crate that
//! exists for changes to the machine. This is only the asking.

use sysinfo::System;

/// What came of asking one process to stop.
///
/// Four answers rather than two, because "it did not work" and "it was never
/// there" and "it is still there" call for different things to be said, and a
/// screen that showed one sentence for all three would be guessing on the
/// person's behalf about what just happened to their machine.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case", tag = "outcome")]
pub enum Stopping {
    /// It is no longer running.
    Stopped,
    /// It had already ended before it was asked. Between a list being drawn
    /// and a button being pressed, programs finish on their own.
    AlreadyGone,
    /// It was asked and it is still there.
    ///
    /// Nothing escalates from here. A second, harder attempt is exactly the
    /// behaviour that loses somebody their unsaved work, and the tool would be
    /// making that trade silently on their behalf. It says what happened
    /// instead.
    StillRunning,
    /// The operating system would not.
    Refused { because: String },
}

/// How long to watch before saying a process is still running.
///
/// Not a limit on the work -- the work is over the instant the request is
/// made. This is how long to keep looking before reporting what was seen, and
/// what it buys is the difference between "still running" and "we did not wait
/// long enough to notice".
const WATCH_FOR: std::time::Duration = std::time::Duration::from_secs(3);
const LOOK_EVERY: std::time::Duration = std::time::Duration::from_millis(100);

/// Ask a process to stop, and say what happened.
///
/// **On Windows this is not a polite request.** There is no portable way to
/// ask a program to close itself as though somebody had clicked its close
/// button; the operating system call available here ends the process, and it
/// does not get to save. That is the reason anything that might hold unsaved
/// work is held back from being offered in the first place, and it is worth
/// saying on any screen that offers this rather than only here.
///
/// On Linux the polite signal is sent first, which a program may handle and
/// shut down cleanly. If it does not, it keeps running and this says so.
pub fn stop_process(pid: u32) -> Stopping {
    let target = sysinfo::Pid::from_u32(pid);
    let mut system = System::new();
    let refresh = sysinfo::ProcessRefreshKind::nothing();
    system.refresh_processes_specifics(sysinfo::ProcessesToUpdate::Some(&[target]), true, refresh);

    let Some(process) = system.process(target) else {
        return Stopping::AlreadyGone;
    };

    // The polite signal where one exists. `kill_with` answers `None` when the
    // platform has no such signal, which is Windows -- and there the fallback
    // is the only thing there is.
    let asked = match process.kill_with(sysinfo::Signal::Term) {
        Some(sent) => sent,
        None => process.kill(),
    };
    if !asked {
        return Stopping::Refused {
            because: "the operating system would not let this process be stopped".to_string(),
        };
    }

    // Watch rather than assume. `kill` reports that the request was made, not
    // that the process went, and reporting a request as a result is how a
    // screen ends up saying a program was stopped while it is still on the
    // taskbar.
    let until = std::time::Instant::now() + WATCH_FOR;
    while std::time::Instant::now() < until {
        std::thread::sleep(LOOK_EVERY);
        system.refresh_processes_specifics(
            sysinfo::ProcessesToUpdate::Some(&[target]),
            true,
            refresh,
        );
        if system.process(target).is_none() {
            return Stopping::Stopped;
        }
    }
    Stopping::StillRunning
}
