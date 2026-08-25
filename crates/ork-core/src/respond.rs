//! Asking an installed application whether it still works.
//!
//! The cheap version of [`crate::launch`]: instead of starting a program and
//! watching it, this asks programs that have a way of reporting themselves and
//! exiting -- `git --version` and its relatives. That rule is what lets this
//! run in a quick scan without opening windows all over somebody's desktop.
//!
//! Two limits on the table below, both deliberate:
//!
//! * Every entry must exit on its own within moments and change nothing. An
//!   application with no such invocation does not belong here; it belongs in
//!   [`crate::launch`], at the full tier, where the user has asked for
//!   something more intrusive.
//! * An application that is not installed is not a problem. A finding should
//!   describe something wrong, not something absent by choice.
//!
//! This lives in the core rather than in the probe that uses it, so that the
//! check which finds a broken application and the check which later declares
//! it repaired are the same code. A verifier that tests something subtly
//! different from what the scan tested would let "fixed" drift away from
//! meaning "not found any more", and that drift is invisible until somebody
//! trusts it.

use std::time::Duration;

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

use crate::PlatformKind;
use crate::exec::{ExecOutcome, LivenessPolicy, run_supervised};

/// An application that can be asked whether it works.
#[derive(Debug, Clone, Copy)]
pub struct AppDefinition {
    /// Stable slug, used in findings and runbook entries.
    pub id: &'static str,
    /// What a person calls it.
    pub name: &'static str,
    /// Executable names to look for on `PATH`, in order of preference.
    pub executables: &'static [&'static str],
    /// Arguments that make the program report itself and exit immediately.
    ///
    /// This must never start a user interface, open a window, modify anything,
    /// or wait for input.
    pub version_args: &'static [&'static str],
    pub platforms: &'static [PlatformKind],
}

impl AppDefinition {
    pub fn runs_on(&self, platform: PlatformKind) -> bool {
        self.platforms.contains(&platform)
    }
}

/// Applications with a known-safe way to ask "are you working?".
///
/// Kept short and conservative on purpose. Adding an entry here is a promise
/// that the tool can tell whether it worked.
pub const APPS: &[AppDefinition] = &[
    AppDefinition {
        id: "git",
        name: "Git",
        executables: &["git"],
        version_args: &["--version"],
        platforms: &[
            PlatformKind::Windows,
            PlatformKind::Linux,
            PlatformKind::MacOs,
        ],
    },
    AppDefinition {
        id: "python",
        name: "Python",
        executables: &["python3", "python"],
        version_args: &["--version"],
        platforms: &[
            PlatformKind::Windows,
            PlatformKind::Linux,
            PlatformKind::MacOs,
        ],
    },
    AppDefinition {
        id: "node",
        name: "Node.js",
        executables: &["node"],
        version_args: &["--version"],
        platforms: &[
            PlatformKind::Windows,
            PlatformKind::Linux,
            PlatformKind::MacOs,
        ],
    },
    AppDefinition {
        id: "firefox",
        name: "Firefox",
        executables: &["firefox"],
        version_args: &["--version"],
        platforms: &[PlatformKind::Windows, PlatformKind::Linux],
    },
    AppDefinition {
        id: "docker",
        name: "Docker",
        executables: &["docker"],
        version_args: &["--version"],
        platforms: &[
            PlatformKind::Windows,
            PlatformKind::Linux,
            PlatformKind::MacOs,
        ],
    },
    AppDefinition {
        id: "ffmpeg",
        name: "FFmpeg",
        executables: &["ffmpeg"],
        version_args: &["-version"],
        platforms: &[
            PlatformKind::Windows,
            PlatformKind::Linux,
            PlatformKind::MacOs,
        ],
    },
];

/// The application with this slug, if this build knows it.
pub fn find(id: &str) -> Option<&'static AppDefinition> {
    APPS.iter().find(|app| app.id == id)
}

/// What happened when the application was asked to report itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RespondResult {
    /// It answered normally. As close to "this installation is fine" as a
    /// question this cheap can get.
    Responds { executable: String },
    /// It ran and returned an error.
    Failed {
        executable: String,
        code: Option<i32>,
        stdout: String,
        stderr: String,
    },
    /// It never finished answering, and was stopped by the supervisor.
    ///
    /// Not a deadline: the supervisor stops a process that has gone quiet on
    /// every measure at once, not one that is merely taking a while.
    Hung { executable: String, idle: Duration },
    /// Not installed, or not on `PATH`.
    NotInstalled,
    /// The question could not be put. Says nothing about the application.
    CouldNotTest { reason: String },
}

/// Asks an application whether it works.
///
/// Behind a trait so the decisions made from the answer can be tested without
/// running real programs.
#[async_trait]
pub trait ResponseTester: Send + Sync {
    async fn test(&self, app: &AppDefinition) -> RespondResult;
}

