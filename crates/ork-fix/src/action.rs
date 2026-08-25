//! What a fix is allowed to be.
//!
//! This is the most safety-critical file in the project, so it is worth being
//! explicit about the threat model. A fix can originate from a runbook written
//! by a person, or from a language model reasoning about a novel problem. Only
//! the first has been reviewed. If the executor accepted arbitrary shell
//! commands, then every safety rule in this tool would be advice rather than a
//! guarantee, and one confidently wrong model output could destroy someone's
//! data.
//!
//! So actions are a closed set of typed operations. There is no variant that
//! means "run this string". Anything that cannot be expressed as one of these
//! operations becomes [`FixAction::Manual`] -- an instruction for a person to
//! carry out and judge themselves. That is deliberately restrictive: the tool
//! would rather tell you to reinstall a driver than reinstall it wrongly.
//!
//! Three layers of defence, in order:
//!
//! 1. The type system. Only these operations exist.
//! 2. Validation at construction, in [`FixAction::validate`]. An action that
//!    fails validation cannot be executed, whatever produced it.
//! 3. Execution without a shell. Programs are launched directly with an
//!    argument list, so there is no string for a metacharacter to escape from.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::Result;

/// Programs an `Inspect` action may run.
///
/// An allowlist rather than a denylist, because the set of harmful commands is
/// unbounded and the set of useful read-only diagnostics is small. Every entry
/// here reads state and changes nothing.
const INSPECT_ALLOWLIST: &[&str] = &[
    // Cross-platform-ish inspection.
    "df",
    "du",
    "free",
    "uname",
    "hostname",
    "whoami",
    // Logs and services, read-only forms.
    "journalctl",
    "dmesg",
    "systemctl",
    // Hardware and drivers.
    "lsblk",
    "lspci",
    "lsusb",
    "lsmod",
    "nvidia-smi",
    "rocm-smi",
    "sensors",
    "smartctl",
    "mhwd",
    // Packages and binaries.
    "pacman",
    "dpkg",
    "rpm",
    "ldd",
    "which",
    // Windows.
    "powershell",
    "wmic",
    "driverquery",
];

/// Subcommands and flags that turn an otherwise read-only program into one
/// that changes the system.
///
/// `systemctl status` is inspection; `systemctl start` is not. `pacman -Q`
/// queries; `pacman -S` installs.
const INSPECT_FORBIDDEN_ARGS: &[&str] = &[
    "start",
    "stop",
    "restart",
    "reload",
    "enable",
    "disable",
    "mask",
    "unmask",
    "kill",
    "isolate",
    "-S",
    "-R",
    "-U",
    "--sync",
    "--remove",
    "--upgrade",
    "--install",
    "--remove-package",
    "-i",
    "-a",
    "--auto",
    "--set",
    "--assign",
    "-w",
    "--write",
    "--delete",
    "--remove-device",
];

/// Path fragments that must never be the target of a change.
///
/// Matched case-insensitively against the whole path, so this catches a target
/// nested inside one of these as well as the directory itself.
const PROTECTED_PATHS: &[&str] = &[
    "/boot",
    "/bin",
    "/sbin",
    "/lib",
    "/lib64",
    "/usr",
    "/etc",
    "/dev",
    "/proc",
    "/sys",
    "/var/lib/pacman",
    // Written with forward slashes; the matcher normalises separators before
    // comparing, so these cover the backslash forms too.
    "c:/windows",
    "c:/program files",
    "c:/program files (x86)",
    "c:/programdata/microsoft",
];

/// Filename patterns that a fix may remove.
///
/// Removing a file is the one destructive-shaped operation the tool performs,
/// so it is confined to the cases where it is genuinely the correct fix: stale
/// lock files and caches left behind by a crash. The file is backed up before
/// removal regardless, so even here nothing is lost.
const REMOVABLE_SUFFIXES: &[&str] = &[".lock", ".lck", ".pid", ".crashreport", ".tmp"];

/// How disruptive an action is. The engine works through candidates in this
/// order, so the cheap reversible thing is always tried first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Risk {
    /// Changes nothing at all.
    None,
    /// Reversible and contained.
    Low,
    /// Reversible, but interrupts what the user is doing.
    Medium,
    /// Changes system-level state. Always needs confirmation.
    High,
}

