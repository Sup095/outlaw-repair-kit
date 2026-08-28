//! Everything that has arranged to start itself.
//!
//! This is the enumeration half. Deciding which of these is worth saying
//! something about is [`crate::probes::startup`], which is deliberately pure
//! so that the judgement can be tested against entries no machine here has.
//!
//! ## Why this is worth a check of its own
//!
//! Two quite different problems live in the same list.
//!
//! The first is that everything unwanted survives a restart by putting itself
//! here. That is not a claim that anything found here is unwanted -- almost
//! all of it is a printer utility and a chat program -- but anything that
//! wants to still be running tomorrow has to be in this list somewhere, and
//! nothing that is not in it will be.
//!
//! The second is much more ordinary and affects far more people: a machine
//! that is slow from the moment it starts is usually a machine with twenty
//! things starting alongside it, most of which were installed alongside
//! something else and none of which the person ever chose.
//!
//! ## What is deliberately left out
//!
//! On Windows, the several hundred scheduled tasks under `\Microsoft\` that
//! come with the operating system. They are all normal, they would bury
//! everything else, and a list nobody reads is worse than no list.
//!
//! On Linux, system-wide systemd units. Almost every one of them was put there
//! by a package, the package manager already knows about them, and listing
//! four hundred would drown the handful a person actually installed by hand.
//! User units, which by definition are not package-managed, are included.

use serde::{Deserialize, Serialize};

use crate::Result;

/// One thing that starts by itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StartupEntry {
    /// What it calls itself.
    pub name: String,
    /// Where it was found, in words a person could go and look at.
    pub source: String,
    /// The command line as written, before anything is interpreted.
    pub command: String,
    /// The executable worked out from that command line, where one could be.
    pub program: Option<String>,
    /// Whether that executable is actually there. `None` when it could not be
    /// worked out at all, which is not the same as it being missing.
    pub program_exists: Option<bool>,
    /// Whether this starts for everybody on the machine or for one account.
    pub for_all_users: bool,
}

impl StartupEntry {
    /// Fill in `program` and `program_exists`.
    ///
    /// `program` is worked out from `command` only when the enumerator did not
    /// already know it. Several of them do: a shortcut in a start-up folder
    /// states its target, and a scheduled task states what it executes as a
    /// field of its own. Guessing in those cases would be throwing away an
    /// exact answer in favour of parsing -- which is how three shortcuts on
    /// this developer's machine were reported as missing programs, their paths
    /// having been cut at the first space in "Start Menu".
    pub fn resolved(mut self) -> Self {
        match &self.program {
            // Trusted, but still tidied. A scheduled task stores what it
            // executes with quotes around it whenever the path has a space in
            // it, and a path with quotation marks in the middle of it is a
            // path that does not exist -- which reported two programs sitting
            // exactly where they should be as missing.
            Some(known) => self.program = Some(unquote(known).to_string()),
            None => self.program = crate::probes::startup::extract_program(&self.command),
        }
        self.program = self.program.take().filter(|program| !program.is_empty());
        self.program_exists = self.program.as_deref().and_then(program_exists);
        self
    }
}

/// A path with the quotation marks taken off, if it had any.
///
/// Only a matched pair, and only at the ends. A quotation mark anywhere else
/// is part of the name, however unlikely that is.
pub fn unquote(path: &str) -> &str {
    let path = path.trim();
    match path
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
    {
        Some(inner) => inner,
        None => path,
    }
}

/// Whether a program is there, or `None` when that cannot be answered.
///
/// The distinction matters more than it looks. A path is a path and can simply
/// be looked for. A bare name -- which is how a scheduled task often states
/// what it runs -- may be found on the PATH, or may sit in a working directory
/// the task carries and this does not. Not finding one is therefore not
/// knowing, and reporting it as missing would be an accusation built out of
/// something we did not look at.
pub fn program_exists(program: &str) -> Option<bool> {
    let path = std::path::Path::new(program);
    if program.contains(['/', '\\']) {
        return Some(path.exists());
    }
    super::common::which(program).map(|_| true)
}

/// Everything on this machine that starts itself, as far as can be seen
/// without administrator rights.
pub fn entries() -> Result<Vec<StartupEntry>> {
    #[cfg(windows)]
    {
        windows_entries()
    }
    #[cfg(target_os = "linux")]
    {
        linux_entries()
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        Ok(Vec::new())
    }
}

