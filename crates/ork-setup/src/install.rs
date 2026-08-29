//! Putting things where they go, and being able to say what was put where.
//!
//! Two rules run through all of this.
//!
//! **No administrator rights.** Everything installs under the user's own
//! profile and puts the program on the user's own `PATH`. An installer that
//! demands elevation to unpack a diagnostic tool is asking for rights it does
//! not need, and a person who grants them cannot tell the difference between
//! this and something that did need them for a worse reason. The one place
//! elevation is ever mentioned is the checks that genuinely require it, and
//! those ask at the time, in the application, for that check.
//!
//! **Everything is written down.** A record of what was installed goes beside
//! the files, so that removing this later is reading a list rather than
//! guessing. An installer that cannot tell you what it did is one you have to
//! take on trust twice.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

// `bail!` is written out at its two call sites rather than imported: both are
// inside `#[cfg(windows)]` blocks, so importing it leaves an unused import on
// Linux -- which is a warning, and warnings are errors here.
use anyhow::{Context, Result};

/// Where the program goes, per platform, under the user's own profile.
pub fn default_directory() -> Result<PathBuf> {
    #[cfg(windows)]
    {
        let base = dirs::data_local_dir()
            .context("this account has no local application data directory")?;
        Ok(base.join("Programs").join("OutlawRepairKit"))
    }
    #[cfg(not(windows))]
    {
        let base = dirs::home_dir().context("this account has no home directory")?;
        Ok(base.join(".local").join("share").join("outlaw-repair-kit"))
    }
}

/// Where a program on `PATH` is expected to live, per platform.
pub fn bin_directory(install_dir: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        install_dir.to_path_buf()
    }
    #[cfg(not(windows))]
    {
        // `~/.local/bin` is on the default PATH of every current desktop
        // distribution, so a symlink there needs no shell configuration and
        // no restart.
        dirs::home_dir()
            .map(|home| home.join(".local").join("bin"))
            .unwrap_or_else(|| install_dir.to_path_buf())
    }
}

/// The name of the command-line program on this platform.
pub const fn program_name() -> &'static str {
    if cfg!(windows) {
        "outlaw.exe"
    } else {
        "outlaw"
    }
}

/// One thing this installer did, in enough detail to undo it.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Step {
    /// A file was written that was not there before.
    Wrote { path: String, sha256: String },
    /// A directory was added to the user's PATH.
    AddedToPath { directory: String },
    /// The program was made reachable by name from a terminal.
    Linked { path: String },
    /// A shortcut or desktop entry was created.
    Shortcut {
        path: String,
        /// What clicking it opens, in the words the receipt should use.
        #[serde(default)]
        label: String,
    },
    /// Something was installed by another program on our behalf, which this
    /// will never remove -- it did not put it there in any sense it can undo.
    Delegated { what: String, command: String },
}

/// The record left beside the installation.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Receipt {
    pub version: String,
    pub installed_at: String,
    pub directory: String,
    pub steps: Vec<Step>,
}

impl Receipt {
    pub const FILE: &'static str = "install-receipt.json";

    pub fn write(&self, directory: &Path) -> Result<PathBuf> {
        let path = directory.join(Self::FILE);
        let text =
            serde_json::to_string_pretty(self).context("could not describe what was done")?;
        fs::write(&path, text)
            .with_context(|| format!("could not write the record to {}", path.display()))?;
        Ok(path)
    }

    pub fn read(directory: &Path) -> Option<Receipt> {
        let text = fs::read_to_string(directory.join(Self::FILE)).ok()?;
        serde_json::from_str(&text).ok()
    }
}

