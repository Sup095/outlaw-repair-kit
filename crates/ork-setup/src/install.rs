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
    /// A shortcut or desktop entry was created.
    Shortcut { path: String },
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

/// Make the desktop application reachable the way this platform expects.
///
/// Best-effort by design. A missing shortcut is a nuisance somebody can work
/// around in ten seconds; failing an otherwise good install over one would be
/// out of all proportion.
pub fn make_shortcut(target: &Path, label: &str) -> Result<PathBuf> {
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

        let script = format!(
            "$shell = New-Object -ComObject WScript.Shell; \
             $link = $shell.CreateShortcut('{}'); \
             $link.TargetPath = '{}'; \
             $link.WorkingDirectory = '{}'; \
             $link.Description = 'Outlaw Repair Kit'; \
             $link.Save()",
            link.display().to_string().replace('\'', "''"),
            target.display().to_string().replace('\'', "''"),
            target
                .parent()
                .unwrap_or(target)
                .display()
                .to_string()
                .replace('\'', "''"),
        );
        let output = ork_core::platform::run_capture(
            "powershell",
            &["-NoProfile", "-NonInteractive", "-Command", &script],
        )
        .context("could not create a Start Menu shortcut")?;
        if !output.success {
            anyhow::bail!("could not create a Start Menu shortcut");
        }
        Ok(link)
    }

    #[cfg(not(windows))]
    {
        let applications = dirs::data_dir()
            .context("this account has no data directory")?
            .join("applications");
        fs::create_dir_all(&applications)?;
        let entry = applications.join("outlaw-repair-kit.desktop");
        let contents = format!(
            "[Desktop Entry]\n\
             Type=Application\n\
             Name={label}\n\
             Comment=Scan a computer for problems, in plain language\n\
             Exec={}\n\
             Terminal=false\n\
             Categories=System;Utility;\n",
            target.display()
        );
        fs::write(&entry, contents)
            .with_context(|| format!("could not write {}", entry.display()))?;
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