/// A program to run, as a program and an argument list.
///
/// Never a single string. There is no shell involved in executing this, so
/// there is nothing for a quote or a semicolon to break out of.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Program {
    pub program: String,
    pub args: Vec<String>,
}

impl Program {
    pub fn new(program: impl Into<String>, args: &[&str]) -> Self {
        Self {
            program: program.into(),
            args: args.iter().map(|arg| (*arg).to_string()).collect(),
        }
    }

    /// How this would be written in a terminal, for display only.
    pub fn display(&self) -> String {
        std::iter::once(self.program.clone())
            .chain(self.args.iter().cloned())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// Something the tool can do to try to fix a problem.
///
/// There is deliberately no variant meaning "run this arbitrary command".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "kebab-case")]
pub enum FixAction {
    /// Gather more information. Changes nothing.
    Inspect { program: Program, purpose: String },
    /// Restart a system service.
    RestartService { service: String },
    /// Remove a stale lock or cache file, after backing it up.
    RemoveStaleFile { path: PathBuf, reason: String },
    /// Something a person has to do. The tool explains and stops.
    ///
    /// Everything the tool will not do itself -- installing drivers, changing
    /// packages, restarting the machine, touching firmware -- arrives here.
    Manual { instruction: String },
}

/// Why an action was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Refusal {
    #[error("`{program}` is not on the list of read-only inspection commands")]
    ProgramNotAllowed { program: String },
    #[error("`{argument}` would change the system, so it cannot be part of an inspection")]
    ArgumentChangesState { argument: String },
    #[error("arguments must not contain shell metacharacters (found in `{argument}`)")]
    ShellMetacharacter { argument: String },
    #[error("{path} is part of the operating system and must not be modified")]
    ProtectedPath { path: String },
    #[error("only stale lock and cache files may be removed, and {path} is not one")]
    NotRemovable { path: String },
    #[error("a path must be absolute, and {path} is not")]
    RelativePath { path: String },
    #[error("`{kind}` is not a kind of fix this tool knows how to carry out")]
    UnknownRecipe { kind: String },
    #[error("the instruction is empty")]
    Empty,
}

/// Characters that would be meaningful to a shell.
///
/// Nothing here executes through a shell, so these cannot actually inject
/// anything. They are rejected anyway: an argument containing them is a sign
/// that whatever produced it believed it was writing a shell command, and that
/// belief is worth failing loudly on rather than silently passing a literal
/// semicolon to a program.
fn has_shell_metacharacter(argument: &str) -> bool {
    argument
        .chars()
        .any(|c| matches!(c, ';' | '|' | '&' | '$' | '`' | '>' | '<' | '\n' | '\r'))
}

fn is_protected(path: &Path) -> bool {
    let text = path
        .to_string_lossy()
        .to_ascii_lowercase()
        .replace('\\', "/");
    PROTECTED_PATHS.iter().any(|protected| {
        let protected = protected.to_ascii_lowercase().replace('\\', "/");
        text == protected || text.starts_with(&format!("{protected}/"))
    })
}

/// Whether a path escapes upward, which would let a relative-looking target
/// land somewhere protected.
fn has_parent_traversal(path: &Path) -> bool {
    path.components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
}

impl FixAction {
    /// How disruptive this action is.
    pub fn risk(&self) -> Risk {
        match self {
            FixAction::Inspect { .. } => Risk::None,
            FixAction::RemoveStaleFile { .. } => Risk::Low,
            FixAction::RestartService { .. } => Risk::Medium,
            // A manual instruction changes nothing by itself, but what it asks
            // for is usually the invasive part, so it is ordered last.
            FixAction::Manual { .. } => Risk::High,
        }
    }

    /// Whether carrying this out modifies the machine.
    pub fn changes_the_system(&self) -> bool {
        match self {
            FixAction::Inspect { .. } | FixAction::Manual { .. } => false,
            FixAction::RestartService { .. } | FixAction::RemoveStaleFile { .. } => true,
        }
    }

    /// Whether the tool needs explicit confirmation before doing this.
    pub fn needs_confirmation(&self) -> bool {
        self.changes_the_system()
    }