/// Write a verified download to its place.
///
/// Written to a temporary name in the same directory and then renamed, so an
/// interrupted install cannot leave half a program under the name of a whole
/// one. Same directory rather than the system temporary one, because a rename
/// across file systems is a copy and stops being atomic.
pub fn place(directory: &Path, name: &str, bytes: &[u8]) -> Result<PathBuf> {
    fs::create_dir_all(directory)
        .with_context(|| format!("could not create {}", directory.display()))?;

    let final_path = directory.join(name);
    let staging = directory.join(format!(".{name}.incoming"));

    {
        let mut file = fs::File::create(&staging)
            .with_context(|| format!("could not write to {}", staging.display()))?;
        file.write_all(bytes)
            .with_context(|| format!("could not write {}", staging.display()))?;
        file.sync_all().ok();
    }

    mark_executable(&staging)?;

    // Windows will not rename over a file that is currently running, which is
    // exactly the case when somebody re-runs this while the tool is open.
    // Moving the old one aside first turns an unhelpful "access denied" into
    // an install that works and a leftover the next run clears.
    if final_path.exists() {
        let retired = directory.join(format!(".{name}.old"));
        let _ = fs::remove_file(&retired);
        if fs::rename(&final_path, &retired).is_err() {
            let _ = fs::remove_file(&final_path);
        }
    }

    fs::rename(&staging, &final_path).with_context(|| {
        format!(
            "could not put {} in place -- is it running?",
            final_path.display()
        )
    })?;

    Ok(final_path)
}

fn mark_executable(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(path)?.permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions)
            .with_context(|| format!("could not make {} executable", path.display()))?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}

/// Clear anything a previous run left behind.
pub fn tidy(directory: &Path) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') && (name.ends_with(".old") || name.ends_with(".incoming")) {
            let _ = fs::remove_file(entry.path());
        }
    }
}

/// Whether a directory is already on this user's PATH.
pub fn already_on_path(directory: &Path) -> bool {
    let Ok(path) = std::env::var("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|entry| entry == directory)
}

/// Append a directory to one of this account's environment variables.
///
/// Split out from [`add_to_path`] and given the variable name as an argument
/// for one reason: it is the most dangerous thing this program does on
/// Windows, and this is the only way to test it without experimenting on
/// somebody's real PATH. The tests below point it at a scratch variable and
/// then delete that variable.
///
/// `[Environment]::SetEnvironmentVariable` at User scope is used rather than
/// `setx`, because setx silently truncates a value longer than 1024
/// characters. A developer's PATH is routinely longer than that, and quietly
/// truncating somebody's PATH is a far worse thing to do than failing.
#[cfg(windows)]
fn append_to_user_variable(variable: &str, directory: &Path) -> Result<bool> {
    // The user's own environment, in the user's own registry hive. No
    // administrator rights, and nothing outside this account is touched.
    let quoted = directory.display().to_string().replace('\'', "''");
    let name = variable.replace('\'', "''");
    let script = format!(
        "$name = '{name}'; \
         $dir = '{quoted}'; \
         $current = [Environment]::GetEnvironmentVariable($name, 'User'); \
         if ($null -eq $current -or $current -eq '') {{ \
             [Environment]::SetEnvironmentVariable($name, $dir, 'User') \
         }} elseif (($current -split ';') -notcontains $dir) {{ \
             [Environment]::SetEnvironmentVariable($name, \"$current;$dir\", 'User') \
         }}"
    );
    let output = ork_core::platform::run_capture(
        "powershell",
        &["-NoProfile", "-NonInteractive", "-Command", &script],
    )
    .with_context(|| format!("could not update this account's {variable}"))?;
    if !output.success {
        anyhow::bail!(
            "could not update this account's {variable}: {}",
            output.stderr.trim()
        );
    }
    Ok(true)
}

/// Put a directory on the user's PATH, permanently, without elevation.
///
/// Returns `Ok(false)` when it was already there.
pub fn add_to_path(directory: &Path) -> Result<bool> {
    if already_on_path(directory) {
        return Ok(false);
    }

    #[cfg(windows)]
    {
        append_to_user_variable("PATH", directory)
    }

    #[cfg(not(windows))]
    {
        // `~/.local/bin` is already on PATH on every current desktop
        // distribution, so there is normally nothing to do. When it is not,
        // one line is appended to the shell's own profile -- appended, and
        // never rewritten, because that file is the user's.
        fs::create_dir_all(directory)
            .with_context(|| format!("could not create {}", directory.display()))?;

        let home = dirs::home_dir().context("this account has no home directory")?;
        let profile = home.join(".profile");
        let line = format!("\nexport PATH=\"{}:$PATH\"\n", directory.display());

        let existing = fs::read_to_string(&profile).unwrap_or_default();
        if existing.contains(&directory.display().to_string()) {
            return Ok(false);
        }

        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&profile)
            .with_context(|| format!("could not add to {}", profile.display()))?;
        file.write_all(line.as_bytes())
            .with_context(|| format!("could not add to {}", profile.display()))?;
        Ok(true)
    }
}

