//! Testing whether an application actually starts.
//!
//! The probe catalogue in [`crate::probes`] only tests programs that can be
//! asked to report themselves and exit -- `git --version` and friends. That
//! rule keeps a scan from opening windows all over somebody's desktop, and it
//! is a good rule, but it excludes precisely the applications people complain
//! about: launchers, clients, anything with a user interface. Steam is the
//! case this was written for.
//!
//! So testing one of those means actually starting it and watching what
//! happens. Three things can be observed, and only one of them is good news:
//!
//! * it exits quickly with an error -- broken, and the error says why;
//! * it exits quickly with success -- ambiguous, because a launcher that hands
//!   off to an instance already running also exits zero;
//! * it stays up -- it started.
//!
//! **Nothing is ever killed for being slow.** The wait here is not a deadline
//! on the work: the program is never terminated for taking too long, and a
//! program still running when the window ends has *passed*. The window is how
//! long to keep watching for a failure, not how long the program may take.
//!
//! **Only a process this module started is ever stopped.** If the application
//! was already running beforehand, the test does not run at all -- it would
//! prove nothing, and the honest answer is to say so.
//!
//! This lives in the core, not in the fix layer, so that the check which finds
//! a broken application and the check which later declares it repaired are the
//! same code. If those two ever drifted apart, "fixed" would quietly come to
//! mean something other than "not found any more".

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::PlatformKind;
use async_trait::async_trait;

/// How long to keep watching for a failure before concluding it started.
///
/// A launcher that is going to fall over does it almost immediately -- a
/// missing library, a stale lock, a broken runtime all fail at start-up. Ten
/// seconds of *not* failing is meaningful evidence. Nothing is killed at the
/// end of it; the program has simply passed.
const SETTLE: Duration = Duration::from_secs(10);

/// How often to check whether it has exited.
const POLL: Duration = Duration::from_millis(250);

/// How to stop something this module started.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shutdown {
    /// Ask it to quit properly, the way its own interface would.
    Politely {
        program: &'static str,
        args: &'static [&'static str],
    },
    /// Stop the process we started, and only that one.
    OurProcess,
}

/// An application whose launch can be tested.
#[derive(Debug, Clone, Copy)]
pub struct LaunchTarget {
    /// Stable slug, matched against a finding's subject.
    pub id: &'static str,
    pub name: &'static str,
    /// Executable names to look for, in order of preference.
    pub executables: &'static [&'static str],
    /// Arguments to start it with. Empty means "however it normally starts".
    pub launch_args: &'static [&'static str],
    /// Process names that mean it is running, for telling a fresh start from
    /// a hand-off to an instance that was already there.
    pub process_names: &'static [&'static str],
    pub shutdown: Shutdown,
    pub platforms: &'static [PlatformKind],
}

impl LaunchTarget {
    pub fn runs_on(&self, platform: PlatformKind) -> bool {
        self.platforms.contains(&platform)
    }
}

/// Applications this build can test by launching.
///
/// Short on purpose. Every entry is an application whose launch failure is a
/// real complaint people have, and whose start-up can be observed without
/// changing anything. Adding one is a promise that the tool can tell whether
/// it worked.
pub const LAUNCHERS: &[LaunchTarget] = &[LaunchTarget {
    id: "steam",
    name: "Steam",
    executables: &["steam", "steam.exe"],
    launch_args: &[],
    // The wrapper script exits almost at once on Linux while the real client
    // carries on, so "did the wrapper exit" is not the same question as "is
    // Steam running". These names answer the second one.
    process_names: &["steam", "steamwebhelper", "steam.exe"],
    // Steam has its own way of being asked to quit, which closes it the way
    // choosing Exit from its menu would.
    shutdown: Shutdown::Politely {
        program: "steam",
        args: &["-shutdown"],
    },
    platforms: &[PlatformKind::Linux, PlatformKind::Windows],
}];

/// What was observed when the application was started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchResult {
    /// It started and stayed up.
    Started,
    /// It exited with a failure. The output usually names the cause.
    Failed { code: Option<i32>, output: String },
    /// It exited successfully and straight away, and nothing is running --
    /// which for a launcher tells us very little.
    ExitedImmediately { code: i32 },
    /// It was already running before the test, so starting it proves nothing.
    AlreadyRunning,
    /// It is not installed, or not on the PATH.
    NotFound,
    /// The test itself could not be carried out.
    CouldNotTest { reason: String },
}

/// Starts an application and reports what happened.
///
/// Behind a trait so the decision-making below can be tested without starting
/// real programs on somebody's desktop.
#[async_trait]
pub trait LaunchTester: Send + Sync {
    async fn test(&self, target: &LaunchTarget) -> LaunchResult;
}

/// The real thing: starts the application and watches it.
#[derive(Debug, Clone)]
pub struct RealLaunchTester {
    settle: Duration,
}

impl Default for RealLaunchTester {
    fn default() -> Self {
        Self { settle: SETTLE }
    }
}