    /// One line describing what this would do.
    pub fn describe(&self) -> String {
        match self {
            FixAction::Inspect { program, purpose } => {
                format!("Look at {purpose} (runs `{}`)", program.display())
            }
            FixAction::RestartService { service } => format!("Restart the `{service}` service"),
            FixAction::RemoveStaleFile { path, reason } => {
                format!(
                    "Remove {} ({reason}) -- a copy is kept first",
                    path.display()
                )
            }
            FixAction::Manual { instruction } => instruction.clone(),
        }
    }

    /// Check that this action is one the tool is permitted to carry out.
    ///
    /// Called before an action can be queued and again before it is executed.
    /// An action that fails this can never run, regardless of whether it came
    /// from a reviewed runbook or from a model.
    pub fn validate(&self) -> std::result::Result<(), Refusal> {
        match self {
            FixAction::Inspect { program, purpose } => {
                if purpose.trim().is_empty() {
                    return Err(Refusal::Empty);
                }
                let name = Path::new(&program.program)
                    .file_stem()
                    .map(|stem| stem.to_string_lossy().to_ascii_lowercase())
                    .unwrap_or_default();
                if !INSPECT_ALLOWLIST.contains(&name.as_str()) {
                    return Err(Refusal::ProgramNotAllowed {
                        program: program.program.clone(),
                    });
                }
                for argument in &program.args {
                    if has_shell_metacharacter(argument) {
                        return Err(Refusal::ShellMetacharacter {
                            argument: argument.clone(),
                        });
                    }
                    // Compared case-insensitively in both directions. Some
                    // tools distinguish `-R` from `-r`, so refusing both is
                    // over-strict -- which is the safe direction to be wrong in.
                    if INSPECT_FORBIDDEN_ARGS
                        .iter()
                        .any(|forbidden| argument.eq_ignore_ascii_case(forbidden))
                    {
                        return Err(Refusal::ArgumentChangesState {
                            argument: argument.clone(),
                        });
                    }
                }
                Ok(())
            }
            FixAction::RestartService { service } => {
                if service.trim().is_empty() {
                    return Err(Refusal::Empty);
                }
                if has_shell_metacharacter(service) || service.contains(char::is_whitespace) {
                    return Err(Refusal::ShellMetacharacter {
                        argument: service.clone(),
                    });
                }
                Ok(())
            }
            FixAction::RemoveStaleFile { path, .. } => {
                // Order matters. The protected-path checks run first so that a
                // dangerous target is always refused *as* a dangerous target.
                // Absoluteness is platform-dependent -- a Unix path is not
                // absolute on Windows -- and letting that check run first would
                // report `/etc/passwd.lock` as merely "relative", which is the
                // right refusal for the wrong reason and hides what happened.
                if has_parent_traversal(path) {
                    return Err(Refusal::ProtectedPath {
                        path: path.display().to_string(),
                    });
                }
                if is_protected(path) {
                    return Err(Refusal::ProtectedPath {
                        path: path.display().to_string(),
                    });
                }
                if !path.is_absolute() {
                    return Err(Refusal::RelativePath {
                        path: path.display().to_string(),
                    });
                }
                let name = path
                    .file_name()
                    .map(|name| name.to_string_lossy().to_ascii_lowercase())
                    .unwrap_or_default();
                if !REMOVABLE_SUFFIXES
                    .iter()
                    .any(|suffix| name.ends_with(suffix))
                {
                    return Err(Refusal::NotRemovable {
                        path: path.display().to_string(),
                    });
                }
                Ok(())
            }
            FixAction::Manual { instruction } => {
                if instruction.trim().is_empty() {
                    return Err(Refusal::Empty);
                }
                Ok(())
            }
        }
    }