/// Install the window by running the bundle it ships as, and then tidy the
/// bundle away.
///
/// This used to download the bundle, put it in the folder, and say "run it to
/// install the window" -- which left somebody who asked for the window with a
/// folder containing an installer and no window in it. Asking for a thing and
/// being handed the means of getting the thing is not installing it.
///
/// Run without its own questions, because they were already asked here: the
/// person ticked a box that said install the window, and answering the same
/// question twice in two different windows is how an installer loses somebody
/// half way through. Nothing else about it is silent -- the plan says this
/// will happen before it happens, and the receipt records it afterwards.
#[cfg(windows)]
pub fn run_window_installer(bundle: &Path) -> Result<()> {
    use ork_core::unseen::Unseen;

    // The `/S` is NSIS's, and is the only way to install without a second
    // window appearing over this one.
    let status = std::process::Command::new(bundle)
        .arg("/S")
        .unseen()
        .status()
        .with_context(|| format!("could not run {}", bundle.display()))?;

    if !status.success() {
        anyhow::bail!(
            "the window's installer stopped with {}",
            status
                .code()
                .map(|code| format!("code {code}"))
                .unwrap_or_else(|| "no exit code".to_string())
        );
    }
    Ok(())
}

/// On Linux the window is an AppImage: placing it *is* installing it, so
/// there is nothing to run.
#[cfg(not(windows))]
pub fn run_window_installer(bundle: &Path) -> Result<()> {
    mark_executable(bundle)
}

/// Whether the window's bundle is a thing to be run or a thing to be kept.
///
/// A Windows bundle is an installer: once it has run, keeping it is keeping a
/// copy of an installer nobody needs, in a folder somebody will later wonder
/// about. A Linux AppImage *is* the program, and deleting it would delete
/// what was just installed.
pub const fn bundle_is_disposable() -> bool {
    cfg!(windows)
}

/// What a shortcut opens when somebody clicks it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Opens {
    /// The window. Clicking it shows a window and nothing else.
    AWindow,
    /// The terminal program, with a terminal around it that stays open.
    ///
    /// This is the distinction that used to be missing, and getting it wrong
    /// produces the worst kind of failure. A shortcut pointing straight at
    /// `outlaw` opens a console on Windows, prints, and closes it again
    /// faster than anybody can read; on Linux, in a desktop entry that says
    /// `Terminal=false`, it does nothing visible whatsoever. Both look
    /// exactly like a program that is broken, to the one person least able to
    /// tell the difference -- somebody whose computer is already misbehaving.
    ATerminal,
}

/// The name of the little script a terminal shortcut points at.
pub const fn shim_name() -> &'static str {
    if cfg!(windows) {
        "outlaw-terminal.cmd"
    } else {
        "outlaw-terminal"
    }
}