/// Whether `/etc/ld.so.preload` is present, and what is in it.
///
/// Its own function, and not part of the list above, because it is not a
/// startup entry -- it is a file that makes a library load into *every*
/// program on the machine. On an ordinary desktop it simply does not exist.
pub fn library_preload() -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        std::fs::read_to_string("/etc/ld.so.preload")
            .ok()
            .filter(|text| !text.trim().is_empty())
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

/// One entry as PowerShell hands it over.
#[derive(Debug, Deserialize)]
struct Raw {
    name: Option<String>,
    source: Option<String>,
    command: Option<String>,
    /// Set where the source already knows exactly what runs, rather than
    /// leaving it to be read back out of a command line.
    program: Option<String>,
    machine: Option<bool>,
}

/// Read what PowerShell sent back.
///
/// Its own function, and not behind a `cfg`, because the two things most
/// likely to go wrong here can both be checked without a Windows machine and
/// neither was being checked at all:
///
/// * **One result comes back as an object, not a list.** `ConvertTo-Json`
///   does that whenever the collection has a single element, and it is the
///   classic way a parser that works everywhere breaks on the one machine
///   with exactly one start-up entry.
/// * **Nothing at all comes back** when every source was unavailable, and an
///   empty answer must be an empty list rather than an error.
fn read_entries(text: &str) -> Vec<StartupEntry> {
    let text = text.trim();
    if text.is_empty() {
        return Vec::new();
    }
    let raw: Vec<Raw> = match serde_json::from_str::<Vec<Raw>>(text) {
        Ok(list) => list,
        Err(_) => match serde_json::from_str::<Raw>(text) {
            Ok(one) => vec![one],
            Err(error) => {
                tracing::debug!(%error, "could not read the list of start-up entries");
                return Vec::new();
            }
        },
    };

    raw.into_iter()
        .filter_map(|entry| {
            let command = entry.command?;
            if command.trim().is_empty() {
                return None;
            }
            Some(
                StartupEntry {
                    name: entry.name.unwrap_or_else(|| "unnamed".to_string()),
                    source: entry.source.unwrap_or_default(),
                    command,
                    program: entry
                        .program
                        .map(|program| program.trim().to_string())
                        .filter(|program| !program.is_empty()),
                    program_exists: None,
                    for_all_users: entry.machine.unwrap_or(false),
                }
                .resolved(),
            )
        })
        .collect()
}

