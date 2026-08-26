//! Running an external program under a liveness check.
//!
//! This exists because of a rule that runs through the whole tool: nothing is
//! given a deadline. A test that legitimately takes six hours is allowed to
//! take six hours. What is *not* allowed is a process that has silently died
//! on its feet, holding the scan open forever.
//!
//! The distinction is liveness, not elapsed time. A process is considered
//! alive while it is doing anything at all -- burning CPU, reading or writing
//! disk, or producing output. Only when it does none of those for a sustained
//! window is it declared stuck, and the caller is told that is what happened
//! rather than being handed a bare timeout. The user's cancellation always
//! takes effect immediately, whatever the process is doing.

use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::Context;
use sysinfo::{MINIMUM_CPU_UPDATE_INTERVAL, Pid, ProcessRefreshKind, ProcessesToUpdate, System};

use crate::Result;

/// When to conclude that a running process is stuck rather than working.
#[derive(Debug, Clone, Copy)]
pub struct LivenessPolicy {
    /// How long a process may show no activity of any kind before it is
    /// treated as hung.
    ///
    /// This is not a time limit on the work. A process that keeps doing
    /// something resets this window every time it does, and may run forever.
    pub stall_window: Duration,
    /// How often to look.
    ///
    /// Raised to [`LivenessPolicy::minimum_poll_interval`] in practice. See
    /// that function for why a shorter one is worse than useless.
    pub poll_interval: Duration,
}

impl Default for LivenessPolicy {
    fn default() -> Self {
        Self {
            // Thirty seconds of total silence -- no CPU, no disk, no output --
            // is a long time for a program that is genuinely working.
            stall_window: Duration::from_secs(30),
            poll_interval: Duration::from_millis(500),
        }
    }
}

impl LivenessPolicy {
    /// The shortest gap between looks that still measures CPU.
    ///
    /// Process CPU use is a rate, worked out from the difference between two
    /// readings. Ask again too soon and the underlying counters have not moved
    /// enough to say anything, so a process pinned at full tilt reads as
    /// perfectly idle -- and the supervisor concludes that the busiest process
    /// on the machine is stuck. That is the precise opposite of what a
    /// liveness check is for, so the interval is raised rather than obeyed.
    pub fn minimum_poll_interval() -> Duration {
        MINIMUM_CPU_UPDATE_INTERVAL
    }

    /// How often this policy will actually look.
    pub fn effective_poll_interval(&self) -> Duration {
        self.poll_interval.max(Self::minimum_poll_interval())
    }
}

/// How a supervised process ended.
#[derive(Debug, Clone)]
pub enum ExecOutcome {
    /// The process finished on its own.
    Exited {
        code: Option<i32>,
        stdout: String,
        stderr: String,
        duration: Duration,
    },
    /// The process stopped doing anything and was terminated.
    ///
    /// This is a finding in its own right, not an error: a program that hangs
    /// on launch is exactly the kind of fault this tool exists to catch.
    Stalled {
        stdout: String,
        stderr: String,
        idle: Duration,
        duration: Duration,
    },
    /// The user cancelled. The process was terminated.
    Cancelled {
        stdout: String,
        stderr: String,
        duration: Duration,
    },
}

impl ExecOutcome {
    /// Whether the process ran and exited successfully.
    pub fn succeeded(&self) -> bool {
        matches!(self, ExecOutcome::Exited { code: Some(0), .. })
    }

    pub fn stdout(&self) -> &str {
        match self {
            ExecOutcome::Exited { stdout, .. }
            | ExecOutcome::Stalled { stdout, .. }
            | ExecOutcome::Cancelled { stdout, .. } => stdout,
        }
    }

    pub fn stderr(&self) -> &str {
        match self {
            ExecOutcome::Exited { stderr, .. }
            | ExecOutcome::Stalled { stderr, .. }
            | ExecOutcome::Cancelled { stderr, .. } => stderr,
        }
    }
}

/// Shared buffer that a reader thread appends into.
#[derive(Default)]
struct Sink {
    text: Mutex<String>,
    bytes: AtomicU64,
}

impl Sink {
    fn drain_into(self: &Arc<Self>, stream: impl std::io::Read + Send + 'static) {
        let sink = Arc::clone(self);
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stream);
            let mut line = String::new();
            // read_line on a lossy reader can fail on invalid UTF-8; stopping
            // there loses the rest of the output, so read bytes and convert.
            let mut raw = Vec::new();
            while let Ok(count) = reader.read_until(b'\n', &mut raw) {
                if count == 0 {
                    break;
                }
                line.clear();
                line.push_str(&String::from_utf8_lossy(&raw));
                raw.clear();
                sink.bytes.fetch_add(count as u64, Ordering::Relaxed);
                if let Ok(mut text) = sink.text.lock() {
                    text.push_str(&line);
                }
            }
        });
    }

    fn snapshot(&self) -> String {
        self.text
            .lock()
            .map(|text| text.clone())
            .unwrap_or_default()
    }
}