/// Write the script that opens the program at a prompt and stays there.
///
/// A desktop entry has no way of saying "and then leave the terminal open",
/// and a Windows shortcut can only say it with quoting that is easy to get
/// wrong and unreadable afterwards. A few lines of script say it plainly, sit
/// beside the program where anybody can read them, and mean that
/// double-clicking the file in a file manager works as well as the shortcut
/// does.
pub fn write_terminal_shim(directory: &Path) -> Result<PathBuf> {
    let path = directory.join(shim_name());

    #[cfg(windows)]
    let contents = concat!(
        "@echo off\r\n",
        "rem Opens the Outlaw Repair Kit at a prompt and stays there, so what\r\n",
        "rem it prints can be read and typed at. Deleting this file removes\r\n",
        "rem nothing but the convenience.\r\n",
        "cd /d \"%~dp0\"\r\n",
        "\"%~dp0outlaw.exe\" %*\r\n",
        "cmd /k\r\n",
    );

    #[cfg(not(windows))]
    let contents = concat!(
        "#!/bin/sh\n",
        "# Opens the Outlaw Repair Kit at a prompt and stays there, so what\n",
        "# it prints can be read and typed at. Deleting this file removes\n",
        "# nothing but the convenience.\n",
        "here=$(dirname \"$0\")\n",
        "\"$here/outlaw\" \"$@\"\n",
        "exec \"${SHELL:-/bin/sh}\"\n",
    );

    fs::write(&path, contents).with_context(|| format!("could not write {}", path.display()))?;
    mark_executable(&path)?;
    Ok(path)
}

/// Make the program reachable by name from a terminal.
///
/// On Windows the program is installed straight into the directory that goes
/// on PATH, so there is nothing to do. On Linux it is not: the program lives
/// under `~/.local/share`, and `~/.local/bin` is the directory every current
/// desktop distribution already has on PATH. Without this, the installer
/// added a directory to PATH that the program was not in, and typing `outlaw`
/// answered "command not found" on a machine where it had just been correctly
/// installed.
///
/// A symlink, so that replacing the program replaces what the link reaches.
/// A copy if the file system will not have a link, because a working copy is
/// worth more than a tidy failure.
pub fn link_into_path(program: &Path, bin: &Path) -> Result<Option<PathBuf>> {
    let Some(directory) = program.parent() else {
        return Ok(None);
    };
    if directory == bin {
        return Ok(None);
    }
    fs::create_dir_all(bin).with_context(|| format!("could not create {}", bin.display()))?;

    let link = bin.join(program.file_name().unwrap_or_default());
    // Replaced rather than added to: a link left over from a previous install
    // is exactly what this is here to correct.
    let _ = fs::remove_file(&link);

    #[cfg(unix)]
    {
        if std::os::unix::fs::symlink(program, &link).is_ok() {
            return Ok(Some(link));
        }
    }

    fs::copy(program, &link)
        .with_context(|| format!("could not put the program in {}", bin.display()))?;
    mark_executable(&link)?;
    Ok(Some(link))
}

