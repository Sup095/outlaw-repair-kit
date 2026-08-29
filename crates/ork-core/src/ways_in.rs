//! How this tool gets opened, and where each way in lives on this machine.
//!
//! There are two front-ends -- a window and a terminal program -- and three
//! separate pieces of code have to agree about where they end up: the terminal
//! program, when somebody types `outlaw` on its own and it should say how to
//! open the window; the installer, when it makes a shortcut and needs
//! something real to point at; and the documentation, which claims specific
//! paths. When those three disagree, the symptom is a shortcut that does
//! nothing, which is worse than no shortcut at all -- it looks like the tool
//! is broken rather than absent.
//!
//! So the list of places lives here once.
//!
//! Everything in this module answers "did I find it", never "is it
//! installed". A window installed somewhere nobody thought of is still
//! installed, and reporting it as missing would be the tool being confidently
//! wrong about somebody's own computer.

use std::path::{Path, PathBuf};

/// What the window is called when it is being talked about.
pub const WINDOW_LABEL: &str = "Outlaw Repair Kit";

/// What the terminal program is called when it is being typed.
pub const PROGRAM: &str = "outlaw";

/// The window's file name, on every platform.
///
/// One name, chosen rather than defaulted. It is `mainBinaryName` in
/// `tauri.conf.json` and the `[[bin]]` name in the window's own manifest, and
/// a test below reads the first of those and checks it still says this -- so
/// renaming the window in one place and not the other fails the build rather
/// than shipping a shortcut that points at nothing.
pub const fn window_file_name() -> &'static str {
    if cfg!(windows) {
        "outlaw-repair-kit.exe"
    } else {
        "outlaw-repair-kit"
    }
}

/// The terminal program's file name.
pub const fn program_file_name() -> &'static str {
    if cfg!(windows) {
        "outlaw.exe"
    } else {
        "outlaw"
    }
}

/// Every place the window lands, given the ways this project publishes it.
///
/// Ordered by how likely each is, and it is fine for this to be a guess: the
/// only thing done with these paths is checking whether a file is there.
pub fn window_places() -> Vec<PathBuf> {
    let mut places = Vec::new();
    let file = window_file_name();

    // Beside the terminal program is first on purpose. It is the one place
    // that is true regardless of how the tool arrived -- unpacked by hand,
    // built from source, or put there by the install script.
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        places.push(dir.join(file));
    }

    if cfg!(windows) {
        // The Windows bundle installs per-user by default and per-machine
        // when somebody chooses that, and names the folder after the product.
        if let Some(local) = dirs::data_local_dir() {
            places.push(local.join(WINDOW_LABEL).join(file));
            places.push(local.join("Programs").join(WINDOW_LABEL).join(file));
            // Where the install script puts the terminal program, in case the
            // window was dropped in beside it.
            places.push(local.join("Programs").join("OutlawRepairKit").join(file));
        }
        for variable in ["ProgramFiles", "ProgramFiles(x86)"] {
            if let Some(dir) = std::env::var_os(variable) {
                places.push(PathBuf::from(dir).join(WINDOW_LABEL).join(file));
            }
        }
    } else {
        if let Some(home) = dirs::home_dir() {
            places.push(home.join(".local").join("bin").join(file));
        }
        places.push(PathBuf::from("/usr/bin").join(file));
        places.push(PathBuf::from("/usr/local/bin").join(file));
        places.push(PathBuf::from("/opt").join(WINDOW_LABEL).join(file));
    }

    places
}

/// The installed window, if one can be found.
///
/// `None` means "not found in any of the usual places", which is not the same
/// as "not installed", and nothing that calls this may say otherwise.
pub fn find_window() -> Option<PathBuf> {
    window_places().into_iter().find(|place| place.is_file())
}

/// The window, found the same way but starting from a directory somebody has
/// just installed into.
///
/// The installer knows where it put things and should not have to search for
/// them, but it should still find a window that was already there.
pub fn find_window_near(directory: &Path) -> Option<PathBuf> {
    let beside = directory.join(window_file_name());
    if beside.is_file() {
        return Some(beside);
    }
    find_window()
}

/// The terminal program, if this is not it.
///
/// Used by the window, which wants to be able to say where the command-line
/// half of the tool is on a machine that has both.
pub fn find_program() -> Option<PathBuf> {
    let file = program_file_name();
    let mut places = Vec::new();
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        places.push(dir.join(file));
    }
    if cfg!(windows) {
        if let Some(local) = dirs::data_local_dir() {
            places.push(local.join("Programs").join("OutlawRepairKit").join(file));
            places.push(local.join(WINDOW_LABEL).join(file));
        }
    } else if let Some(home) = dirs::home_dir() {
        places.push(home.join(".local").join("bin").join(file));
    }
    places.into_iter().find(|place| place.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_two_front_ends_are_not_the_same_file() {
        // Guards the mistake this module exists to prevent: a shortcut meant
        // for the window that points at the terminal program, which on
        // Windows flashes a console and vanishes and on Linux does nothing
        // visible at all.
        assert_ne!(window_file_name(), program_file_name());
    }

    #[test]
    fn every_place_the_window_might_be_is_a_file_path_not_a_directory() {
        for place in window_places() {
            assert_eq!(
                place.file_name().and_then(|name| name.to_str()),
                Some(window_file_name()),
                "{} does not end in the window's file name",
                place.display()
            );
        }
    }

    #[test]
    fn there_is_somewhere_to_look_on_every_platform() {
        // A list that came out empty would make `find_window` answer "not
        // found" everywhere, for ever, without ever having looked.
        assert!(
            window_places().len() >= 2,
            "only {} place(s) to look",
            window_places().len()
        );
    }

    #[test]
    fn looking_beside_this_program_comes_first() {
        // The one place that is true however the tool arrived. If a search
        // order change ever demoted it, a machine with the window unpacked
        // next to the program would start reporting it as missing.
        let places = window_places();
        let exe = std::env::current_exe().unwrap();
        assert_eq!(places[0], exe.parent().unwrap().join(window_file_name()));
    }

    #[test]
    fn a_window_that_is_not_there_is_reported_as_not_found_rather_than_guessed_at() {
        // Whatever this machine has, the answer must be a real file or
        // nothing. It must never be a path that only might exist.
        if let Some(found) = find_window() {
            assert!(found.is_file(), "{} is not a file", found.display());
        }
    }

    #[test]
    fn the_name_looked_for_is_the_name_the_window_is_actually_built_under() {
        // Read from the window's own configuration rather than written down
        // twice. This is the exact failure this module exists to prevent: the
        // published .deb installed `/usr/bin/ork-desktop` while everything
        // here looked for `outlaw-repair-kit`, so the window could not be
        // found on a machine that had it installed.
        let config = include_str!("../../../apps/desktop/src-tauri/tauri.conf.json");
        let built_as = config
            .lines()
            .find_map(|line| {
                let (key, value) = line.split_once(':')?;
                key.contains("mainBinaryName").then(|| {
                    value
                        .trim()
                        .trim_end_matches(',')
                        .trim_matches('"')
                        .to_string()
                })
            })
            .expect("tauri.conf.json should name the binary explicitly");

        let expected = window_file_name().trim_end_matches(".exe");
        assert_eq!(
            built_as, expected,
            "the window is built as `{built_as}` and looked for as `{expected}`"
        );
    }

    #[test]
    fn a_window_beside_a_named_directory_is_found_there() {
        let dir = std::env::temp_dir().join(format!("ork-ways-in-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let planted = dir.join(window_file_name());
        std::fs::write(&planted, b"not really a window").unwrap();

        assert_eq!(find_window_near(&dir), Some(planted));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
