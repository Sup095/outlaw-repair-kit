//! What has the window in front of you.
//!
//! `docs/proposals/process-control.md` puts "anything with a window in front
//! of you right now" in the list of things a sweep does not touch. Nothing
//! implemented it: the classifier accepted the answer and was never given
//! one, so pointed at a real machine mid-game the enumeration offered the
//! running game as a candidate -- exactly the case the rule exists for.
//!
//! That was harmless while nothing could act on the list. It stops being
//! harmless the moment a button can, so it is a prerequisite for that button
//! rather than a refinement of it.
//!
//! The awkward part is that not knowing is *not* the careful answer here. An
//! unknown owner means "hold it back" and costs a few megabytes; an unknown
//! foreground means this rail protects nothing at all, silently. So the
//! answer carries its own failure, [`InFront::Unknown`], and every screen
//! that shows a sweep is expected to say so rather than let the rail appear
//! to have been applied.

/// What could be established about the window in front of you.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum InFront {
    /// This process owns the window you are looking at.
    Process(u32),
    /// Nothing does. The desktop is showing, the screen is locked, or the
    /// focused window belongs to no process -- all of which are real answers,
    /// distinct from not being able to ask.
    Nothing,
    /// The question could not be asked here. The string is why, in the same
    /// plain terms a skipped check gives its reason.
    ///
    /// Owned rather than borrowed because a survey travels: it is serialised
    /// and read back on the machine it was not taken on, and a reason that
    /// could not survive that journey would arrive as a survey that looked
    /// like every rail had been applied.
    Unknown(String),
}

impl Default for InFront {
    /// Nobody asked. Not the same as asking and being told nothing, and it
    /// reads as "this rail has not been applied", which is what it means.
    fn default() -> InFront {
        InFront::Unknown("the window in front of you was not asked about".to_string())
    }
}

impl InFront {
    /// The process, if there is one and it is known.
    pub fn pid(&self) -> Option<u32> {
        match self {
            InFront::Process(pid) => Some(*pid),
            _ => None,
        }
    }

    /// Why the question could not be answered, if it could not.
    ///
    /// Something to print. A sweep that could not tell what you are looking
    /// at is still a usable sweep, but only if it says so.
    pub fn unanswered(&self) -> Option<&str> {
        match self {
            InFront::Unknown(why) => Some(why),
            _ => None,
        }
    }
}

/// Ask the machine what has the window in front of you, right now.
#[cfg(windows)]
pub fn ask() -> InFront {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowThreadProcessId,
    };

    // Safety: both take what they are given and write only through the
    // pointer we pass. A null window is a documented return, not a failure.
    let window = unsafe { GetForegroundWindow() };
    if window.is_null() {
        // No window has the focus. Genuinely common: the desktop is showing,
        // the screen is locked, or the focus is passing between windows at
        // the moment we looked.
        return InFront::Nothing;
    }
    let mut pid: u32 = 0;
    let thread = unsafe { GetWindowThreadProcessId(window, &mut pid) };
    if thread == 0 || pid == 0 {
        return InFront::Unknown(
            "the window in front could not be traced back to a program".to_string(),
        );
    }
    InFront::Process(pid)
}

/// Ask the machine what has the window in front of you, right now.
///
/// On Linux this depends on the display server rather than on the system, and
/// one of the two common ones will not answer at all.
#[cfg(not(windows))]
pub fn ask() -> InFront {
    // Wayland has no protocol that lets one program ask which window another
    // program has in front of you. That is deliberate, and it is a reasonable
    // thing for a display server to refuse; it is not a gap to be worked
    // around. X11 will answer, but only through a connection this tool does
    // not otherwise need and cannot make on a machine with no display at all.
    //
    // Naming which of the three it is matters, because the three have
    // different answers: on Wayland nothing will ever be able to tell, on X11
    // something could be built, and on a machine with no desktop the question
    // does not arise.
    let session = std::env::var("XDG_SESSION_TYPE").unwrap_or_default();
    if !session.eq_ignore_ascii_case("wayland") && std::env::var_os("WAYLAND_DISPLAY").is_none() {
        if std::env::var_os("DISPLAY").is_none() {
            return InFront::Unknown("there is no desktop session on this machine".to_string());
        }
        return InFront::Unknown("reading the active window on X11 is not built yet".to_string());
    }
    InFront::Unknown(
        "Wayland does not let one program ask what another has in front of you".to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asking_the_real_machine_answers_or_says_why_not() {
        // Whatever this machine is, and whether or not anything has the focus
        // while the tests run, the one thing that must not happen is a
        // panic or a hang inside a rail that decides what gets closed.
        let answer = ask();
        match &answer {
            InFront::Process(pid) => assert_ne!(*pid, 0, "a process id of zero is not an answer"),
            InFront::Nothing => {}
            InFront::Unknown(why) => assert!(!why.is_empty(), "unknown must carry a reason"),
        }
        // And it must be stable enough to ask twice.
        let _ = ask();
    }

    #[test]
    fn not_knowing_is_readable_rather_than_silent() {
        // The whole point of the enum. `Option<u32>` could say "no process"
        // and "could not tell" only by conflating them, and conflating them
        // is what let the rail look applied when it had not been.
        assert_eq!(InFront::Nothing.unanswered(), None);
        assert_eq!(InFront::Process(42).unanswered(), None);
        assert_eq!(
            InFront::Unknown("no desktop".to_string()).unanswered(),
            Some("no desktop")
        );
    }

    #[test]
    fn only_a_real_answer_names_a_process() {
        assert_eq!(InFront::Process(42).pid(), Some(42));
        assert_eq!(InFront::Nothing.pid(), None);
        assert_eq!(InFront::Unknown("no desktop".to_string()).pid(), None);
    }

    #[cfg(windows)]
    #[test]
    fn the_answer_on_windows_is_a_process_that_exists() {
        // A pid that no longer exists is worse than no pid: the classifier
        // would hold back nothing and believe it had. If we got one, it must
        // be a process the machine will admit to.
        let Some(pid) = ask().pid() else {
            return;
        };
        let mut system = sysinfo::System::new();
        system.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
        assert!(
            system.process(sysinfo::Pid::from_u32(pid)).is_some(),
            "the foreground window named process {pid}, which is not running"
        );
    }
}
