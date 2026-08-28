//! What starts with the machine, and which of it is worth saying something
//! about.
//!
//! ## This is not a rootkit scan, and does not pretend to be one
//!
//! The Deep tier once promised "an exhaustive rootkit scan". It should not
//! have, and this is not it. Software that has taken control of the operating
//! system's own kernel can answer every question in this file with whatever it
//! likes: the list of things that start automatically is read *through* the
//! thing that would be hiding in it. Finding one honestly means examining the
//! disk from a system that is not the compromised one, and a program running
//! as an ordinary user on the machine in question cannot do that. A checkbox
//! saying "no rootkits found" would be a lie told confidently, which is worse
//! than the absence of the feature.
//!
//! What this does instead is real, deterministic, and useful:
//!
//! * **It lists what has arranged to run on its own** -- which is the single
//!   biggest reason a computer is slow from the moment it is turned on, and it
//!   is almost never something the person chose.
//! * **It names entries pointing at programs that are not there.** Left behind
//!   by uninstallers that did half the job. Harmless, and clutter.
//! * **It names entries running out of a temporary or downloads folder.**
//!   Installed software does not live there. Something that does is either
//!   unwanted or was installed by something that had no business installing
//!   it.
//! * **It names commands that arrive encoded** rather than written out. There
//!   is no benign reason for a start-up entry to hide what it runs.
//!
//! Every one of those is a statement about what was observed, not a verdict
//! about what it is. The wording throughout keeps that line, because "you have
//! malware" from a tool that cannot know is how people get frightened into
//! reinstalling a working machine.

use async_trait::async_trait;

use crate::Result;
use crate::finding::{Category, Finding, Severity, Triage};
use crate::platform::PlatformKind;
use crate::platform::startup::StartupEntry;
use crate::probe::{Probe, ProbeContext, ProbeMeta};
use crate::tier::ScanTier;

const PROBE_ID: &str = "startup.entries";

/// Above this many things starting by themselves, start-up is slow enough to
/// notice and the list is worth a look.
///
/// Not a fault at any number. It is mentioned rather than reported, at the
/// lowest severity there is, because "twenty programs start with your
/// computer" explains a complaint that people otherwise never get an answer
/// to.
pub const CROWDED: usize = 15;

/// How many entries to name in a finding before counting the rest.
const NAMED: usize = 12;

/// Something about an entry worth mentioning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Concern {
    /// The program it points at is not there.
    Missing,
    /// It runs from a temporary, cache, or downloads folder.
    TemporaryLocation,
    /// The command is encoded rather than written out.
    EncodedCommand,
}

/// Directory names that no installed program should be starting from.
///
/// Matched against the whole path, lowercased, so that `AppData\Local\Temp`
/// and `/tmp` and `~/Downloads` all land. Deliberately short: every addition
/// is a chance to accuse an ordinary program of something.
const TEMPORARY: &[&str] = &[
    "\\temp\\",
    "\\tmp\\",
    "/tmp/",
    "/var/tmp/",
    "\\downloads\\",
    "/downloads/",
    "\\inetcache\\",
    "\\temporary internet files\\",
];

/// Ways of saying "what follows is base64, run it".
const ENCODED: &[&str] = &[
    "-encodedcommand",
    "-enc ",
    "-ec ",
    "frombase64string",
    "-e j",
];

