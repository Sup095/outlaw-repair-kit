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

/// Which kind of desktop session this is, from the variables that describe it.
///
/// Split out from [`ask`] and given its inputs rather than reading them,
/// because otherwise the whole Linux answer is a branch that only exists on
/// Linux and only runs on a machine with a desktop -- which is to say it ships
/// having never been executed. The decision is the part worth testing; the
/// environment lookup is not.
///
/// Compiled on every platform for the same reason. A function excluded from
/// the Windows build is a function nobody here can run, and this is a rail
/// that decides what gets closed.
pub fn from_session(
    session: &str,
    wayland_display: Option<&str>,
    display: Option<&str>,
) -> InFront {
    // Wayland has no protocol that lets one program ask which window another
    // program has in front of you. That is deliberate, and it is a reasonable
    // thing for a display server to refuse; it is not a gap to be worked
    // around. X11 will answer, but only through a connection this tool does
    // not otherwise need and cannot make on a machine with no display at all.
    //
    // Naming which of the three it is matters, because the three have
    // different answers: on Wayland nothing will ever be able to tell, on X11
    // something could be built, and on a machine with no desktop the question
    // does not arise. Somebody reading "could not be checked" deserves to know
    // which of those they have.
    //
    // Either signal is enough for Wayland. `XDG_SESSION_TYPE` is set by the
    // login manager and is missing under plenty of ways of starting a session;
    // `WAYLAND_DISPLAY` is set by the compositor itself and is the more
    // reliable of the two. Requiring both would call a Wayland desktop an X11
    // one, which is the wrong way round to be wrong: it would promise an
    // answer that will never come.
    let wayland = session.eq_ignore_ascii_case("wayland")
        || wayland_display.is_some_and(|value| !value.is_empty());
    if wayland {
        return InFront::Unknown(
            "Wayland does not let one program ask what another has in front of you".to_string(),
        );
    }
    if display.is_some_and(|value| !value.is_empty()) || session.eq_ignore_ascii_case("x11") {
        return InFront::Unknown("reading the active window on X11 is not built yet".to_string());
    }
    InFront::Unknown("there is no desktop session on this machine".to_string())
}

/// Ask the machine what has the window in front of you, right now.
///
/// On Linux this depends on the display server rather than on the system, and
/// one of the two common ones will not answer at all.
#[cfg(not(windows))]
pub fn ask() -> InFront {
    let session = std::env::var("XDG_SESSION_TYPE").unwrap_or_default();
    from_session(
        &session,
        std::env::var("WAYLAND_DISPLAY").ok().as_deref(),
        std::env::var("DISPLAY").ok().as_deref(),
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

    // The Linux answer, run on whatever machine this is. Every one of these
    // was unreachable until `from_session` was split out: the branch existed
    // only in the Linux build, and the Linux build only runs the branch on a
    // machine with a desktop, which no test machine has.

    #[test]
    fn a_wayland_session_says_the_question_cannot_be_asked() {
        let answer = from_session("wayland", Some("wayland-0"), None);
        let why = answer.unanswered().expect("Wayland cannot be asked");
        assert!(
            why.contains("Wayland"),
            "the reason must name Wayland: {why}"
        );
    }

    #[test]
    fn the_compositor_is_believed_over_the_login_manager() {
        // `XDG_SESSION_TYPE` is set by whatever logged you in and is missing
        // or wrong under plenty of ways of starting a session.
        // `WAYLAND_DISPLAY` is set by the compositor that is actually running.
        // Calling a Wayland desktop an X11 one would promise an answer that is
        // never coming, so either signal is enough.
        for (session, wayland) in [("", Some("wayland-0")), ("tty", Some("wayland-1"))] {
            let answer = from_session(session, wayland, Some(":0"));
            let why = answer.unanswered().expect("still unanswerable");
            assert!(
                why.contains("Wayland"),
                "XDG_SESSION_TYPE={session:?} with a compositor running read as {why:?}"
            );
        }
    }

    #[test]
    fn an_x11_session_is_told_apart_from_no_session_at_all() {
        // Different answers on purpose. One could be built and has not been;
        // the other is a question that does not arise. Somebody told only
        // "could not be checked" cannot tell which they have.
        let x11 = from_session("x11", None, Some(":0"));
        let none = from_session("", None, None);
        assert!(x11.unanswered().unwrap().contains("X11"));
        assert!(none.unanswered().unwrap().contains("no desktop"));
        assert_ne!(
            x11, none,
            "a machine with a desktop and a machine without must not be given              the same reason"
        );
    }

    #[test]
    fn a_variable_set_to_nothing_is_not_a_desktop() {
        // Exported and empty is how a shell leaves a variable it has unset by
        // assignment, and it is not a display. Treating it as one would
        // report X11 on a headless server.
        assert!(
            from_session("", Some(""), Some(""))
                .unanswered()
                .unwrap()
                .contains("no desktop")
        );
    }

    #[test]
    fn every_linux_answer_is_unknown_and_none_of_them_are_silent() {
        // None of these can name a process, and every one of them must carry
        // a reason -- because on this rail an unknown with no reason is a
        // list that looks complete and is not.
        for (session, wayland, display) in [
            ("wayland", Some("wayland-0"), None),
            ("x11", None, Some(":0")),
            ("tty", None, None),
            ("", None, None),
        ] {
            let answer = from_session(session, wayland, display);
            assert_eq!(answer.pid(), None);
            let why = answer.unanswered().unwrap_or("");
            assert!(!why.is_empty(), "{session:?} gave no reason");
        }
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
