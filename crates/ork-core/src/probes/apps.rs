//! Do the installed applications actually run?
//!
//! "It won't launch any more" is one of the most common real complaints about
//! a computer, and one of the least visible to a scan that only looks at
//! hardware and logs. This probe catches it directly, by running installed
//! applications and seeing what happens.
//!
//! Two deliberate limits on how far it goes at this tier:
//!
//! * Only applications with a *safe* invocation are tested -- one that prints
//!   a version and exits. Starting a graphical application unannounced during
//!   a background scan would be obnoxious, so full launch testing belongs to
//!   the Full tier, where the user has asked for something more intrusive.
//! * Applications that are not installed are not mentioned at all. A finding
//!   should describe something wrong, not something absent by choice.
//!
//! Each launch runs under the liveness supervisor, so an application that
//! hangs on startup is reported as hung rather than quietly holding the scan
//! open forever.

use async_trait::async_trait;

use crate::Result;
use crate::exec::{ExecOutcome, LivenessPolicy, run_supervised};
use crate::finding::{Category, Finding, Severity, Triage};
use crate::platform::PlatformKind;
use crate::probe::{Probe, ProbeContext, ProbeMeta};
use crate::tier::ScanTier;

/// An application this probe knows how to test.
struct AppDefinition {
    /// Stable slug, used in findings and runbook entries.
    id: &'static str,
    /// What a person calls it.
    name: &'static str,
    /// Executable names to look for on `PATH`, in order of preference.
    executables: &'static [&'static str],
    /// Arguments that make the program report itself and exit immediately.
    ///
    /// This must never start a user interface, open a window, modify anything,
    /// or wait for input. If an application has no such invocation, it does
    /// not belong in this table.
    version_args: &'static [&'static str],
    platforms: &'static [PlatformKind],
}

