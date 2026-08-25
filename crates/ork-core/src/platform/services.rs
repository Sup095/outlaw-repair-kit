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
    // Two properties, not one. `is-active` answers "inactive" for a unit
    // systemd has never heard of, which is indistinguishable from a unit that
    // exists and is stopped -- so a typo in a finding would look like a
    // service waiting to be restarted. `LoadState` is what tells them apart.
    match run_capture(
        "systemctl",
        &["show", "-p", "LoadState", "-p", "ActiveState", name],
    ) {
        Ok(output) => interpret_systemd_show(&output.stdout, output.stderr.trim()),
        Err(error) => ServiceStatus::Unknown {
            detail: error.to_string(),
        },
    }
}

/// What `systemctl show` reports, read as a whole.
///
/// `LoadState` is consulted first because it decides whether the answer from
/// `ActiveState` means anything at all.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn interpret_systemd_show(stdout: &str, stderr: &str) -> ServiceStatus {
    let property = |wanted: &str| {
        stdout.lines().find_map(|line| {
            let (key, value) = line.split_once('=')?;
            (key.trim() == wanted).then(|| value.trim().to_ascii_lowercase())
        })
    };

    match property("LoadState").as_deref() {
        Some("loaded") => match property("ActiveState") {
            Some(active) => interpret_systemd(&active),
            // Loaded but no state reported at all. Nothing to go on.
            None => ServiceStatus::Unknown {
                detail: "systemd reported no state".to_string(),
            },
        },
        Some("not-found") => ServiceStatus::NotFound,
        // Masked, or a unit file systemd could not read. It exists in some
        // sense but is not going to run, and "not found" would send someone
        // looking for a spelling mistake that is not there.
        Some(other) => ServiceStatus::Unknown {
            detail: format!("the unit is {other}"),
        },
        // No LoadState line usually means systemd is not the init system
        // here, which is a real possibility in a container or on a
        // distribution that does not use it.
        None => ServiceStatus::Unknown {
            detail: if stderr.is_empty() {
                "systemd did not answer".to_string()
            } else {
                stderr.to_string()
            },
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

    #[test]
    fn a_unit_systemd_has_never_heard_of_is_not_merely_stopped() {
        // This is the one that matters. `is-active` says "inactive" for a
        // name that does not exist, which would make a typo in a finding look
        // like a service sitting there waiting to be restarted.
        assert_eq!(
            interpret_systemd_show(
                "LoadState=not-found
ActiveState=inactive
",
                ""
            ),
            ServiceStatus::NotFound
        );
    }

    #[test]
    fn a_loaded_unit_is_judged_on_its_active_state() {
        assert_eq!(
            interpret_systemd_show(
                "LoadState=loaded
ActiveState=active
",
                ""
            ),
            ServiceStatus::Running
        );
        assert_eq!(
            interpret_systemd_show(
                "LoadState=loaded
ActiveState=failed
",
                ""
            ),
            ServiceStatus::Stopped
        );
    }

    #[test]
    fn a_masked_unit_is_not_reported_as_a_spelling_mistake() {
        // It exists and will not run. Calling that "not found" sends someone
        // hunting for a typo that is not there.
        let answer = interpret_systemd_show(
            "LoadState=masked
ActiveState=inactive
",
            "",
        );
        assert!(
            matches!(answer, ServiceStatus::Unknown { .. }),
            "{answer:?}"
        );
        assert!(!answer.is_running());
    }

    #[test]
    fn no_answer_at_all_is_not_read_as_anything() {
        let answer = interpret_systemd_show("", "System has not been booted with systemd");
        match answer {
            ServiceStatus::Unknown { detail } => assert!(detail.contains("systemd")),
            other => panic!("expected unknown, got {other:?}"),
        }
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