#[cfg(windows)]
fn windows_entries() -> Result<Vec<StartupEntry>> {
    use super::common;

    // One PowerShell invocation for all three places, because three would be
    // three chances to be slow on a machine that is already unwell.
    //
    // Every part is wrapped so that one unavailable source -- a policy that
    // blocks the scheduler module, a registry key that is not there -- costs
    // that source rather than the whole check.
    const SCRIPT: &str = r#"
$found = New-Object System.Collections.ArrayList
$keys = @(
  'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Run',
  'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\RunOnce',
  'HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Run',
  'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Run',
  'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\RunOnce'
)
foreach ($key in $keys) {
  try {
    if (-not (Test-Path $key)) { continue }
    $item = Get-ItemProperty -Path $key -ErrorAction Stop
    foreach ($property in $item.PSObject.Properties) {
      if ($property.Name -like 'PS*') { continue }
      [void]$found.Add([pscustomobject]@{
        name = $property.Name
        source = $key
        command = [string]$property.Value
        machine = ($key -like 'HKLM*')
      })
    }
  } catch {}
}
$common = [Environment]::GetFolderPath('CommonStartup')
try { $shell = New-Object -ComObject WScript.Shell } catch { $shell = $null }
foreach ($folder in @([Environment]::GetFolderPath('Startup'), $common)) {
  try {
    if (-not $folder -or -not (Test-Path $folder)) { continue }
    foreach ($file in Get-ChildItem -Path $folder -File -ErrorAction Stop) {
      $line = $file.FullName
      $target = $file.FullName
      # Almost everything in these folders is a shortcut. Following it is the
      # difference between checking the shortcut and checking the program --
      # and only the second one can notice something starting out of a
      # temporary folder.
      if ($file.Extension -eq '.lnk' -and $shell) {
        try {
          $link = $shell.CreateShortcut($file.FullName)
          if ($link.TargetPath) {
            $target = $link.TargetPath
            $line = $link.TargetPath
            if ($link.Arguments) { $line = $line + ' ' + $link.Arguments }
          }
        } catch {}
      }
      [void]$found.Add([pscustomobject]@{
        name = $file.Name
        source = $folder
        command = $line
        program = $target
        machine = ($folder -eq $common)
      })
    }
  } catch {}
}
try {
  foreach ($task in Get-ScheduledTask -ErrorAction Stop) {
    if ($task.TaskPath -like '\Microsoft\*') { continue }
    $starts = $false
    foreach ($trigger in $task.Triggers) {
      $kind = $trigger.CimClass.CimClassName
      if ($kind -like '*Logon*' -or $kind -like '*Boot*' -or $kind -like '*Startup*') { $starts = $true }
    }
    if (-not $starts) { continue }
    $action = $task.Actions | Select-Object -First 1
    $line = ''
    if ($action.Execute) {
      $line = $action.Execute
      if ($action.Arguments) { $line = $line + ' ' + $action.Arguments }
    }
    if (-not $line) { continue }
    [void]$found.Add([pscustomobject]@{
      name = $task.TaskName
      source = 'Scheduled task ' + $task.TaskPath + $task.TaskName
      command = $line
      program = $action.Execute
      machine = $true
    })
  }
} catch {}
$found | ConvertTo-Json -Compress -Depth 3
"#;

    let output = common::run_capture("powershell", &["-NoProfile", "-Command", SCRIPT])?;
    Ok(read_entries(&output.stdout))
}

#[cfg(target_os = "linux")]
fn linux_entries() -> Result<Vec<StartupEntry>> {
    let home = dirs::home_dir();
    Ok(walk(
        &[
            (
                home.as_ref().map(|home| home.join(".config/autostart")),
                false,
            ),
            (Some(std::path::PathBuf::from("/etc/xdg/autostart")), true),
        ],
        home.as_ref()
            .map(|home| home.join(".config/systemd/user"))
            .as_deref(),
    ))
}

/// Read start-up entries out of the directories they live in.
///
/// Takes the directories rather than knowing them, so that the whole walk --
/// which is most of what the Linux side of this consists of -- can be run
/// against a made-up machine on any operating system. Before this it was
/// executed by nothing at all: the two parsers below had tests, and the code
/// that finds files for them to parse did not.
#[cfg(any(target_os = "linux", test))]
pub fn walk(
    autostart: &[(Option<std::path::PathBuf>, bool)],
    user_units: Option<&std::path::Path>,
) -> Vec<StartupEntry> {
    let mut found = Vec::new();

    for (directory, for_all_users) in autostart {
        let for_all_users = *for_all_users;
        let Some(directory) = directory else { continue };
        let Ok(listing) = std::fs::read_dir(&directory) else {
            continue;
        };
        for item in listing.flatten() {
            let path = item.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("desktop") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            let Some((name, command)) = desktop_entry(&text) else {
                continue;
            };
            found.push(
                StartupEntry {
                    name,
                    source: path.display().to_string(),
                    command,
                    program: None,
                    program_exists: None,
                    for_all_users,
                }
                .resolved(),
            );
        }
    }

    // User systemd units, which by definition were not put there by a package.
    if let Some(units) = user_units {
        if let Ok(listing) = std::fs::read_dir(units) {
            for item in listing.flatten() {
                let path = item.path();
                if path.extension().and_then(|ext| ext.to_str()) != Some("service") {
                    continue;
                }
                let Ok(text) = std::fs::read_to_string(&path) else {
                    continue;
                };
                let Some(command) = unit_exec_start(&text) else {
                    continue;
                };
                found.push(
                    StartupEntry {
                        name: path
                            .file_name()
                            .map(|name| name.to_string_lossy().into_owned())
                            .unwrap_or_else(|| "unnamed".to_string()),
                        source: path.display().to_string(),
                        command,
                        program: None,
                        program_exists: None,
                        for_all_users: false,
                    }
                    .resolved(),
                );
            }
        }
    }

    found
}