impl RealLaunchTester {
    pub fn with_settle(settle: Duration) -> Self {
        Self { settle }
    }

    /// Whether any of the target's processes are running right now.
    fn running(target: &LaunchTarget) -> Option<bool> {
        let platform = crate::platform::detect().ok()?;
        let processes = platform.processes().ok()?;
        Some(processes.iter().any(|process| {
            let name = process.name.to_ascii_lowercase();
            target
                .process_names
                .iter()
                .any(|wanted| name == *wanted || name == format!("{wanted}.exe"))
        }))
    }

    fn locate(target: &LaunchTarget) -> Option<String> {
        let platform = crate::platform::detect().ok()?;
        target
            .executables
            .iter()
            .find_map(|name| platform.locate_tool(name).map(|_| (*name).to_string()))
    }

    /// Ask an application that is running on its own -- not as our child --
    /// to close, having been started by this test.
    fn stop_detached(target: &LaunchTarget) {
        if let Shutdown::Politely { program, args } = target.shutdown {
            let _ = Command::new(program)
                .args(args)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    }

    /// Stop what we started, and nothing else.
    fn stop(target: &LaunchTarget, child: &mut std::process::Child) {
        match target.shutdown {
            Shutdown::Politely { program, args } => {
                let asked = Command::new(program)
                    .args(args)
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
                match asked {
                    Ok(status) if status.success() => {
                        // It is closing itself. Reap our own handle so no
                        // zombie is left behind.
                        let _ = child.wait();
                        return;
                    }
                    Ok(_) | Err(_) => {
                        tracing::debug!(
                            target = target.id,
                            "asking it to quit did not work; stopping the process we started"
                        );
                    }
                }
                let _ = child.kill();
                let _ = child.wait();
            }
            Shutdown::OurProcess => {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
}

#[async_trait]
impl LaunchTester for RealLaunchTester {
    async fn test(&self, target: &LaunchTarget) -> LaunchResult {
        // Asked first, because if it is already up then starting it again
        // tells us nothing at all -- and we must not stop an instance the
        // person is using.
        match Self::running(target) {
            Some(true) => return LaunchResult::AlreadyRunning,
            None => {
                return LaunchResult::CouldNotTest {
                    reason: "could not read the list of running programs".to_string(),
                };
            }
            Some(false) => {}
        }

        let Some(program) = Self::locate(target) else {
            return LaunchResult::NotFound;
        };

        let settle = self.settle;
        let target = *target;
        let program_for_task = program.clone();

        // Blocking process work, kept off the async executor.
        let observed = tokio::task::spawn_blocking(move || {
            let mut child = match Command::new(&program_for_task)
                .args(target.launch_args)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
            {
                Ok(child) => child,
                Err(error) => {
                    return LaunchResult::CouldNotTest {
                        reason: format!("{error}"),
                    };
                }
            };

            let started = Instant::now();
            loop {
                match child.try_wait() {
                    Ok(Some(status)) => {
                        let output = read_output(&mut child);
                        if !status.success() {
                            return LaunchResult::Failed {
                                code: status.code(),
                                output,
                            };
                        }
                        // Exited cleanly. On Linux the launcher script does
                        // this while the real client carries on, so the
                        // question that matters is whether it is running now.
                        return match RealLaunchTester::running(&target) {
                            Some(true) => {
                                // The wrapper handed off to a real client that
                                // this test caused to start, so this test is
                                // what closes it again.
                                RealLaunchTester::stop_detached(&target);
                                LaunchResult::Started
                            }
                            _ => LaunchResult::ExitedImmediately {
                                code: status.code().unwrap_or(0),
                            },
                        };
                    }
                    Ok(None) => {
                        if started.elapsed() >= settle {
                            // Still up. It started. Nothing is killed for
                            // being slow -- this is a pass, not a timeout, and
                            // the shutdown below is tidying up after a
                            // successful test, not cutting it short.
                            RealLaunchTester::stop(&target, &mut child);
                            return LaunchResult::Started;
                        }
                        std::thread::sleep(POLL);
                    }
                    Err(error) => {
                        let _ = child.kill();
                        return LaunchResult::CouldNotTest {
                            reason: format!("{error}"),
                        };
                    }
                }
            }
        })
        .await;

        // The child never outlives the task that owns it: every path out of
        // the block above has already stopped what it started.
        match observed {
            Ok(result) => result,
            Err(error) => LaunchResult::CouldNotTest {
                reason: format!("{error}"),
            },
        }
    }
}

fn read_output(child: &mut std::process::Child) -> String {
    use std::io::Read;
    let mut text = String::new();
    if let Some(mut stderr) = child.stderr.take() {
        let _ = stderr.read_to_string(&mut text);
    }
    if text.trim().is_empty()
        && let Some(mut stdout) = child.stdout.take()
    {
        let _ = stdout.read_to_string(&mut text);
    }
    text
}