/// A single reading of how much work a process has done in total.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct ActivitySample {
    /// Bytes of output produced so far.
    output_bytes: u64,
    /// Bytes read from and written to disk so far.
    io_bytes: u64,
    /// Whether the process was using measurable CPU at this instant.
    burning_cpu: bool,
    /// Milliseconds of processor time this process has consumed in total.
    ///
    /// A counter, not a rate, and that distinction is the whole reason it is
    /// here. See [`ActivitySample::differs_from`].
    cpu_time_ms: u64,
}

impl ActivitySample {
    /// Whether anything has happened since `previous`.
    ///
    /// Four signals, and the order they are written in is the order they are
    /// trusted in.
    ///
    /// **Consumed processor time** is the one that carries the weight. It is a
    /// counter that only goes up, so any increase at all is proof the process
    /// ran -- one millisecond is enough. That matters because the other CPU
    /// signal is a *rate*, and a rate is exactly what stops being measurable
    /// on a machine under load: a process spinning flat out on a computer that
    /// is thrashing may be scheduled so little that its rate rounds to nothing.
    /// A diagnostic tool killing a repair because the machine was too busy to
    /// report on it would fail at the one moment it is most needed. This was
    /// found by a supervised process being declared stuck on a loaded build
    /// machine while it sat in a tight loop, having last shown a measurable
    /// rate two seconds earlier.
    ///
    /// **CPU as a level** stays as a second opinion, checked as a level rather
    /// than a delta because a process pinned at 100% shows the same value every
    /// time we look, and treating "unchanged" as "idle" would declare the
    /// busiest possible process stuck.
    ///
    /// **Output** and **disk** are the signals for a process that is waiting on
    /// something rather than computing.
    ///
    /// Every one of them is evidence of life, so any one is enough. Nothing is
    /// ever declared stuck for failing several tests -- only for failing all of
    /// them.
    fn differs_from(&self, previous: &ActivitySample) -> bool {
        self.cpu_time_ms > previous.cpu_time_ms
            || self.burning_cpu
            || self.output_bytes != previous.output_bytes
            || self.io_bytes != previous.io_bytes
    }
}

/// Threshold below which CPU use is indistinguishable from measurement noise.
const CPU_NOISE_FLOOR: f32 = 0.5;

fn sample(system: &mut System, pid: Pid, output_bytes: u64) -> ActivitySample {
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[pid]),
        true,
        ProcessRefreshKind::nothing().with_cpu().with_disk_usage(),
    );

    match system.process(pid) {
        Some(process) => {
            let disk = process.disk_usage();
            ActivitySample {
                output_bytes,
                io_bytes: disk
                    .total_read_bytes
                    .saturating_add(disk.total_written_bytes),
                burning_cpu: process.cpu_usage() > CPU_NOISE_FLOOR,
                cpu_time_ms: process.accumulated_cpu_time(),
            }
        }
        // The process is gone, or we cannot see it. Either way this poll tells
        // us nothing, so report no activity and let try_wait decide.
        None => ActivitySample {
            output_bytes,
            ..Default::default()
        },
    }
}

fn terminate(child: &mut Child) {
    if let Err(error) = child.kill() {
        tracing::debug!(%error, "could not terminate supervised process");
    }
    let _ = child.wait();
}