/// Make something reachable the way this platform expects: a Start menu entry
/// on Windows, a desktop entry on Linux.
///
/// Best-effort by design. A missing shortcut is a nuisance somebody can work
/// around in ten seconds; failing an otherwise good install over one would be
/// out of all proportion.
pub fn make_shortcut(target: &Path, label: &str, opens: Opens) -> Result<PathBuf> {
    #[cfg(windows)]
    {
        let start_menu = dirs::data_dir()
            .context("this account has no application data directory")?
            .join("Microsoft")
            .join("Windows")
            .join("Start Menu")
            .join("Programs");
        fs::create_dir_all(&start_menu)?;
        let link = start_menu.join(format!("{label}.lnk"));

        let quote = |path: &Path| path.display().to_string().replace('\'', "''");
        let working = target.parent().unwrap_or(target);
        // A terminal shortcut points at the script rather than the program,
        // and the script has no icon of its own, so the icon is taken from
        // the program it opens.
        let icon = match opens {
            Opens::AWindow => quote(target),
            Opens::ATerminal => quote(&working.join(program_name())),
        };

        let script = format!(
            "$shell = New-Object -ComObject WScript.Shell; \
             $link = $shell.CreateShortcut('{}'); \
             $link.TargetPath = '{}'; \
             $link.WorkingDirectory = '{}'; \
             $link.IconLocation = '{}'; \
             $link.Description = 'Outlaw Repair Kit'; \
             $link.Save()",
            quote(&link),
            quote(target),
            quote(working),
            icon,
        );
        let output = ork_core::platform::run_capture(
            "powershell",
            &["-NoProfile", "-NonInteractive", "-Command", &script],
        )
        .context("could not create a Start menu shortcut")?;
        if !output.success {
            anyhow::bail!("could not create a Start menu shortcut");
        }
        Ok(link)
    }

    #[cfg(not(windows))]
    {
        let applications = dirs::data_dir()
            .context("this account has no data directory")?
            .join("applications");
        fs::create_dir_all(&applications)?;
        // Two entries can exist side by side, so they cannot share a file
        // name. Both are named from the bundle identifier, so a desktop that
        // already knows this application recognises them as belonging to it.
        let entry = applications.join(match opens {
            Opens::AWindow => "systems.outlaw.repairkit.desktop",
            Opens::ATerminal => "systems.outlaw.repairkit.terminal.desktop",
        });
        // `Terminal=true` for the terminal program is the whole point. An
        // entry that says otherwise leaves somebody clicking an icon that
        // appears to do nothing at all.
        let terminal = match opens {
            Opens::AWindow => "false",
            Opens::ATerminal => "true",
        };
        let contents = format!(
            "[Desktop Entry]\n\
             Type=Application\n\
             Version=1.0\n\
             Name={label}\n\
             Comment=Scan a computer for problems, in plain language\n\
             Exec={}\n\
             Terminal={terminal}\n\
             Categories=System;Utility;Monitor;\n\
             Keywords=diagnostic;repair;scan;hardware;\n",
            target.display()
        );
        fs::write(&entry, contents)
            .with_context(|| format!("could not write {}", entry.display()))?;
        mark_executable(&entry)?;
        Ok(entry)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ork-setup-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A stand-in for the installed program. Nothing here runs it; these
    /// tests are about where files end up, not about what they do.
    fn planted_program(directory: &Path) -> PathBuf {
        let program = directory.join(program_name());
        fs::write(&program, b"not really a program").unwrap();
        program
    }

    #[test]
    fn the_terminal_script_opens_the_program_and_then_stays_open() {
        // The whole reason the script exists. Without the last line the
        // window closes the instant the program finishes printing, which is
        // the failure this replaced: a shortcut that flashes and vanishes,
        // indistinguishable from a program that crashed on start-up.
        let dir = scratch("shim");
        let shim = write_terminal_shim(&dir).unwrap();
        let text = fs::read_to_string(&shim).unwrap();

        assert!(
            text.contains("outlaw"),
            "the script does not open the program:\n{text}"
        );
        let stays_open = text.contains("cmd /k") || text.contains("exec \"${SHELL:-/bin/sh}\"");
        assert!(stays_open, "the script does not stay open:\n{text}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_terminal_script_says_what_it_is_for() {
        // It is a file this installer leaves on somebody's computer. A file
        // whose purpose cannot be worked out by reading it is one more thing
        // to be suspicious of.
        let dir = scratch("shim-comment");
        let text = fs::read_to_string(write_terminal_shim(&dir).unwrap()).unwrap();
        assert!(
            text.to_ascii_lowercase().contains("outlaw repair kit"),
            "the script does not name itself:\n{text}"
        );
        assert!(
            text.to_ascii_lowercase().contains("deleting"),
            "the script does not say it is safe to delete:\n{text}"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn the_terminal_script_can_be_run() {
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch("shim-mode");
        let shim = write_terminal_shim(&dir).unwrap();
        let mode = fs::metadata(&shim).unwrap().permissions().mode();
        assert!(mode & 0o111 != 0, "the script is not executable: {mode:o}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_program_ends_up_in_the_directory_that_goes_on_the_path() {
        // The bug this was written for. On Linux the program is installed
        // under `~/.local/share` and `~/.local/bin` is what goes on PATH, so
        // without this step an install finished successfully and `outlaw`
        // answered "command not found".
        let dir = scratch("link-from");
        let bin = scratch("link-to");
        let program = planted_program(&dir);

        let link = link_into_path(&program, &bin).unwrap().unwrap();
        assert_eq!(link, bin.join(program_name()));
        assert!(link.exists(), "{} was not created", link.display());

        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&bin);
    }

    #[test]
    fn installing_twice_replaces_the_link_rather_than_failing() {
        // Upgrading is the ordinary case, and a link left over from the last
        // version is precisely the thing this has to correct.
        let dir = scratch("link-again-from");
        let bin = scratch("link-again-to");
        let program = planted_program(&dir);

        link_into_path(&program, &bin).unwrap().unwrap();
        let again = link_into_path(&program, &bin);
        assert!(again.is_ok(), "second install failed: {again:?}");
        assert!(again.unwrap().is_some());

        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&bin);
    }

    #[test]
    fn nothing_is_linked_when_the_program_is_already_where_the_path_points() {
        // Windows installs straight into the directory that goes on PATH.
        // Copying a file onto itself is at best pointless and at worst
        // destroys it.
        let dir = scratch("link-same");
        let program = planted_program(&dir);
        assert_eq!(link_into_path(&program, &dir).unwrap(), None);
        assert!(program.exists(), "the program was destroyed");
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(not(windows))]
    #[test]
    fn a_desktop_entry_for_the_terminal_program_asks_for_a_terminal() {
        // `Terminal=false` on a command-line program is the Linux half of the
        // flashing-console bug: clicking the entry does nothing visible at
        // all, and there is no error anywhere to explain why.
        let dir = scratch("entry");
        let shim = write_terminal_shim(&dir).unwrap();
        let entry = make_shortcut(&shim, "Outlaw Repair Kit (terminal)", Opens::ATerminal).unwrap();
        let text = fs::read_to_string(&entry).unwrap();

        assert!(text.contains("Terminal=true"), "{text}");
        assert!(text.contains(&shim.display().to_string()), "{text}");
        let _ = fs::remove_file(&entry);
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(not(windows))]
    #[test]
    fn a_desktop_entry_for_the_window_does_not_ask_for_a_terminal() {
        let dir = scratch("entry-window");
        let window = dir.join(ork_core::ways_in::window_file_name());
        fs::write(&window, b"not really a window").unwrap();
        let entry = make_shortcut(&window, "Outlaw Repair Kit", Opens::AWindow).unwrap();
        let text = fs::read_to_string(&entry).unwrap();

        assert!(text.contains("Terminal=false"), "{text}");
        let _ = fs::remove_file(&entry);
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(not(windows))]
    #[test]
    fn the_two_desktop_entries_do_not_overwrite_each_other() {
        // Both are wanted at once on a machine with both front-ends. One file
        // name for both would mean installing the second silently removed the
        // first.
        let dir = scratch("entry-both");
        let shim = write_terminal_shim(&dir).unwrap();
        let window = dir.join(ork_core::ways_in::window_file_name());
        fs::write(&window, b"not really a window").unwrap();

        let terminal = make_shortcut(&shim, "terminal", Opens::ATerminal).unwrap();
        let app = make_shortcut(&window, "window", Opens::AWindow).unwrap();
        assert_ne!(terminal, app);

        let _ = fs::remove_file(&terminal);
        let _ = fs::remove_file(&app);
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(windows)]
    fn user_variable(name: &str) -> Option<String> {
        let script = format!("[Environment]::GetEnvironmentVariable('{name}', 'User')");
        let output = ork_core::platform::run_capture(
            "powershell",
            &["-NoProfile", "-NonInteractive", "-Command", &script],
        )
        .ok()?;
        let value = output.stdout.trim().to_string();
        if value.is_empty() { None } else { Some(value) }
    }

    #[cfg(windows)]
    fn clear_user_variable(name: &str) {
        let script = format!("[Environment]::SetEnvironmentVariable('{name}', $null, 'User')");
        let _ = ork_core::platform::run_capture(
            "powershell",
            &["-NoProfile", "-NonInteractive", "-Command", &script],
        );
    }

    #[cfg(windows)]
    #[test]
    fn appending_to_a_variable_adds_once_and_keeps_what_was_there() {
        // The most dangerous thing this program does on Windows is rewrite an
        // environment variable that other software depends on. Pointed at a
        // scratch variable here, so the behaviour can be checked without
        // experimenting on somebody's real PATH -- which, if this were wrong,
        // is exactly what would be destroyed.
        const SCRATCH: &str = "ORK_SETUP_PATH_TEST";
        clear_user_variable(SCRATCH);

        let first = Path::new(r"C:\ork-test-one");
        let second = Path::new(r"C:\ork-test-two");

        // From nothing.
        append_to_user_variable(SCRATCH, first).unwrap();
        assert_eq!(user_variable(SCRATCH).as_deref(), Some(r"C:\ork-test-one"));

        // A second directory is appended, and the first survives. This is the
        // property that matters: a PATH that loses its existing entries breaks
        // every other program on the machine.
        append_to_user_variable(SCRATCH, second).unwrap();
        let both = user_variable(SCRATCH).unwrap();
        assert!(both.contains("ork-test-one"), "{both}");
        assert!(both.contains("ork-test-two"), "{both}");

        // Adding the same one again changes nothing -- no duplicate entries
        // from running the installer twice.
        append_to_user_variable(SCRATCH, second).unwrap();
        assert_eq!(user_variable(SCRATCH).unwrap(), both);

        clear_user_variable(SCRATCH);
        assert_eq!(
            user_variable(SCRATCH),
            None,
            "the test cleaned up after itself"
        );
    }

    #[test]
    fn a_file_is_placed_whole_or_not_at_all() {
        let dir = scratch("place");
        let path = place(&dir, "outlaw-test.bin", b"contents").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"contents");

        // Nothing half-written is left lying around under a name that looks
        // like a finished file.
        let leftovers: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.contains(".incoming"))
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn installing_over_an_existing_copy_replaces_it() {
        let dir = scratch("replace");
        place(&dir, "outlaw-test.bin", b"old").unwrap();
        let path = place(&dir, "outlaw-test.bin", b"new").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"new");

        tidy(&dir);
        let names: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["outlaw-test.bin"], "{names:?}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_record_survives_being_written_and_read_back() {
        // The point of the record is that somebody can find out later what was
        // done. A record that cannot be read back is a record of nothing.
        let dir = scratch("receipt");
        let receipt = Receipt {
            version: "v0.6.0".to_string(),
            installed_at: "2026-08-26 12:00:00".to_string(),
            directory: dir.display().to_string(),
            steps: vec![
                Step::Wrote {
                    path: "outlaw.exe".to_string(),
                    sha256: "abc".to_string(),
                },
                Step::AddedToPath {
                    directory: dir.display().to_string(),
                },
                Step::Delegated {
                    what: "Ollama".to_string(),
                    command: "winget install --id Ollama.Ollama -e".to_string(),
                },
            ],
        };
        receipt.write(&dir).unwrap();

        let back = Receipt::read(&dir).expect("the record reads back");
        assert_eq!(back.version, "v0.6.0");
        assert_eq!(back.steps.len(), 3);
        assert_eq!(back.steps, receipt.steps);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_record_is_absence_rather_than_an_error() {
        let dir = scratch("norecord");
        assert!(Receipt::read(&dir).is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_directory_already_on_path_is_not_added_twice() {
        // Whatever is first on PATH, this must agree that it is on PATH.
        let first = std::env::var("PATH")
            .ok()
            .and_then(|path| std::env::split_paths(&path).next());
        if let Some(directory) = first {
            assert!(already_on_path(&directory), "{}", directory.display());
        }
        assert!(!already_on_path(Path::new(
            "/definitely-not-on-anybodys-path-9f3a"
        )));
    }

    #[test]
    fn the_program_is_named_for_the_platform_it_runs_on() {
        if cfg!(windows) {
            assert_eq!(program_name(), "outlaw.exe");
        } else {
            assert_eq!(program_name(), "outlaw");
        }
    }
}
