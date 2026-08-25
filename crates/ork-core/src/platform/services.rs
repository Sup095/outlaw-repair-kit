//! Asking whether a service is actually running.
//!
//! This exists because "the restart command exited zero" and "the service is
//! up" are different statements, and only the second one is worth anything to
//! somebody whose machine is broken. A service can be told to start, report
//! success, and be dead again a second later.
//!
//! Every failure to find out is reported as [`ServiceStatus::Unknown`] rather
//! than guessed at in either direction. A verifier built on this treats not
//! knowing as failure, so a wrong guess here would either undo a fix that
//! worked or keep one that did not.

use crate::platform::ServiceStatus;
use crate::platform::common::run_capture;

/// Ask Windows, through the service control manager.
#[cfg(windows)]
pub fn status(name: &str) -> ServiceStatus {
    // `Get-Service` writes a terminating error when the name does not exist,
    // which is the answer rather than a failure to get one.
    let script = format!(
        "$ErrorActionPreference='Stop'; try {{ (Get-Service -Name '{}').Status.ToString() }} \
         catch {{ 'NotFound' }}",
        name.replace('\'', "''")
    );

    match run_capture(
        "powershell",
        &["-NoProfile", "-NonInteractive", "-Command", &script],
    ) {
        Ok(output) => interpret_windows(output.stdout.trim()),
        Err(error) => ServiceStatus::Unknown {
            detail: error.to_string(),
        },
    }
}

/// Ask systemd.
#[cfg(target_os = "linux")]
pub fn status(name: &str) -> ServiceStatus {
    // `is-active` exits non-zero for anything that is not running, so the exit
    // code says nothing useful on its own -- the word it prints does.
    match run_capture("systemctl", &["is-active", name]) {
        Ok(output) => {
            let said = output.stdout.trim().to_string();
            if said.is_empty() {
                // No word at all usually means systemd is not the init system
                // here, which is a real possibility on a container or a
                // non-systemd distribution.
                return ServiceStatus::Unknown {
                    detail: output.stderr.trim().to_string(),
                };
            }
            interpret_systemd(&said)
        }
        Err(error) => ServiceStatus::Unknown {
            detail: error.to_string(),
        },
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
pub fn status(name: &str) -> ServiceStatus {
    let _ = name;
    ServiceStatus::Unknown {
        detail: "services cannot be inspected on this system".to_string(),
    }
}

// Both interpreters are compiled on every platform, not just the one that
// calls them, so that their tests run on every platform too. Getting "is this
// service running" wrong is the kind of mistake that reports a fix as
// successful when it is not, and it should not be able to hide on the OS the
// build machine happens not to be.
/// What the service control manager's words mean.
///
/// Anything mid-transition is deliberately *not* called running: a service on
/// its way up has not arrived, and reporting it as fixed would be believing a
/// promise instead of an outcome.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn interpret_windows(said: &str) -> ServiceStatus {
    match said.trim().to_ascii_lowercase().as_str() {
        "running" => ServiceStatus::Running,
        "stopped" | "paused" => ServiceStatus::Stopped,
        "notfound" | "" => ServiceStatus::NotFound,
        transitional @ ("startpending" | "stoppending" | "continuepending" | "pausepending") => {
            ServiceStatus::Unknown {
                detail: format!("still {transitional}"),
            }
        }
        other => ServiceStatus::Unknown {
            detail: other.to_string(),
        },
    }
}

/// What systemd's words mean.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn interpret_systemd(said: &str) -> ServiceStatus {
    match said.trim().to_ascii_lowercase().as_str() {
        "active" => ServiceStatus::Running,
        "inactive" | "failed" | "deactivating" => ServiceStatus::Stopped,
        // systemd says this both for a name it has never heard of and for a
        // unit that exists but has no state, so it cannot be read as "running".
        "unknown" => ServiceStatus::NotFound,
        // On its way up. Not there yet, and not a failure either.
        "activating" | "reloading" => ServiceStatus::Unknown {
            detail: format!("still {said}"),
        },
        other => ServiceStatus::Unknown {
            detail: other.to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn running_is_the_only_word_that_means_running() {
        assert_eq!(interpret_windows("Running"), ServiceStatus::Running);
        assert_eq!(interpret_systemd("active"), ServiceStatus::Running);
    }

    #[test]
    fn a_service_on_its_way_up_has_not_arrived() {
        // Reporting a pending start as running would be believing a promise
        // instead of an outcome, and a verifier would call the fix a success.
        for said in ["StartPending", "ContinuePending"] {
            assert!(
                matches!(interpret_windows(said), ServiceStatus::Unknown { .. }),
                "{said} was read as a settled state"
            );
        }
        for said in ["activating", "reloading"] {
            assert!(
                matches!(interpret_systemd(said), ServiceStatus::Unknown { .. }),
                "{said} was read as a settled state"
            );
        }
    }

    #[test]
    fn a_stopped_or_failed_service_is_stopped() {
        assert_eq!(interpret_windows("Stopped"), ServiceStatus::Stopped);
        assert_eq!(interpret_systemd("inactive"), ServiceStatus::Stopped);
        // A unit that tried and failed is not running, whatever else it is.
        assert_eq!(interpret_systemd("failed"), ServiceStatus::Stopped);
    }

    #[test]
    fn a_name_nothing_recognises_is_not_a_running_service() {
        assert_eq!(interpret_windows("NotFound"), ServiceStatus::NotFound);
        assert_eq!(interpret_windows(""), ServiceStatus::NotFound);
        assert_eq!(interpret_systemd("unknown"), ServiceStatus::NotFound);
    }

    #[test]
    fn words_nobody_expected_are_never_guessed_at() {
        for said in ["banana", "ACTIVE-ISH", "0"] {
            assert!(
                matches!(interpret_windows(said), ServiceStatus::Unknown { .. }),
                "{said} was interpreted"
            );
        }
        assert!(matches!(
            interpret_systemd("banana"),
            ServiceStatus::Unknown { .. }
        ));
    }

    /// Asks the real service control manager. The words above are only worth
    /// anything if they are the words this machine actually says.
    #[test]
    fn a_name_nothing_has_heard_of_is_reported_as_not_found_on_this_machine() {
        let answer = status("ork-definitely-not-a-real-service-name");
        assert!(
            matches!(
                answer,
                ServiceStatus::NotFound | ServiceStatus::Unknown { .. }
            ),
            "a made-up service reported {answer:?}"
        );
        assert!(
            !answer.is_running(),
            "a made-up service was reported as running"
        );
    }

    #[cfg(windows)]
    #[test]
    fn a_service_windows_always_has_gives_a_definite_answer() {
        // The service control manager itself is always registered.
        let answer = status("Winmgmt");
        assert!(
            !matches!(answer, ServiceStatus::NotFound),
            "Windows Management Instrumentation was not found: {answer:?}"
        );
    }

    #[test]
    fn case_does_not_change_the_answer() {
        assert_eq!(interpret_windows("RUNNING"), ServiceStatus::Running);
        assert_eq!(interpret_systemd("ACTIVE"), ServiceStatus::Running);
    }
}