/// Run a program, supervising it for liveness rather than for elapsed time.
///
/// This blocks. Call it from a blocking context.
pub fn run_supervised(
    program: &str,
    args: &[&str],
    policy: LivenessPolicy,
    cancel: &tokio_util::sync::CancellationToken,
) -> Result<ExecOutcome> {
    let clock = Instant::now();
    tracing::debug!(program, ?args, "starting supervised process");

    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("could not start `{program}`"))?;

    let out = Arc::new(Sink::default());
    let err = Arc::new(Sink::default());
    if let Some(stream) = child.stdout.take() {
        out.drain_into(stream);
    }
    if let Some(stream) = child.stderr.take() {
        err.drain_into(stream);
    }

    let pid = Pid::from_u32(child.id());
    let mut system = System::new();
    let mut previous = ActivitySample::default();
    let mut last_activity = Instant::now();

    loop {
        if cancel.is_cancelled() {
            terminate(&mut child);
            return Ok(ExecOutcome::Cancelled {
                stdout: out.snapshot(),
                stderr: err.snapshot(),
                duration: clock.elapsed(),
            });
        }

        if let Some(status) = child.try_wait()? {
            // Give the reader threads a moment to finish draining the pipes,
            // or the last lines of output -- often the interesting ones -- are
            // lost to the race.
            std::thread::sleep(Duration::from_millis(50));
            return Ok(ExecOutcome::Exited {
                code: status.code(),
                stdout: out.snapshot(),
                stderr: err.snapshot(),
                duration: clock.elapsed(),
            });
        }

        let current = sample(
            &mut system,
            pid,
            out.bytes.load(Ordering::Relaxed) + err.bytes.load(Ordering::Relaxed),
        );
        if current.differs_from(&previous) {
            last_activity = Instant::now();
        }
        previous = current;

        let idle = last_activity.elapsed();
        if idle >= policy.stall_window {
            tracing::debug!(
                program,
                ?idle,
                "process showed no activity; treating as stuck"
            );
            terminate(&mut child);
            return Ok(ExecOutcome::Stalled {
                stdout: out.snapshot(),
                stderr: err.snapshot(),
                idle,
                duration: clock.elapsed(),
            });
        }

        std::thread::sleep(policy.effective_poll_interval());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio_util::sync::CancellationToken;

    /// A command that exits immediately, on whichever platform runs the tests.
    fn quick_command() -> (&'static str, Vec<&'static str>) {
        if cfg!(windows) {
            ("cmd", vec!["/C", "echo hello"])
        } else {
            ("sh", vec!["-c", "echo hello"])
        }
    }

    /// A command that sleeps without doing anything -- the shape of a hang.
    fn idle_command() -> (&'static str, Vec<&'static str>) {
        if cfg!(windows) {
            ("cmd", vec!["/C", "ping -n 20 127.0.0.1 > nul"])
        } else {
            ("sh", vec!["-c", "sleep 20"])
        }
    }

    /// A command that stays busy for a fixed number of seconds without
    /// producing any output.
    ///
    /// This is deliberately wall-clock bound rather than a fixed iteration
    /// count. An iteration count runs for however long the machine takes,
    /// which on a fast CI runner can finish before the stall window it is
    /// supposed to outlast -- leaving a test that passes without testing
    /// anything.
    fn busy_command(seconds: u32) -> (&'static str, Vec<String>) {
        if cfg!(windows) {
            (
                "powershell",
                vec![
                    "-NoProfile".to_string(),
                    "-NonInteractive".to_string(),
                    "-Command".to_string(),
                    format!(
                        "$end=(Get-Date).AddSeconds({seconds}); while((Get-Date) -lt $end){{}}"
                    ),
                ],
            )
        } else {
            (
                "sh",
                vec![
                    "-c".to_string(),
                    format!(
                        "end=$(($(date +%s)+{seconds})); while [ \"$(date +%s)\" -lt \"$end\" ]; do :; done"
                    ),
                ],
            )
        }
    }

    #[test]
    fn a_command_that_exits_reports_its_output_and_code() {
        let (program, args) = quick_command();
        let outcome = run_supervised(
            program,
            &args,
            LivenessPolicy::default(),
            &CancellationToken::new(),
        )
        .expect("command should run");

        assert!(outcome.succeeded(), "expected success, got {outcome:?}");
        assert!(
            outcome.stdout().contains("hello"),
            "stdout was {:?}",
            outcome.stdout()
        );
    }

    #[test]
    fn a_process_doing_nothing_is_declared_stuck() {
        let (program, args) = idle_command();
        let policy = LivenessPolicy {
            stall_window: Duration::from_secs(2),
            poll_interval: Duration::from_millis(100),
        };
        let outcome = run_supervised(program, &args, policy, &CancellationToken::new())
            .expect("command should run");

        match outcome {
            ExecOutcome::Stalled { idle, .. } => {
                assert!(idle >= Duration::from_secs(2), "idle was {idle:?}");
            }
            other => panic!("expected the idle process to be declared stuck, got {other:?}"),
        }
    }

    #[test]
    fn an_impatient_poll_interval_is_raised_to_one_that_can_see_cpu() {
        // Asking too often reads every process as idle, which would make the
        // supervisor kill exactly the work it exists to protect.
        let impatient = LivenessPolicy {
            stall_window: Duration::from_secs(2),
            poll_interval: Duration::from_millis(1),
        };
        assert_eq!(
            impatient.effective_poll_interval(),
            LivenessPolicy::minimum_poll_interval()
        );

        let patient = LivenessPolicy {
            stall_window: Duration::from_secs(30),
            poll_interval: Duration::from_secs(1),
        };
        assert_eq!(patient.effective_poll_interval(), Duration::from_secs(1));
        assert!(
            LivenessPolicy::default().poll_interval >= LivenessPolicy::minimum_poll_interval(),
            "the default policy must not need raising"
        );
    }

    #[test]
    fn a_process_doing_real_work_is_never_declared_stuck() {
        // This is the whole point of a liveness check rather than a timeout.
        // The process produces no output at all and runs for twice the stall
        // window, but it is busy, so it must be allowed to finish.
        const STALL_SECS: u64 = 2;
        const WORK_SECS: u32 = 4;

        let (program, args) = busy_command(WORK_SECS);
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let policy = LivenessPolicy {
            stall_window: Duration::from_secs(STALL_SECS),
            poll_interval: Duration::from_millis(100),
        };

        let started = Instant::now();
        let outcome = run_supervised(program, &args, policy, &CancellationToken::new())
            .expect("command should run");

        assert!(
            matches!(outcome, ExecOutcome::Exited { .. }),
            "a busy process must not be treated as stuck, got {outcome:?}"
        );
        // Guard against the test silently proving nothing: the work has to
        // have actually outlasted the stall window.
        assert!(
            started.elapsed() > Duration::from_secs(STALL_SECS),
            "the workload finished in {:?}, which is inside the stall window, so this run              did not test anything",
            started.elapsed()
        );
    }

    #[test]
    fn cancellation_takes_effect_promptly() {
        let (program, args) = idle_command();
        let cancel = CancellationToken::new();
        let trigger = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            trigger.cancel();
        });

        let started = Instant::now();
        let outcome = run_supervised(
            program,
            &args,
            LivenessPolicy {
                stall_window: Duration::from_secs(600),
                ..Default::default()
            },
            &cancel,
        )
        .expect("command should run");

        assert!(
            matches!(outcome, ExecOutcome::Cancelled { .. }),
            "got {outcome:?}"
        );
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "cancellation was not prompt"
        );
    }

    #[test]
    fn a_missing_program_is_an_error_not_a_stall() {
        let result = run_supervised(
            "ork-definitely-not-a-real-program",
            &[],
            LivenessPolicy::default(),
            &CancellationToken::new(),
        );
        assert!(result.is_err());
    }

    #[test]
    fn a_pinned_process_is_not_mistaken_for_an_idle_one() {
        // CPU is sampled as a rate, so a process at a constant 100% reports an
        // unchanged value every poll. Treating "unchanged" as "idle" would
        // declare the busiest possible process stuck.
        let busy = ActivitySample {
            output_bytes: 0,
            io_bytes: 0,
            burning_cpu: true,
            cpu_time_ms: 0,
        };
        assert!(busy.differs_from(&busy));

        let idle = ActivitySample {
            output_bytes: 0,
            io_bytes: 0,
            burning_cpu: false,
            cpu_time_ms: 0,
        };
        assert!(!idle.differs_from(&idle));
    }

    #[test]
    fn a_process_too_starved_to_register_a_rate_is_still_alive() {
        // The failure this exists to prevent, seen for real: a process sitting
        // in a tight loop was declared stuck on a loaded build machine,
        // because on a computer that is thrashing a spinning process can be
        // scheduled so little that its CPU *rate* rounds to nothing. That is
        // the exact condition this tool runs in, so the rate cannot be the
        // only thing asked.
        //
        // Consumed processor time is a counter rather than a rate. One
        // millisecond of it is proof the process ran, at any load.
        let before = ActivitySample {
            output_bytes: 0,
            io_bytes: 0,
            burning_cpu: false,
            cpu_time_ms: 1_000,
        };
        let after = ActivitySample {
            cpu_time_ms: 1_001,
            ..before
        };
        assert!(
            after.differs_from(&before),
            "a millisecond of processor time is evidence of life"
        );

        // And a counter that has not moved says nothing either way, so the
        // other signals still decide.
        let unmoved = ActivitySample { ..before };
        assert!(!unmoved.differs_from(&before));
    }

    #[test]
    fn a_counter_that_goes_backwards_is_not_read_as_activity() {
        // Which it should never do -- but it is read from the operating system
        // across process restarts and reused identifiers, and `!=` would turn
        // a decrease into a liveness signal.
        let before = ActivitySample {
            output_bytes: 0,
            io_bytes: 0,
            burning_cpu: false,
            cpu_time_ms: 5_000,
        };
        let after = ActivitySample {
            cpu_time_ms: 10,
            ..before
        };
        assert!(!after.differs_from(&before));
    }

    #[test]
    fn output_alone_counts_as_activity() {
        let before = ActivitySample {
            output_bytes: 10,
            io_bytes: 0,
            burning_cpu: false,
            cpu_time_ms: 0,
        };
        let after = ActivitySample {
            output_bytes: 20,
            ..before
        };
        assert!(after.differs_from(&before));
    }
}