/// Applications with a known-safe way to ask "are you working?".
///
/// Kept short and conservative on purpose. Every entry here is a program that
/// exits on its own within moments and touches nothing. Anything that needs a
/// real launch to be meaningfully tested -- games, launchers, graphical
/// applications -- is left to the Full tier.
const APPS: &[AppDefinition] = &[
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

/// Trim program output down to something readable in a report.
fn excerpt(text: &str, limit: usize) -> String {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.chars().count() > limit {
        format!("{}...", text.chars().take(limit).collect::<String>())
    } else {
        text
    }
}

/// Build a finding from an application that did not start cleanly.
fn launch_finding(app: &AppDefinition, executable: &str, outcome: &ExecOutcome) -> Option<Finding> {
    let (id, severity, title, detail, hint) = match outcome {
        ExecOutcome::Exited { code: Some(0), .. } => return None,
        // The user stopped the scan. That is not the application's fault and
        // must not be recorded as one.
        ExecOutcome::Cancelled { .. } => return None,
        ExecOutcome::Exited { code, .. } => (
            "app.launch-failed",
            Severity::High,
            format!("{} is installed but fails to start", app.name),
            format!(
                "Running `{executable}` returned an error instead of starting normally \
                 (exit code {}). Something about this installation is broken -- a missing \
                 dependency, a permissions problem, or a partially applied update are the \
                 usual causes.",
                code.map(|code| code.to_string())
                    .unwrap_or_else(|| "unknown".to_string())
            ),
            "Run the program from a terminal to see its full error, then check whether it \
             was affected by a recent update.",
        ),
        ExecOutcome::Stalled { idle, .. } => (
            "app.launch-hung",
            Severity::High,
            format!("{} hangs instead of starting", app.name),
            format!(
                "Running `{executable}` never finished starting. It stopped using the \
                 processor, stopped reading and writing, and produced no output for {} \
                 seconds, so it was stopped. A program that hangs this way is usually \
                 waiting on something that will never arrive -- a service that is not \
                 running, a network resource, or a lock held by another process.",
                idle.as_secs()
            ),
            "Check whether any service this program depends on is running, and look for a \
             stale lock file left behind by a previous crash.",
        ),
    };

    let mut builder = Finding::builder("apps.launch-check", id)
        .subject(app.id)
        .severity(severity)
        .category(Category::Application)
        .title(title)
        .detail(detail)
        .evidence("application", app.name)
        .evidence("executable", executable)
        .remediation_hint(hint)
        // An application that will not start rarely has one known cause, so
        // this is worked through the triage queue rather than fixed inline.
        .triage(Triage::Queue);

    let stdout = excerpt(outcome.stdout(), 400);
    if !stdout.is_empty() {
        builder = builder.evidence("stdout", stdout);
    }
    let stderr = excerpt(outcome.stderr(), 400);
    if !stderr.is_empty() {
        builder = builder.evidence("stderr", stderr);
    }

    Some(builder.build())
}

#[derive(Debug, Default)]
pub struct AppLaunchProbe;

#[async_trait]
impl Probe for AppLaunchProbe {
    fn meta(&self) -> ProbeMeta {
        ProbeMeta {
            id: "apps.launch-check",
            name: "Application launch check",
            description: "Runs installed applications to confirm they still start cleanly.",
            category: Category::Application,
            min_tier: ScanTier::Quick,
            platforms: &[
                PlatformKind::Windows,
                PlatformKind::Linux,
                PlatformKind::MacOs,
            ],
            requires_tools: &[],
            requires_elevation: false,
        }
    }

    async fn run(&self, ctx: &ProbeContext) -> Result<Vec<Finding>> {
        let platform_kind = ctx.platform().kind();
        let mut findings = Vec::new();

        for app in APPS {
            if ctx.is_cancelled() {
                break;
            }
            if !app.platforms.contains(&platform_kind) {
                continue;
            }

            // Only test what is actually installed. Reporting on absent
            // programs would be reporting on the user's choices.
            let Some(executable) = app
                .executables
                .iter()
                .find_map(|name| ctx.platform().locate_tool(name))
            else {
                tracing::debug!(app = app.id, "not installed; skipping");
                continue;
            };
            let executable = executable.to_string_lossy().to_string();

            let args: Vec<String> = app.version_args.iter().map(|arg| arg.to_string()).collect();
            let cancel = ctx.cancel_token().clone();
            let program = executable.clone();
            let outcome = tokio::task::spawn_blocking(move || {
                let args: Vec<&str> = args.iter().map(String::as_str).collect();
                run_supervised(&program, &args, LivenessPolicy::default(), &cancel)
            })
            .await?;

            match outcome {
                Ok(outcome) => {
                    tracing::debug!(
                        app = app.id,
                        succeeded = outcome.succeeded(),
                        "launch tested"
                    );
                    findings.extend(launch_finding(app, &executable, &outcome));
                }
                // Being unable to start it at all is itself the answer, but we
                // cannot tell a broken install from a vanished one, so this is
                // recorded rather than diagnosed.
                Err(error) => {
                    tracing::debug!(app = app.id, %error, "could not run launch test");
                }
            }
        }

        Ok(findings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn app() -> AppDefinition {
        AppDefinition {
            id: "test-app",
            name: "Test App",
            executables: &["testapp"],
            version_args: &["--version"],
            platforms: &[PlatformKind::Linux],
        }
    }

    fn exited(code: i32, stdout: &str, stderr: &str) -> ExecOutcome {
        ExecOutcome::Exited {
            code: Some(code),
            stdout: stdout.to_string(),
            stderr: stderr.to_string(),
            duration: Duration::from_millis(10),
        }
    }

    #[test]
    fn an_application_that_starts_cleanly_produces_no_finding() {
        assert!(launch_finding(&app(), "/usr/bin/testapp", &exited(0, "v1.2.3", "")).is_none());
    }

    #[test]
    fn a_cancelled_test_is_never_blamed_on_the_application() {
        // The user stopped the scan. Recording that as a broken application
        // would be actively misleading.
        let cancelled = ExecOutcome::Cancelled {
            stdout: String::new(),
            stderr: String::new(),
            duration: Duration::from_millis(10),
        };
        assert!(launch_finding(&app(), "/usr/bin/testapp", &cancelled).is_none());
    }

    #[test]
    fn a_failing_application_is_reported_with_its_error_output() {
        let outcome = exited(127, "", "error while loading shared libraries: libfoo.so.1");
        let finding =
            launch_finding(&app(), "/usr/bin/testapp", &outcome).expect("expected a finding");

        assert_eq!(finding.id, "app.launch-failed");
        assert_eq!(finding.severity, Severity::High);
        let stderr = finding
            .evidence
            .iter()
            .find(|item| item.label == "stderr")
            .expect("stderr should be captured as evidence");
        assert!(stderr.value.contains("libfoo.so.1"));
    }

    #[test]
    fn a_hanging_application_is_distinguished_from_a_failing_one() {
        let outcome = ExecOutcome::Stalled {
            stdout: String::new(),
            stderr: String::new(),
            idle: Duration::from_secs(30),
            duration: Duration::from_secs(31),
        };
        let finding =
            launch_finding(&app(), "/usr/bin/testapp", &outcome).expect("expected a finding");

        assert_eq!(finding.id, "app.launch-hung");
        assert!(finding.detail.contains("30 seconds"));
    }

    #[test]
    fn every_catalogue_entry_declares_a_safe_invocation() {
        // An entry with no arguments would launch the application for real,
        // which is exactly what this tier must not do.
        for app in APPS {
            assert!(
                !app.version_args.is_empty(),
                "{} has no version arguments",
                app.id
            );
            assert!(!app.executables.is_empty(), "{} has no executables", app.id);
            assert!(
                !app.platforms.is_empty(),
                "{} supports no platforms",
                app.id
            );
        }
    }

    #[test]
    fn long_output_is_trimmed_for_display() {
        let long = "word ".repeat(500);
        let trimmed = excerpt(&long, 100);
        assert!(trimmed.chars().count() <= 103);
        assert!(trimmed.ends_with("..."));
    }
}