/// The name and command out of a `.desktop` file.
///
/// Not a general parser: it wants the two lines that say what this is and what
/// it runs, and a desktop file that is missing either is not a startup entry
/// anybody can act on.
#[cfg(any(target_os = "linux", test))]
pub fn desktop_entry(text: &str) -> Option<(String, String)> {
    let mut name = None;
    let mut exec = None;
    for line in text.lines() {
        let line = line.trim();
        // Only the plain keys. `Name[de]` is the same entry in German, and
        // taking whichever came last would report a name in a language the
        // reader may not have.
        if let Some(rest) = line.strip_prefix("Name=") {
            name.get_or_insert(rest.trim().to_string());
        } else if let Some(rest) = line.strip_prefix("Exec=") {
            exec.get_or_insert(rest.trim().to_string());
        }
    }
    let exec = exec?;
    Some((name.unwrap_or_else(|| exec.clone()), exec))
}

/// The `ExecStart=` line out of a systemd unit.
#[cfg(any(target_os = "linux", test))]
pub fn unit_exec_start(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .find_map(|line| line.strip_prefix("ExecStart="))
        .map(|rest| rest.trim().to_string())
        .filter(|rest| !rest.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_quoted_path_from_a_source_that_states_one_loses_the_quotes() {
        // Windows stores a scheduled task's program quoted whenever the path
        // contains a space. Kept, those quotes make the path one that cannot
        // exist -- which reported two programs sitting exactly where they
        // should be as missing.
        let quoted = StartupEntry {
            name: "Thing".into(),
            source: "Scheduled task".into(),
            command: "\"C:/Program Files/Thing/thing.exe\" -a".into(),
            program: Some("\"C:/Program Files/Thing/thing.exe\"".into()),
            program_exists: None,
            for_all_users: true,
        }
        .resolved();
        assert_eq!(
            quoted.program.as_deref(),
            Some("C:/Program Files/Thing/thing.exe")
        );
    }

    #[test]
    fn only_a_matched_pair_of_quotes_is_taken_off() {
        assert_eq!(unquote("\"a b\""), "a b");
        assert_eq!(unquote("a b"), "a b");
        assert_eq!(unquote("\"a b"), "\"a b");
        assert_eq!(unquote("a\"b"), "a\"b");
    }

    /// A directory of files, cleaned up when it goes out of scope.
    struct Scratch(std::path::PathBuf);

    impl Scratch {
        fn new(label: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("ork-startup-{label}"));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("a scratch directory");
            Self(dir)
        }

        fn write(&self, name: &str, contents: &str) -> &Self {
            std::fs::write(self.0.join(name), contents).expect("a file");
            self
        }

        fn path(&self) -> std::path::PathBuf {
            self.0.clone()
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn the_walk_finds_autostart_files_and_says_who_they_are_for() {
        // The whole Linux side of the enumeration, run against a made-up
        // machine. Before this the two parsers had tests and the code that
        // finds files for them to parse was executed by nothing at all.
        let mine = Scratch::new("mine");
        mine.write(
            "backup.desktop",
            "[Desktop Entry]\nName=Backup helper\nExec=/usr/bin/backup --daemon\n",
        )
        .write("notes.txt", "not a desktop file, and not an entry")
        // A file an editor left behind, holding a command that would parse
        // perfectly well. Nothing starts it, so reporting it would be
        // inventing a start-up entry out of a backup file.
        .write(
            "backup.desktop.bak",
            "[Desktop Entry]\nName=Old backup helper\nExec=/usr/bin/backup --old\n",
        )
        .write("broken.desktop", "[Desktop Entry]\nName=No command\n");

        let everyones = Scratch::new("everyones");
        everyones.write(
            "printer.desktop",
            "[Desktop Entry]\nName=Printer applet\nExec=/usr/bin/printer-applet\n",
        );

        let found = walk(
            &[
                (Some(mine.path()), false),
                (Some(everyones.path()), true),
                // A directory that is not there is an ordinary state, not a
                // failure: plenty of machines have no /etc/xdg/autostart.
                (Some(std::path::PathBuf::from("/definitely/not/here")), true),
                (None, false),
            ],
            None,
        );

        let names: Vec<&str> = found.iter().map(|entry| entry.name.as_str()).collect();
        assert!(names.contains(&"Backup helper"), "{names:?}");
        assert!(names.contains(&"Printer applet"), "{names:?}");
        assert!(
            !names.contains(&"Old backup helper"),
            "reported a leftover backup file as something that starts: {names:?}"
        );
        assert_eq!(found.len(), 2, "{names:?}");

        let backup = found
            .iter()
            .find(|entry| entry.name == "Backup helper")
            .unwrap();
        assert_eq!(backup.command, "/usr/bin/backup --daemon");
        assert_eq!(backup.program.as_deref(), Some("/usr/bin/backup"));
        assert!(!backup.for_all_users);

        let printer = found
            .iter()
            .find(|entry| entry.name == "Printer applet")
            .unwrap();
        assert!(printer.for_all_users, "a shared entry should say so");
    }

    #[test]
    fn the_walk_finds_user_systemd_units_and_ignores_everything_else() {
        // User units only, because a system unit was almost certainly put
        // there by a package and listing four hundred of those would drown
        // the one somebody wrote by hand.
        let units = Scratch::new("units");
        units
            .write(
                "sync.service",
                "[Unit]\nDescription=Sync\n\n[Service]\nExecStart=/usr/bin/sync-thing --serve\n",
            )
            .write("sync.timer", "[Timer]\nOnCalendar=daily\n")
            .write("empty.service", "[Unit]\nDescription=Nothing\n");

        let found = walk(&[], Some(&units.path()));
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].name, "sync.service");
        assert_eq!(found[0].command, "/usr/bin/sync-thing --serve");
        assert!(!found[0].for_all_users);
    }

    #[test]
    fn a_machine_with_nowhere_to_look_produces_an_empty_list_not_a_failure() {
        assert!(walk(&[], None).is_empty());
        assert!(walk(&[(None, false)], Some(std::path::Path::new("/nope"))).is_empty());
    }

    #[test]
    fn a_list_of_entries_is_read_back() {
        let text = r#"[
          {"name":"Thing","source":"HKLM","command":"C:\\Thing\\thing.exe -q","machine":true},
          {"name":"Other","source":"HKCU","command":"C:\\Other\\other.exe","machine":false}
        ]"#;
        let found = read_entries(text);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].name, "Thing");
        assert!(found[0].for_all_users);
        assert!(!found[1].for_all_users);
    }

    #[test]
    fn a_single_entry_arriving_as_an_object_is_still_read() {
        // PowerShell's `ConvertTo-Json` emits an object rather than a list
        // whenever the collection holds exactly one thing. A parser that only
        // understands lists works on every machine except the one with a
        // single start-up entry, and that machine is not rare.
        let text =
            r#"{"name":"Only","source":"HKCU","command":"C:\\Only\\only.exe","machine":false}"#;
        let found = read_entries(text);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "Only");
    }

    #[test]
    fn a_stated_program_is_used_and_a_missing_one_is_worked_out() {
        let text = r#"[
          {"name":"Task","source":"Scheduled task","command":"\"C:\\P F\\t.exe\" -a",
           "program":"\"C:\\P F\\t.exe\"","machine":true},
          {"name":"Key","source":"HKCU","command":"C:\\P F\\k.exe --go","machine":false}
        ]"#;
        let found = read_entries(text);
        assert_eq!(found[0].program.as_deref(), Some("C:\\P F\\t.exe"));
        assert_eq!(found[1].program.as_deref(), Some("C:\\P F\\k.exe"));
    }

    #[test]
    fn nothing_at_all_is_an_empty_list_rather_than_an_error() {
        // Every source unavailable is an ordinary outcome -- a locked-down
        // machine, a policy blocking the scheduler -- and must not lose the
        // whole check.
        assert!(read_entries("").is_empty());
        assert!(read_entries("   \n  ").is_empty());
    }

    #[test]
    fn output_that_is_not_json_at_all_loses_the_check_and_not_the_scan() {
        // PowerShell writing a warning where JSON was expected happens, and
        // the answer is no entries rather than a failed scan.
        assert!(read_entries("At line:1 char:1 + something went wrong").is_empty());
    }

    #[test]
    fn an_entry_that_runs_nothing_is_dropped() {
        let text = r#"[
          {"name":"Empty","source":"HKCU","command":"   ","machine":false},
          {"name":"NoCommand","source":"HKCU","machine":false}
        ]"#;
        assert!(read_entries(text).is_empty());
    }

    #[test]
    fn a_path_that_is_there_and_one_that_is_not_are_told_apart() {
        let here = std::env::current_exe().expect("this test binary exists");
        assert_eq!(program_exists(&here.display().to_string()), Some(true));

        let nowhere = here.with_file_name("definitely-not-a-real-program-9f2a");
        assert_eq!(program_exists(&nowhere.display().to_string()), Some(false));
    }

    #[test]
    fn a_bare_name_that_is_not_on_the_path_is_not_knowing_rather_than_missing() {
        // A scheduled task often states a bare name and carries a working
        // directory this cannot see. Calling that missing is an accusation
        // built out of somewhere we did not look -- and it is exactly what
        // reported Intel's Thunderbolt task as broken on a machine where it
        // is fine.
        assert_eq!(
            program_exists("definitely-not-a-real-program-9f2a.exe"),
            None
        );
    }

    #[test]
    fn a_bare_name_that_is_on_the_path_is_found() {
        let known = if cfg!(windows) { "cmd.exe" } else { "sh" };
        assert_eq!(program_exists(known), Some(true), "could not find {known}");
    }

    #[test]
    fn a_program_the_enumerator_already_knew_is_not_guessed_at_again() {
        // The shortcut case. Parsing `...\Start Menu\Programs\Startup\X.lnk`
        // out of a command line cuts it at the first space; the enumerator
        // knows the real target, and that must win.
        let entry = StartupEntry {
            name: "Thing".into(),
            source: "a folder".into(),
            command: r"C:\Some Place\With Spaces\thing.lnk".into(),
            program: Some(r"C:\Program Files\Thing\thing.exe".into()),
            program_exists: None,
            for_all_users: false,
        }
        .resolved();
        assert_eq!(
            entry.program.as_deref(),
            Some(r"C:\Program Files\Thing\thing.exe")
        );
    }

    #[test]
    fn a_desktop_file_gives_up_its_name_and_command() {
        let text = "[Desktop Entry]\nType=Application\nName=Backup helper\nExec=/usr/bin/backup --daemon\n";
        assert_eq!(
            desktop_entry(text),
            Some((
                "Backup helper".to_string(),
                "/usr/bin/backup --daemon".to_string()
            ))
        );
    }

    #[test]
    fn a_translated_name_does_not_replace_the_plain_one() {
        // `Name[de]` starts with `Name` but is not it, and a naive prefix
        // match would report whichever translation happened to come last.
        let text = "[Desktop Entry]\nName=Backup helper\nName[de]=Sicherungshelfer\nExec=/usr/bin/backup\n";
        let (name, _) = desktop_entry(text).unwrap();
        assert_eq!(name, "Backup helper");
    }

    #[test]
    fn a_desktop_file_that_runs_nothing_is_not_an_entry() {
        let text = "[Desktop Entry]\nType=Directory\nName=Somewhere\n";
        assert_eq!(desktop_entry(text), None);
    }

    #[test]
    fn a_nameless_desktop_file_is_named_by_what_it_runs() {
        // Better than "unnamed": the command is the thing a person would
        // search for anyway.
        let text = "[Desktop Entry]\nExec=/opt/thing/thing\n";
        let (name, _) = desktop_entry(text).unwrap();
        assert_eq!(name, "/opt/thing/thing");
    }

    #[test]
    fn a_unit_gives_up_the_first_thing_it_starts() {
        let text = "[Unit]\nDescription=Thing\n\n[Service]\nExecStart=/usr/bin/thing --serve\nExecStop=/usr/bin/thing --stop\n";
        assert_eq!(
            unit_exec_start(text),
            Some("/usr/bin/thing --serve".to_string())
        );
    }

    #[test]
    fn a_unit_that_starts_nothing_reports_nothing() {
        assert_eq!(unit_exec_start("[Unit]\nDescription=Thing\n"), None);
        assert_eq!(unit_exec_start("[Service]\nExecStart=\n"), None);
    }
}