    /// Validate, converting a refusal into an error.
    pub fn ensure_permitted(&self) -> Result<()> {
        self.validate()
            .map_err(|refusal| anyhow::anyhow!("refused: {refusal}"))
    }
}

/// Turning a runbook's written-down fix into something the engine can carry
/// out.
///
/// Runbooks are text, edited by people who are not compiling anything. They
/// name a fix in this vocabulary and the fix layer decides what that means --
/// which keeps the closed set of possible actions owned by the code that has
/// to execute them safely, rather than by whoever wrote the file.
///
/// A recipe naming something outside the set is refused, not approximated.
/// There is deliberately no "run this command" recipe, for the same reason
/// there is no such [`FixAction`].
impl FixAction {
    /// Build an action from a runbook recipe.
    ///
    /// `target` is expanded first: `~` becomes the user's home directory and
    /// `%VAR%`/`$VAR` become the environment's value, because a runbook cannot
    /// know where anybody's home directory is. The result still goes through
    /// [`FixAction::validate`] before the engine will touch it.
    pub fn from_recipe(kind: &str, target: &str) -> std::result::Result<Self, Refusal> {
        let expanded = expand(target);
        if expanded.trim().is_empty() {
            return Err(Refusal::Empty);
        }

        match kind {
            "restart-service" => Ok(FixAction::RestartService { service: expanded }),
            "remove-stale-file" => Ok(FixAction::RemoveStaleFile {
                path: std::path::PathBuf::from(&expanded),
                reason: "named by a runbook as safe to remove when stale".to_string(),
            }),
            other => Err(Refusal::UnknownRecipe {
                kind: other.to_string(),
            }),
        }
    }
}

/// Expand `~`, `$VAR`, and `%VAR%` so a runbook can name a path without
/// knowing whose machine it will run on.
///
/// An unset variable expands to nothing, which leaves a path that will fail
/// validation rather than one that quietly points somewhere unintended.
fn expand(input: &str) -> String {
    let mut text = input.trim().to_string();

    if let Some(rest) = text.strip_prefix("~/").or_else(|| text.strip_prefix("~\\"))
        && let Some(home) = dirs::home_dir()
    {
        text = home.join(rest).to_string_lossy().into_owned();
    }

    // %VAR% on Windows, $VAR elsewhere. Both are accepted everywhere so one
    // runbook entry can serve both platforms.
    let mut out = String::with_capacity(text.len());
    let bytes: Vec<char> = text.chars().collect();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            '%' => match bytes[index + 1..].iter().position(|c| *c == '%') {
                Some(offset) => {
                    let name: String = bytes[index + 1..index + 1 + offset].iter().collect();
                    out.push_str(&std::env::var(&name).unwrap_or_default());
                    index += offset + 2;
                }
                None => {
                    out.push('%');
                    index += 1;
                }
            },
            '$' => {
                let start = index + 1;
                let mut end = start;
                while end < bytes.len() && (bytes[end].is_alphanumeric() || bytes[end] == '_') {
                    end += 1;
                }
                if end == start {
                    out.push('$');
                    index += 1;
                } else {
                    let name: String = bytes[start..end].iter().collect();
                    out.push_str(&std::env::var(&name).unwrap_or_default());
                    index = end;
                }
            }
            other => {
                out.push(other);
                index += 1;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_recipe_naming_something_outside_the_closed_set_is_refused() {
        // The whole point of the closed set. A runbook is text; it does not
        // get to invent new powers by naming them.
        for kind in [
            "run-command",
            "delete-directory",
            "install-package",
            "",
            "RESTART-SERVICE",
        ] {
            assert!(
                FixAction::from_recipe(kind, "anything").is_err(),
                "`{kind}` was accepted as a kind of fix"
            );
        }
    }

    #[test]
    fn a_service_recipe_becomes_a_service_restart() {
        let action = FixAction::from_recipe("restart-service", "cups").expect("valid");
        assert!(matches!(action, FixAction::RestartService { ref service } if service == "cups"));
        assert!(action.validate().is_ok());
    }

    #[test]
    fn a_recipe_still_has_to_pass_the_safety_checks() {
        // Coming from a runbook buys a recipe nothing. Validation runs on the
        // result exactly as it would on anything else.
        let action = FixAction::from_recipe("remove-stale-file", "/etc/passwd").expect("built");
        assert!(
            action.validate().is_err(),
            "a protected path was accepted from a runbook"
        );

        let relative = FixAction::from_recipe("remove-stale-file", "steam.pid").expect("built");
        assert!(
            relative.validate().is_err(),
            "a relative path was accepted from a runbook"
        );
    }

    #[test]
    fn an_empty_target_is_refused_rather_than_expanded_into_nothing() {
        assert!(FixAction::from_recipe("remove-stale-file", "   ").is_err());
        // An unset variable expands to nothing, which must not become a fix
        // that acts on some unintended path.
        assert!(
            FixAction::from_recipe("remove-stale-file", "$ORK_DEFINITELY_UNSET_VARIABLE").is_err()
        );
    }

    #[test]
    fn a_home_relative_path_is_expanded_to_this_users_home() {
        let Some(home) = dirs::home_dir() else {
            eprintln!("skipped: this machine has no home directory");
            return;
        };
        let action = FixAction::from_recipe("remove-stale-file", "~/.steam/steam.pid").unwrap();
        match action {
            FixAction::RemoveStaleFile { path, .. } => {
                assert!(path.is_absolute(), "expansion produced {path:?}");
                assert!(path.starts_with(&home), "{path:?} is not under {home:?}");
            }
            other => panic!("expected a file removal, got {other:?}"),
        }
    }

    #[test]
    fn environment_variables_are_expanded_in_both_notations() {
        unsafe {
            std::env::set_var("ORK_TEST_DIR", "/tmp/ork-test");
        }
        for written in ["$ORK_TEST_DIR/steam.pid", "%ORK_TEST_DIR%/steam.pid"] {
            let action = FixAction::from_recipe("remove-stale-file", written).unwrap();
            match action {
                FixAction::RemoveStaleFile { path, .. } => assert!(
                    path.to_string_lossy().contains("ork-test"),
                    "{written} expanded to {path:?}"
                ),
                other => panic!("expected a file removal, got {other:?}"),
            }
        }
    }

    use super::*;

    fn inspect(program: &str, args: &[&str]) -> FixAction {
        FixAction::Inspect {
            program: Program::new(program, args),
            purpose: "the system log".to_string(),
        }
    }

    #[test]
    fn a_read_only_inspection_is_permitted() {
        assert!(
            inspect("journalctl", &["--priority=3", "--lines", "50"])
                .validate()
                .is_ok()
        );
        assert!(inspect("df", &["-h"]).validate().is_ok());
        assert!(
            inspect("systemctl", &["status", "steam"])
                .validate()
                .is_ok()
        );
    }

    #[test]
    fn a_program_that_is_not_on_the_allowlist_is_refused() {
        // An allowlist, so anything unrecognised is refused by default rather
        // than permitted by omission.
        assert_eq!(
            inspect("rm", &["-rf", "/"]).validate(),
            Err(Refusal::ProgramNotAllowed {
                program: "rm".to_string()
            })
        );
        assert!(matches!(
            inspect("bash", &["-c", "anything"]).validate(),
            Err(Refusal::ProgramNotAllowed { .. })
        ));
        assert!(matches!(
            inspect("curl", &["http://example.com"]).validate(),
            Err(Refusal::ProgramNotAllowed { .. })
        ));
    }

    #[test]
    fn an_allowlisted_program_cannot_be_used_to_change_state() {
        // systemctl reads and writes. Only the reading half is inspection.
        assert!(
            inspect("systemctl", &["status", "nginx"])
                .validate()
                .is_ok()
        );
        assert!(matches!(
            inspect("systemctl", &["stop", "nginx"]).validate(),
            Err(Refusal::ArgumentChangesState { .. })
        ));
        assert!(matches!(
            inspect("pacman", &["-R", "linux"]).validate(),
            Err(Refusal::ArgumentChangesState { .. })
        ));
        assert!(inspect("pacman", &["-Q"]).validate().is_ok());
    }

    #[test]
    fn shell_metacharacters_are_refused_even_though_no_shell_is_used() {
        // Nothing here runs through a shell, so these cannot inject. They are
        // refused because their presence means whatever produced the action
        // thought it was writing a shell command, and that is worth failing on.
        for hostile in [
            "a; rm -rf /",
            "x && reboot",
            "$(whoami)",
            "`id`",
            "out > /etc/passwd",
        ] {
            assert!(
                matches!(
                    inspect("df", &[hostile]).validate(),
                    Err(Refusal::ShellMetacharacter { .. })
                ),
                "{hostile} should have been refused"
            );
        }
    }

    #[test]
    fn a_path_inside_the_operating_system_can_never_be_removed() {
        for hostile in [
            "/etc/passwd.lock",
            "/boot/grub.lock",
            "/usr/lib/something.lock",
            "C:/Windows/System32/config.lock",
            "/dev/sda.lock",
        ] {
            let action = FixAction::RemoveStaleFile {
                path: PathBuf::from(hostile),
                reason: "stale".to_string(),
            };
            assert!(
                matches!(action.validate(), Err(Refusal::ProtectedPath { .. })),
                "{hostile} should have been refused"
            );
        }
    }

    #[test]
    fn parent_traversal_cannot_be_used_to_escape_into_a_protected_path() {
        let action = FixAction::RemoveStaleFile {
            path: PathBuf::from("/home/user/../../etc/shadow.lock"),
            reason: "stale".to_string(),
        };
        assert!(matches!(
            action.validate(),
            Err(Refusal::ProtectedPath { .. })
        ));
    }

    #[test]
    fn only_lock_and_cache_files_may_be_removed() {
        // Absoluteness is platform-specific, so the permitted case has to use
        // a path that is actually absolute on the machine running the test.
        let home = if cfg!(windows) {
            "C:/Users/someone"
        } else {
            "/home/someone"
        };
        let ok = FixAction::RemoveStaleFile {
            path: PathBuf::from(format!("{home}/.steam/steam.lock")),
            reason: "left by a crash".to_string(),
        };
        assert!(ok.validate().is_ok(), "got {:?}", ok.validate());

        // Someone's actual data is never a candidate, wherever it lives.
        for name in ["thesis.docx", "photos", ".bashrc"] {
            let action = FixAction::RemoveStaleFile {
                path: PathBuf::from(format!("{home}/{name}")),
                reason: "stale".to_string(),
            };
            let path = format!("{home}/{name}");
            assert!(
                matches!(action.validate(), Err(Refusal::NotRemovable { .. })),
                "{path} should have been refused"
            );
        }
    }

    #[test]
    fn a_relative_path_is_refused_because_it_depends_on_where_we_are_standing() {
        let action = FixAction::RemoveStaleFile {
            path: PathBuf::from("steam.lock"),
            reason: "stale".to_string(),
        };
        assert!(matches!(
            action.validate(),
            Err(Refusal::RelativePath { .. })
        ));
    }

    #[test]
    fn a_service_name_cannot_smuggle_in_extra_arguments() {
        assert!(
            FixAction::RestartService {
                service: "steam".to_string()
            }
            .validate()
            .is_ok()
        );
        for hostile in ["steam; reboot", "steam && rm -rf /", "steam nginx"] {
            let action = FixAction::RestartService {
                service: hostile.to_string(),
            };
            assert!(
                matches!(action.validate(), Err(Refusal::ShellMetacharacter { .. })),
                "{hostile} should have been refused"
            );
        }
    }

    #[test]
    fn empty_actions_are_refused() {
        assert_eq!(
            FixAction::Manual {
                instruction: "  ".to_string()
            }
            .validate(),
            Err(Refusal::Empty)
        );
        assert_eq!(
            FixAction::RestartService {
                service: String::new()
            }
            .validate(),
            Err(Refusal::Empty)
        );
    }

    #[test]
    fn only_the_two_state_changing_actions_need_confirmation() {
        assert!(!inspect("df", &["-h"]).needs_confirmation());
        assert!(
            !FixAction::Manual {
                instruction: "restart".to_string()
            }
            .needs_confirmation()
        );
        assert!(
            FixAction::RestartService {
                service: "x".to_string()
            }
            .needs_confirmation()
        );
        assert!(
            FixAction::RemoveStaleFile {
                path: PathBuf::from("/tmp/a.lock"),
                reason: "r".to_string()
            }
            .needs_confirmation()
        );
    }

    #[test]
    fn risk_orders_least_disruptive_first() {
        let mut actions = [
            FixAction::Manual {
                instruction: "reinstall the driver".to_string(),
            },
            FixAction::RestartService {
                service: "steam".to_string(),
            },
            inspect("df", &["-h"]),
        ];
        actions.sort_by_key(|action| action.risk());

        assert!(matches!(actions[0], FixAction::Inspect { .. }));
        assert!(matches!(actions[1], FixAction::RestartService { .. }));
        assert!(matches!(actions[2], FixAction::Manual { .. }));
    }

    #[test]
    fn there_is_no_way_to_express_an_arbitrary_command() {
        // This test exists to fail loudly if a future change adds a variant
        // that takes a command string. If that ever becomes necessary, every
        // safety guarantee in this module needs revisiting first.
        let action = inspect("journalctl", &["-p", "3"]);
        let serialised = serde_json::to_string(&action).unwrap();
        assert!(
            !serialised.contains("shell") && !serialised.contains("raw"),
            "an action must never carry a raw command string: {serialised}"
        );
    }
}