/// The real thing: finds the program and runs it under the liveness
/// supervisor.
pub struct RealResponseTester {
    cancel: CancellationToken,
}

impl RealResponseTester {
    pub fn new(cancel: CancellationToken) -> Self {
        Self { cancel }
    }
}

impl Default for RealResponseTester {
    fn default() -> Self {
        Self::new(CancellationToken::new())
    }
}

#[async_trait]
impl ResponseTester for RealResponseTester {
    async fn test(&self, app: &AppDefinition) -> RespondResult {
        let platform = match crate::platform::detect() {
            Ok(platform) => platform,
            Err(error) => {
                return RespondResult::CouldNotTest {
                    reason: format!("this machine could not be read: {error:#}"),
                };
            }
        };

        let Some(executable) = app
            .executables
            .iter()
            .find_map(|name| platform.locate_tool(name))
        else {
            return RespondResult::NotInstalled;
        };
        let executable = executable.to_string_lossy().to_string();

        let args: Vec<String> = app.version_args.iter().map(|arg| arg.to_string()).collect();
        let cancel = self.cancel.clone();
        let program = executable.clone();
        let outcome = tokio::task::spawn_blocking(move || {
            let args: Vec<&str> = args.iter().map(String::as_str).collect();
            run_supervised(&program, &args, LivenessPolicy::default(), &cancel)
        })
        .await;

        match outcome {
            Ok(Ok(outcome)) => interpret(&executable, &outcome),
            // Being unable to run it at all is not the same as it being
            // broken, and the two must not be conflated.
            Ok(Err(error)) => RespondResult::CouldNotTest {
                reason: format!("{error:#}"),
            },
            Err(error) => RespondResult::CouldNotTest {
                reason: format!("the test could not be run: {error}"),
            },
        }
    }
}

/// What the supervisor's report means.
pub fn interpret(executable: &str, outcome: &ExecOutcome) -> RespondResult {
    match outcome {
        ExecOutcome::Exited { code: Some(0), .. } => RespondResult::Responds {
            executable: executable.to_string(),
        },
        // The user stopped it. That is not the application's fault and must
        // never be recorded as one.
        ExecOutcome::Cancelled { .. } => RespondResult::CouldNotTest {
            reason: "the test was stopped before it finished".to_string(),
        },
        ExecOutcome::Exited {
            code,
            stdout,
            stderr,
            ..
        } => RespondResult::Failed {
            executable: executable.to_string(),
            code: *code,
            stdout: stdout.clone(),
            stderr: stderr.clone(),
        },
        ExecOutcome::Stalled { idle, .. } => RespondResult::Hung {
            executable: executable.to_string(),
            idle: *idle,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exited(code: i32, stdout: &str, stderr: &str) -> ExecOutcome {
        ExecOutcome::Exited {
            code: Some(code),
            stdout: stdout.to_string(),
            stderr: stderr.to_string(),
            duration: Duration::from_millis(10),
        }
    }

    #[test]
    fn a_clean_answer_means_it_responds() {
        assert_eq!(
            interpret("/usr/bin/git", &exited(0, "git version 2.4", "")),
            RespondResult::Responds {
                executable: "/usr/bin/git".to_string()
            }
        );
    }

    #[test]
    fn an_error_carries_the_output_that_explains_it() {
        let result = interpret("/usr/bin/git", &exited(127, "", "libpcre.so.3: not found"));
        match result {
            RespondResult::Failed { code, stderr, .. } => {
                assert_eq!(code, Some(127));
                assert!(stderr.contains("libpcre"));
            }
            other => panic!("expected a failure, got {other:?}"),
        }
    }

    #[test]
    fn a_cancelled_test_is_never_blamed_on_the_application() {
        // The user stopped the scan. Reading that as a broken program would
        // manufacture a fault out of somebody pressing Stop.
        let cancelled = ExecOutcome::Cancelled {
            stdout: String::new(),
            stderr: String::new(),
            duration: Duration::from_millis(5),
        };
        assert!(matches!(
            interpret("/usr/bin/git", &cancelled),
            RespondResult::CouldNotTest { .. }
        ));
    }

    #[test]
    fn every_application_in_the_table_can_be_looked_up_by_its_slug() {
        // Findings carry the slug and verifiers look it up. A typo here would
        // mean the scan reports a problem nothing can ever re-test.
        for app in APPS {
            assert!(find(app.id).is_some(), "{} is not findable", app.id);
            assert!(
                !app.executables.is_empty() && !app.platforms.is_empty(),
                "{} would never be tested",
                app.id
            );
        }
    }

    #[test]
    fn no_two_applications_share_a_slug() {
        let mut ids: Vec<&str> = APPS.iter().map(|app| app.id).collect();
        let total = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), total, "two applications share a slug");
    }
}
