//! Running things without a console window appearing.
//!
//! Almost everything this tool asks the machine is answered by another
//! program: PowerShell for the registry and the event log, `smartctl` for disk
//! health, `wmic` and `nvidia-smi` for hardware, `ollama` for a model. From a
//! terminal that is invisible -- the child inherits the console it was started
//! from and nothing appears.
//!
//! From a window it is not. Windows gives a console subsystem process its own
//! console, so every one of those calls flashes a black rectangle onto the
//! screen and takes the focus with it. A scan makes dozens. The effect is a
//! desktop application that appears to be doing something furtive, and it is
//! worst during an install -- the moment somebody is deciding whether they
//! trust this program at all.
//!
//! `CREATE_NO_WINDOW` is the whole fix. It is applied here, in one place, so
//! that the answer does not have to be remembered at each of the nine places
//! that start a process.
//!
//! **This is not the same as hiding what the tool does.** Every command is
//! logged, the fix engine shows the exact command before running it, and the
//! audit log keeps all of them. What is suppressed is a window nobody asked
//! for, not the record.

use std::process::Command;

/// Start a process without giving it a console window of its own.
///
/// A no-op everywhere but Windows, which is the only platform that would
/// otherwise create one.
pub trait Unseen {
    fn unseen(&mut self) -> &mut Self;
}

impl Unseen for Command {
    #[cfg(windows)]
    fn unseen(&mut self) -> &mut Command {
        use std::os::windows::process::CommandExt;
        // Documented in the Windows process creation flags. Written out rather
        // than pulled from a crate, because one constant is not worth a
        // dependency and the number is fixed for the life of the platform.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        self.creation_flags(CREATE_NO_WINDOW)
    }

    #[cfg(not(windows))]
    fn unseen(&mut self) -> &mut Command {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_command_still_runs_and_still_answers() {
        // The flag must not change what a command does or what comes back
        // from it -- only whether a window appears. Checked with something
        // every platform has.
        let (program, args): (&str, &[&str]) = if cfg!(windows) {
            ("cmd", &["/C", "echo", "hello"])
        } else {
            ("echo", &["hello"])
        };

        let output = Command::new(program)
            .args(args)
            .unseen()
            .output()
            .expect("a program every machine has should run");

        assert!(output.status.success());
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("hello"),
            "output was lost: {output:?}"
        );
    }

    #[test]
    fn a_failure_is_still_a_failure() {
        // Suppressing the window must not suppress the exit status, which is
        // how every caller here decides whether a check ran.
        let (program, args): (&str, &[&str]) = if cfg!(windows) {
            ("cmd", &["/C", "exit 3"])
        } else {
            ("sh", &["-c", "exit 3"])
        };

        let status = Command::new(program)
            .args(args)
            .unseen()
            .status()
            .expect("should start");
        assert_eq!(status.code(), Some(3));
    }

    #[test]
    fn a_program_that_is_not_there_still_fails_to_start() {
        let started = Command::new("definitely-not-a-real-program-4f2a")
            .unseen()
            .output();
        assert!(started.is_err(), "a missing program should not start");
    }
}