/// The executable out of a command line.
///
/// Harder than it looks, and worth its own function because everything else
/// here depends on getting it right:
///
/// * A quoted path is the easy case and the only unambiguous one.
/// * An unquoted Windows path may contain spaces, so splitting on whitespace
///   gives `C:\Program` for half the entries on a real machine. The first
///   `.exe` boundary is used instead.
/// * Environment variables are expanded, because `%ProgramFiles%\...` is
///   extremely common in the registry and an unexpanded one would be reported
///   as a missing file on every machine.
pub fn extract_program(command: &str) -> Option<String> {
    let command = command.trim();
    if command.is_empty() {
        return None;
    }

    let raw = if let Some(rest) = command.strip_prefix('"') {
        // Everything up to the closing quote. An unterminated quote means the
        // whole of the rest, which is the best available reading of it.
        rest.split('"').next().unwrap_or(rest).to_string()
    } else if let Some(end) = executable_suffix_end(command) {
        command[..end].to_string()
    } else {
        command
            .split_whitespace()
            .next()
            .unwrap_or(command)
            .to_string()
    };

    let expanded = expand(&raw);
    let trimmed = expanded.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Where the first `.exe` ends, when it is followed by a space or the end.
///
/// `C:\Program Files\App\app.exe --start` ends at `app.exe`, not at
/// `C:\Program`. `C:\Tools\app.exeutil\run` is not a match, because the `.exe`
/// there is part of a longer name.
fn executable_suffix_end(command: &str) -> Option<usize> {
    let lowered = command.to_lowercase();
    let mut from = 0;
    while let Some(at) = lowered[from..].find(".exe") {
        let end = from + at + 4;
        match lowered[end..].chars().next() {
            None | Some(' ') | Some('\t') => return Some(end),
            _ => from = end,
        }
    }
    None
}

/// Replace `%NAME%` and `$NAME` with what they stand for.
///
/// A name with nothing behind it is left exactly as written, rather than
/// replaced with nothing -- a path that silently lost a segment would be
/// reported as a missing file, which is a false accusation dressed as a fact.
fn expand(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;

    while let Some(start) = rest.find('%') {
        let (before, after) = rest.split_at(start);
        out.push_str(before);
        let after = &after[1..];
        match after.find('%') {
            Some(end) => {
                let name = &after[..end];
                match std::env::var(name) {
                    Ok(value) => out.push_str(&value),
                    Err(_) => {
                        out.push('%');
                        out.push_str(name);
                        out.push('%');
                    }
                }
                rest = &after[end + 1..];
            }
            None => {
                out.push('%');
                rest = after;
                break;
            }
        }
    }
    out.push_str(rest);

    // `~` is the other one that appears in real entries, and only ever at the
    // front.
    if let Some(rest) = out.strip_prefix("~/")
        && let Some(home) = dirs::home_dir()
    {
        return home.join(rest).display().to_string();
    }
    out
}

/// What is worth mentioning about one entry.
pub fn concerns(entry: &StartupEntry) -> Vec<Concern> {
    let mut found = Vec::new();

    if entry.program_exists == Some(false) {
        found.push(Concern::Missing);
    }
    if let Some(program) = &entry.program {
        let lowered = program.to_lowercase();
        if TEMPORARY.iter().any(|place| lowered.contains(place)) {
            found.push(Concern::TemporaryLocation);
        }
    }
    let command = entry.command.to_lowercase();
    if ENCODED.iter().any(|form| command.contains(form)) {
        found.push(Concern::EncodedCommand);
    }

    found
}

/// A list of entries, written for a person.
fn listing(entries: &[&StartupEntry]) -> String {
    let mut lines: Vec<String> = entries
        .iter()
        .take(NAMED)
        .map(|entry| format!("{} ({})", entry.name, entry.source))
        .collect();
    if entries.len() > NAMED {
        lines.push(format!("and {} more", entries.len() - NAMED));
    }
    lines.join("; ")
}

/// Everything worth saying about what starts on this machine.
///
/// Pure, and taking the list rather than reading it, so that the judgement can
/// be tested against machines nobody here has: one with an encoded command,
/// one with thirty startup entries, one with a preload file.
pub fn assess(entries: &[StartupEntry], preload: Option<&str>) -> Vec<Finding> {
    let mut findings = Vec::new();

    if let Some(contents) = preload {
        findings.push(preload_finding(contents));
    }

    let missing: Vec<&StartupEntry> = entries
        .iter()
        .filter(|entry| concerns(entry).contains(&Concern::Missing))
        .collect();
    let temporary: Vec<&StartupEntry> = entries
        .iter()
        .filter(|entry| concerns(entry).contains(&Concern::TemporaryLocation))
        .collect();
    let encoded: Vec<&StartupEntry> = entries
        .iter()
        .filter(|entry| concerns(entry).contains(&Concern::EncodedCommand))
        .collect();

    if !encoded.is_empty() {
        findings.push(
            Finding::builder(PROBE_ID, "startup.encoded-command")
                .severity(Severity::High)
                .category(Category::Malware)
                .title(format!(
                    "{} start-up {} hide{} what {} run",
                    encoded.len(),
                    if encoded.len() == 1 {
                        "entry"
                    } else {
                        "entries"
                    },
                    if encoded.len() == 1 { "s" } else { "" },
                    if encoded.len() == 1 { "it" } else { "they" }
                ))
                .detail(format!(
                    "These start automatically and pass their instructions along encoded \
                     rather than written out, so what they actually do is not visible in the \
                     entry: {}. Ordinary software has no reason to do this -- an installer, \
                     an updater, and a printer utility all say plainly what they run. It is \
                     worth finding out what these are before doing anything else on this \
                     list.",
                    listing(&encoded)
                ))
                .evidence("count", encoded.len().to_string())
                .evidence(
                    "commands",
                    encoded
                        .iter()
                        .take(NAMED)
                        .map(|entry| entry.command.clone())
                        .collect::<Vec<_>>()
                        .join(" | "),
                )
                .remediation_hint(
                    "Do not delete these yet. Search for the name first: knowing what it is \
                     tells you whether the rest of the machine needs looking at.",
                )
                .triage(Triage::Queue)
                .build(),
        );
    }

    if !temporary.is_empty() {
        findings.push(
            Finding::builder(PROBE_ID, "startup.temporary-location")
                .severity(Severity::Medium)
                .category(Category::Malware)
                .title(format!(
                    "{} start-up {} run{} from a temporary folder",
                    temporary.len(),
                    if temporary.len() == 1 {
                        "entry"
                    } else {
                        "entries"
                    },
                    if temporary.len() == 1 { "s" } else { "" }
                ))
                .detail(format!(
                    "Installed software lives in a program folder. These start from a \
                     temporary, cache, or downloads folder instead: {}. That is where \
                     unwanted software puts itself, because those folders can be written to \
                     without any special rights -- and it is also where a badly made \
                     installer leaves things, so this is a reason to look rather than a \
                     verdict.",
                    listing(&temporary)
                ))
                .evidence("count", temporary.len().to_string())
                .evidence(
                    "programs",
                    temporary
                        .iter()
                        .filter_map(|entry| entry.program.clone())
                        .take(NAMED)
                        .collect::<Vec<_>>()
                        .join(" | "),
                )
                .remediation_hint(
                    "Find out what each one is before removing it. Anything genuinely \
                     temporary can be removed from the start-up list without uninstalling \
                     anything.",
                )
                .triage(Triage::Queue)
                .build(),
        );
    }

    if !missing.is_empty() {
        findings.push(
            Finding::builder(PROBE_ID, "startup.points-at-nothing")
                .severity(Severity::Low)
                .category(Category::Configuration)
                .title(format!(
                    "{} start-up {} point{} at {} that {} not there",
                    missing.len(),
                    if missing.len() == 1 {
                        "entry"
                    } else {
                        "entries"
                    },
                    if missing.len() == 1 { "s" } else { "" },
                    if missing.len() == 1 {
                        "a program"
                    } else {
                        "programs"
                    },
                    if missing.len() == 1 { "is" } else { "are" }
                ))
                .detail(format!(
                    "These are asked for every time the machine starts, and the program each \
                     one names is not on the disk: {}. Almost always what is left when \
                     something was uninstalled and its uninstaller did not finish the job. \
                     Nothing is broken by them -- the operating system tries, fails, and \
                     carries on -- but they are clutter, and they make the list of what \
                     actually starts harder to read.",
                    listing(&missing)
                ))
                .evidence("count", missing.len().to_string())
                .evidence(
                    "programs",
                    missing
                        .iter()
                        .filter_map(|entry| entry.program.clone())
                        .take(NAMED)
                        .collect::<Vec<_>>()
                        .join(" | "),
                )
                .remediation_hint(
                    "Safe to remove from the start-up list: there is nothing behind them to \
                     break.",
                )
                .triage(Triage::Queue)
                .build(),
        );
    }

    // Said last, and only when there are enough of them to be felt. Counted
    // over everything, including the entries above, because the person
    // wondering why their machine takes two minutes to become usable does not
    // care which category each one fell into.
    if entries.len() >= CROWDED {
        findings.push(
            Finding::builder(PROBE_ID, "startup.crowded")
                .severity(Severity::Info)
                .category(Category::Performance)
                .title(format!("{} things start with this computer", entries.len()))
                .detail(format!(
                    "Not a fault, and not necessarily a problem -- but it is the usual answer \
                     to why a machine is slow for the first minute or two after it starts, \
                     and most of these were installed alongside something else rather than \
                     chosen. The list: {}.",
                    listing(&entries.iter().collect::<Vec<_>>())
                ))
                .evidence("count", entries.len().to_string())
                .triage(Triage::None)
                .build(),
        );
    }

    findings
}

fn preload_finding(contents: &str) -> Finding {
    let libraries: Vec<&str> = contents
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect();

    Finding::builder(PROBE_ID, "startup.library-preload")
        .subject("/etc/ld.so.preload")
        .severity(Severity::High)
        .category(Category::Malware)
        .title("A library is being loaded into every program on this machine")
        .detail(format!(
            "`/etc/ld.so.preload` exists and names {}. Anything listed there is loaded into \
             every program that starts, before that program's own code runs, and can change \
             what any of them does or sees. On an ordinary desktop this file does not exist \
             at all. It has legitimate uses -- some commercial software and some debugging \
             tools use it -- so this is not by itself proof of anything; it is the single \
             most worthwhile thing on this machine to be able to explain.",
            if libraries.is_empty() {
                "nothing that could be read".to_string()
            } else {
                libraries.join(", ")
            }
        ))
        .evidence("libraries", libraries.join(", "))
        .remediation_hint(
            "Find out what put it there before changing it. Removing the file while something \
             depends on it can stop programs starting.",
        )
        .triage(Triage::Queue)
        .build()
}

/// The probe itself, which is only the plumbing.
pub struct StartupProbe;

#[async_trait]
impl Probe for StartupProbe {
    fn meta(&self) -> ProbeMeta {
        ProbeMeta {
            id: PROBE_ID,
            name: "Start-up entries",
            description: "Everything that has arranged to start by itself, and anything about \
                          those that is worth a look.",
            category: Category::Configuration,
            min_tier: ScanTier::Full,
            platforms: &[PlatformKind::Windows, PlatformKind::Linux],
            requires_tools: &[],
            // Deliberately not. Everything read here is readable by the person
            // whose machine it is, and a check that only runs for
            // administrators is a check most people never see.
            requires_elevation: false,
        }
    }

    async fn run(&self, ctx: &ProbeContext) -> Result<Vec<Finding>> {
        if ctx.is_cancelled() {
            return Ok(Vec::new());
        }
        let (entries, preload) = tokio::task::spawn_blocking(|| {
            (
                crate::platform::startup::entries(),
                crate::platform::startup::library_preload(),
            )
        })
        .await?;
        Ok(assess(&entries?, preload.as_deref()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, command: &str, exists: Option<bool>) -> StartupEntry {
        StartupEntry {
            name: name.to_string(),
            source: "a place".to_string(),
            command: command.to_string(),
            program: extract_program(command),
            program_exists: exists,
            for_all_users: false,
        }
    }

    #[test]
    fn a_quoted_path_with_spaces_survives_intact() {
        // The common case on Windows, and the one that splitting on
        // whitespace turns into `C:\Program`.
        assert_eq!(
            extract_program(r#""C:\Program Files\App\app.exe" --background"#),
            Some(r"C:\Program Files\App\app.exe".to_string())
        );
    }

    #[test]
    fn an_unquoted_path_with_spaces_survives_too() {
        // Registry entries written without quotes are extremely common, and
        // this is the case a naive parser gets wrong on every machine.
        assert_eq!(
            extract_program(r"C:\Program Files\App\app.exe --background"),
            Some(r"C:\Program Files\App\app.exe".to_string())
        );
    }

    #[test]
    fn an_unquoted_path_with_no_arguments_survives() {
        assert_eq!(
            extract_program(r"C:\Program Files\App\app.exe"),
            Some(r"C:\Program Files\App\app.exe".to_string())
        );
    }

    #[test]
    fn a_name_that_merely_contains_exe_is_not_split_there() {
        assert_eq!(
            extract_program(r"C:\Tools\app.exefile\run.exe --now"),
            Some(r"C:\Tools\app.exefile\run.exe".to_string())
        );
    }

    #[test]
    fn a_plain_command_with_no_extension_is_the_first_word() {
        assert_eq!(
            extract_program("/usr/bin/backup --daemon"),
            Some("/usr/bin/backup".to_string())
        );
        assert_eq!(extract_program("thing"), Some("thing".to_string()));
    }

    #[test]
    fn an_unterminated_quote_gives_up_the_rest_rather_than_nothing() {
        assert_eq!(
            extract_program(r#""C:\Program Files\App\app.exe --background"#),
            Some(r"C:\Program Files\App\app.exe --background".to_string())
        );
    }

    #[test]
    fn nothing_at_all_is_nothing_rather_than_an_empty_name() {
        assert_eq!(extract_program(""), None);
        assert_eq!(extract_program("   "), None);
    }

    #[test]
    fn an_environment_variable_is_expanded() {
        // Otherwise every entry written this way -- and the registry is full
        // of them -- is reported as pointing at a program that is not there.
        // SAFETY: single-threaded test, and the variable is read back at once.
        unsafe { std::env::set_var("ORK_TEST_PLACE", "C:/somewhere") };
        assert_eq!(
            extract_program(r"%ORK_TEST_PLACE%\app.exe --go"),
            Some(r"C:/somewhere\app.exe".to_string())
        );
        unsafe { std::env::remove_var("ORK_TEST_PLACE") };
    }

    #[test]
    fn an_unknown_variable_is_left_alone_rather_than_deleted() {
        // Replacing it with nothing would turn the path into one that is
        // genuinely not there, and the tool would report a missing program
        // that is sitting exactly where it should be.
        let got = extract_program(r"%ORK_NO_SUCH_VARIABLE%\app.exe").unwrap();
        assert!(got.starts_with("%ORK_NO_SUCH_VARIABLE%"), "{got}");
    }

    #[test]
    fn a_stray_percent_sign_does_not_eat_the_path() {
        let got = extract_program(r"C:\100%\app.exe").unwrap();
        assert!(got.contains("app.exe"), "{got}");
    }

    #[test]
    fn a_program_that_is_not_there_is_noticed() {
        let gone = entry("Leftover", r"C:\Gone\gone.exe", Some(false));
        assert_eq!(concerns(&gone), vec![Concern::Missing]);
    }

    #[test]
    fn a_program_that_is_there_is_not_accused_of_anything() {
        let fine = entry("Printer", r"C:\Program Files\Printer\p.exe", Some(true));
        assert!(concerns(&fine).is_empty());
    }

    #[test]
    fn a_program_we_could_not_look_for_is_not_reported_as_missing() {
        // Not knowing and being absent are different, and only one of them is
        // worth telling somebody about.
        let unknown = entry("Odd", "something-unresolvable", None);
        assert!(!concerns(&unknown).contains(&Concern::Missing));
    }

    #[test]
    fn running_from_a_temporary_folder_is_noticed_on_either_platform() {
        for command in [
            r"C:\Users\a\AppData\Local\Temp\thing.exe",
            r"C:\Users\a\Downloads\setup.exe",
            "/tmp/thing",
            "/home/a/Downloads/thing",
        ] {
            let suspicious = entry("Thing", command, Some(true));
            assert!(
                concerns(&suspicious).contains(&Concern::TemporaryLocation),
                "missed {command}"
            );
        }
    }

    #[test]
    fn an_ordinary_program_folder_is_not_a_temporary_one() {
        // The false positive that would matter: `Temp` appearing inside a
        // longer word, or a company called something unfortunate.
        for command in [
            r"C:\Program Files\Template Studio\app.exe",
            r"C:\Program Files\Contemporary\app.exe",
            "/usr/bin/tmpwatch",
        ] {
            let fine = entry("Thing", command, Some(true));
            assert!(
                !concerns(&fine).contains(&Concern::TemporaryLocation),
                "wrongly accused {command}"
            );
        }
    }

    #[test]
    fn an_encoded_command_is_noticed() {
        for command in [
            "powershell -EncodedCommand SQBFAFgA",
            "powershell.exe -nop -w hidden -enc SQBFAFgA",
            "powershell -c \"iex ([Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('...')))\"",
        ] {
            let hidden = entry("Thing", command, Some(true));
            assert!(
                concerns(&hidden).contains(&Concern::EncodedCommand),
                "missed {command}"
            );
        }
    }

    #[test]
    fn an_ordinary_command_is_not_called_encoded() {
        for command in [
            r"C:\Program Files\App\app.exe --encoding utf8",
            r"C:\Program Files\Encoder\encoder.exe",
            "/usr/bin/backup --daemon",
        ] {
            let fine = entry("Thing", command, Some(true));
            assert!(
                !concerns(&fine).contains(&Concern::EncodedCommand),
                "wrongly accused {command}"
            );
        }
    }

    #[test]
    fn a_clean_machine_produces_nothing() {
        let clean: Vec<StartupEntry> = (0..4)
            .map(|number| {
                entry(
                    &format!("App {number}"),
                    &format!(r"C:\Program Files\App{number}\app.exe"),
                    Some(true),
                )
            })
            .collect();
        assert!(assess(&clean, None).is_empty());
    }

    #[test]
    fn each_kind_of_concern_produces_its_own_finding() {
        let mixed = vec![
            entry("Gone", r"C:\Gone\gone.exe", Some(false)),
            entry("Temp", r"C:\Users\a\AppData\Local\Temp\t.exe", Some(true)),
            entry("Hidden", "powershell -enc SQBFAFgA", Some(true)),
        ];
        let ids: Vec<String> = assess(&mixed, None)
            .into_iter()
            .map(|finding| finding.id)
            .collect();
        assert!(ids.contains(&"startup.points-at-nothing".to_string()));
        assert!(ids.contains(&"startup.temporary-location".to_string()));
        assert!(ids.contains(&"startup.encoded-command".to_string()));
    }

    #[test]
    fn the_worst_thing_is_reported_first() {
        // The scan sorts by severity, but the order these are built in is
        // what an unsorted consumer sees, and the encoded one is the only
        // entry on this list that could be urgent.
        let mixed = vec![
            entry("Gone", r"C:\Gone\gone.exe", Some(false)),
            entry("Hidden", "powershell -enc SQBFAFgA", Some(true)),
        ];
        let findings = assess(&mixed, None);
        assert_eq!(findings[0].id, "startup.encoded-command");
        assert_eq!(findings[0].severity, Severity::High);
    }

    #[test]
    fn a_crowded_machine_is_mentioned_and_not_reported_as_broken() {
        let crowded: Vec<StartupEntry> = (0..CROWDED)
            .map(|number| {
                entry(
                    &format!("App {number}"),
                    &format!(r"C:\Program Files\App{number}\app.exe"),
                    Some(true),
                )
            })
            .collect();
        let findings = assess(&crowded, None);
        let crowd = findings
            .iter()
            .find(|finding| finding.id == "startup.crowded")
            .expect("should mention it");
        assert_eq!(crowd.severity, Severity::Info);
        assert_eq!(crowd.triage, Triage::None);
        assert!(crowd.detail.contains("Not a fault"), "{}", crowd.detail);
    }

    #[test]
    fn an_ordinary_number_of_start_up_entries_is_not_mentioned_at_all() {
        let ordinary: Vec<StartupEntry> = (0..CROWDED - 1)
            .map(|number| {
                entry(
                    &format!("App {number}"),
                    &format!(r"C:\Program Files\App{number}\app.exe"),
                    Some(true),
                )
            })
            .collect();
        assert!(assess(&ordinary, None).is_empty());
    }

    #[test]
    fn a_long_list_is_cut_off_rather_than_printed_whole() {
        // A finding with two hundred names in it is a finding nobody reads,
        // and on a machine with a genuinely broken uninstaller there can be
        // that many.
        let many: Vec<StartupEntry> = (0..200)
            .map(|number| entry(&format!("Gone {number}"), r"C:\Gone\gone.exe", Some(false)))
            .collect();
        let findings = assess(&many, None);
        let missing = findings
            .iter()
            .find(|finding| finding.id == "startup.points-at-nothing")
            .unwrap();
        assert!(
            missing.detail.contains("and 188 more"),
            "{}",
            missing.detail
        );
        assert!(missing.detail.len() < 2000, "{}", missing.detail.len());
    }

    #[test]
    fn a_preload_file_is_reported_with_what_is_in_it() {
        let findings = assess(&[], Some("/usr/lib/libsomething.so\n"));
        let preload = findings
            .iter()
            .find(|finding| finding.id == "startup.library-preload")
            .expect("a preload file is worth saying out loud");
        assert_eq!(preload.severity, Severity::High);
        assert!(preload.detail.contains("libsomething.so"));
        // And it does not claim to know what it is.
        assert!(
            preload.detail.contains("legitimate uses"),
            "{}",
            preload.detail
        );
    }

    #[test]
    fn a_machine_without_a_preload_file_hears_nothing_about_one() {
        assert!(assess(&[], None).is_empty());
    }

    #[test]
    fn comments_in_a_preload_file_are_not_reported_as_libraries() {
        let findings = assess(&[], Some("# put back by the installer\n/usr/lib/a.so\n"));
        let preload = &findings[0];
        assert!(!preload.detail.contains("put back"), "{}", preload.detail);
        assert!(preload.detail.contains("/usr/lib/a.so"));
    }

    #[test]
    fn nothing_here_claims_to_have_found_malware() {
        // The line this whole probe is built to stay on. A tool that cannot
        // know must not tell somebody their machine is infected -- that is
        // how a working computer gets reinstalled over a leftover registry
        // entry.
        let alarming = vec![
            entry("Hidden", "powershell -enc SQBFAFgA", Some(true)),
            entry("Temp", r"C:\Users\a\AppData\Local\Temp\t.exe", Some(true)),
        ];
        let mut findings = assess(&alarming, Some("/usr/lib/evil.so\n"));
        findings.extend(assess(&alarming, None));
        for finding in &findings {
            let said = format!("{} {}", finding.title, finding.detail).to_lowercase();
            for word in ["malware", "virus", "infected", "trojan", "rootkit"] {
                assert!(
                    !said.contains(word),
                    "{} says \"{word}\": {said}",
                    finding.id
                );
            }
        }
    }
}
