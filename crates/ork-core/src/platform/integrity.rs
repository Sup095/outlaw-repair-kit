//! Do the operating system's own files still match what installed them?
//!
//! Every other check in this tool asks whether something is *behaving* badly.
//! This one asks whether the machine is still made of what it was made of.
//! It is the check that catches the class of fault nothing else can see: a
//! half-finished update, a disk that corrupted a file it was holding, a
//! program that overwrote a shared library with its own copy, or something
//! that replaced a system binary on purpose.
//!
//! It is slow -- minutes, sometimes many of them -- because it reads and
//! hashes a large part of the operating system. That is why it lives in the
//! Deep tier and nowhere else, and why it is supervised for liveness rather
//! than given a deadline: a machine reading a struggling disk may take an hour
//! to do this honestly, and cutting it off at some round number would turn the
//! one check that could have found the corruption into a check that reported
//! nothing.
//!
//! ## What it does not do
//!
//! It never repairs. `sfc /verifyonly` is used in preference to `sfc
//! /scannow` precisely because the second one starts replacing files. Deciding
//! that a system file is wrong and putting a different one in its place is a
//! system-level change, and every system-level change in this tool goes
//! through the queue with a person's confirmation on it.
//!
//! ## Why "altered" is not "damaged"
//!
//! On Linux the answer comes from the package manager comparing what is on
//! disk against what its packages recorded. A file you edited yourself is
//! altered by that definition, and `/etc` is full of files people are supposed
//! to edit. Where the tool marks configuration files as such -- `rpm` does --
//! they are counted separately and reported as configuration rather than
//! damage. Where it does not, the finding says plainly that edits of your own
//! will appear in the list, because a report that calls your own `/etc/fstab`
//! corruption is worse than no report.

use crate::Result;
use crate::exec::{ExecOutcome, LivenessPolicy, run_supervised};
use tokio_util::sync::CancellationToken;

/// What the check concluded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntegrityVerdict {
    /// Everything checked matched what installed it.
    Intact,
    /// Files no longer match. `files` may be empty even so: Windows reports a
    /// count and writes the names to its own log rather than to the console.
    Damaged { files: Vec<String> },
    /// The check could not be carried out, or was interrupted. Never treated
    /// as a pass -- see [`IntegrityReport`].
    CouldNotCheck { reason: String },
}

impl IntegrityVerdict {
    pub fn is_intact(&self) -> bool {
        matches!(self, IntegrityVerdict::Intact)
    }
}

/// The outcome of one integrity check, and where it came from.
///
/// `checked_with` is carried all the way to the finding on purpose. "Your
/// system files are damaged" is a serious thing to tell somebody, and the
/// first question a person who knows their machine will ask is which tool
/// said so, so that they can go and ask it themselves.
#[derive(Debug, Clone)]
pub struct IntegrityReport {
    pub verdict: IntegrityVerdict,
    /// The command that produced the verdict, as a person would type it.
    pub checked_with: String,
    /// Where the full detail lives, when the tool keeps its own log.
    pub log_hint: Option<String>,
    /// Files that differ but are meant to be edited by their owner, kept
    /// apart from damage. Only populated where the underlying tool
    /// distinguishes them.
    pub altered_config: Vec<String>,
}

/// Windows writes console output as UTF-16, which arrives here as text with a
/// zero byte between every character.
///
/// Decoding the bytes as UTF-8 is lossy but harmless for the ASCII messages we
/// match on; the zero bytes survive as `\0` and are what actually break the
/// match. Removing them is the whole repair. Guarded so that a normal UTF-8
/// tool -- which will never contain an interior NUL -- passes through
/// untouched.
pub fn undo_utf16(text: &str) -> String {
    if !text.contains('\0') {
        return text.to_string();
    }
    text.chars()
        .filter(|character| *character != '\0')
        .collect()
}

/// Read `sfc`'s verdict out of what it printed.
///
/// Matched on the distinctive middle of each sentence rather than the whole
/// of it, because the wording carries a trailing sentence about logs that has
/// changed between Windows versions, and because `sfc` overwrites its own
/// progress line with carriage returns, leaving the interesting text buried in
/// the middle of a line.
pub fn interpret_sfc(raw: &str) -> IntegrityVerdict {
    let text = undo_utf16(raw).to_lowercase();

    // Order matters. "did not find any integrity violations" contains "found
    // integrity violations" as a substring in some localisations of the
    // wording, so the clean answer is tested first and wins.
    if text.contains("did not find any integrity violations") {
        return IntegrityVerdict::Intact;
    }
    if text.contains("found integrity violations") {
        return IntegrityVerdict::Damaged { files: Vec::new() };
    }
    if text.contains("could not perform the requested operation") {
        return IntegrityVerdict::CouldNotCheck {
            reason: "Windows Resource Protection could not perform the check. This usually \
                     means another servicing operation -- an update installing, or a repair \
                     already running -- has the component store open."
                .to_string(),
        };
    }
    if text.contains("must be an administrator") || text.contains("elevated") {
        return IntegrityVerdict::CouldNotCheck {
            reason: "sfc needs administrator rights and did not have them".to_string(),
        };
    }
    if text.contains("there is a system repair pending") {
        return IntegrityVerdict::CouldNotCheck {
            reason: "a system repair is already pending; Windows wants a restart before it \
                     will verify anything"
                .to_string(),
        };
    }
    IntegrityVerdict::CouldNotCheck {
        reason: "sfc finished without saying anything this tool recognises".to_string(),
    }
}

/// Read `pacman -Qkk` output.
///
/// Every line naming a file is a difference; the trailing summary lines
/// ("N total files, M altered files") are counted rather than listed.
/// `pacman` does not mark configuration files in this output, so nothing is
/// separated out here -- the finding says so instead.
pub fn parse_pacman(stdout: &str, stderr: &str) -> IntegrityVerdict {
    let mut files = Vec::new();
    for line in stdout.lines().chain(stderr.lines()) {
        let line = line.trim();
        // `package: /path/to/file (Size mismatch)`. Anything without a path is
        // a summary, a warning, or a blank.
        if line.is_empty() || !line.contains('/') {
            continue;
        }
        // A count line -- `foo: 421 total files, 2 altered files` -- has no
        // path in it, so it has already been skipped. What is left and
        // mentions a slash is a real file.
        if let Some(rest) = line.split_once(": ").map(|(_, rest)| rest)
            && rest.starts_with('/')
        {
            files.push(rest.to_string());
        }
    }

    if files.is_empty() {
        IntegrityVerdict::Intact
    } else {
        IntegrityVerdict::Damaged { files }
    }
}

/// Read `rpm -Va` output.
///
/// Each line is nine test-result characters, an optional attribute marker, and
/// a path. The marker `c` means the package declared this file to be
/// configuration -- something the owner of the machine is expected to edit --
/// so those are reported separately from damage rather than counted as it.
pub fn parse_rpm(stdout: &str) -> (Vec<String>, Vec<String>) {
    let mut damaged = Vec::new();
    let mut config = Vec::new();

    for line in stdout.lines() {
        let line = line.trim_end();
        let Some(path_at) = line.find('/') else {
            continue;
        };
        let (prefix, path) = line.split_at(path_at);
        // `missing` lines read `missing     /usr/bin/thing` and carry no test
        // characters; treat them as damage, which is what they are.
        let marked_config = prefix.split_whitespace().any(|token| token == "c");
        // Only the digest, size and mode tests say the content is wrong. A
        // lone timestamp difference is not damage and reporting it as such
        // would fill the list with noise.
        let interesting = prefix.contains('5')
            || prefix.contains('S')
            || prefix.contains('M')
            || prefix.contains("missing");
        if !interesting {
            continue;
        }
        if marked_config {
            config.push(path.to_string());
        } else {
            damaged.push(path.to_string());
        }
    }

    (damaged, config)
}

/// Read `debsums -s` output. It is silent when everything matches and reports
/// only failures, on stderr.
pub fn parse_debsums(stdout: &str, stderr: &str) -> IntegrityVerdict {
    let mut files = Vec::new();
    for line in stdout.lines().chain(stderr.lines()) {
        let line = line.trim();
        // `debsums: checksum mismatch <file>` and
        // `debsums: missing file <file> (from <pkg>)`.
        let Some(rest) = line.strip_prefix("debsums: ") else {
            continue;
        };
        let Some(path_at) = rest.find('/') else {
            continue;
        };
        let path = rest[path_at..]
            .split(" (from ")
            .next()
            .unwrap_or_default()
            .trim();
        if !path.is_empty() {
            files.push(path.to_string());
        }
    }

    if files.is_empty() {
        IntegrityVerdict::Intact
    } else {
        IntegrityVerdict::Damaged { files }
    }
}

/// Turn a supervised run into a verdict, given a parser for its output.
///
/// A stalled or cancelled run is never a pass. Half of a hash check tells you
/// nothing about the half it did not reach, and reporting "intact" because
/// the check was interrupted is the one failure mode that would make this
/// whole check worse than not having it.
fn from_outcome(
    outcome: ExecOutcome,
    parse: impl FnOnce(&str, &str) -> IntegrityVerdict,
) -> IntegrityVerdict {
    match &outcome {
        ExecOutcome::Stalled { idle, .. } => IntegrityVerdict::CouldNotCheck {
            reason: format!(
                "the check stopped responding after {} seconds of no activity at all, and was \
                 stopped. A machine that goes silent part-way through reading its own system \
                 files is often one with a disk that cannot read them -- though a machine with \
                 every processor already saturated looks the same from here, so this is worth \
                 repeating on a quiet machine before reading anything into it.",
                idle.as_secs()
            ),
        },
        ExecOutcome::Cancelled { .. } => IntegrityVerdict::CouldNotCheck {
            reason: "the check was cancelled before it finished".to_string(),
        },
        ExecOutcome::Exited { stdout, stderr, .. } => parse(stdout, stderr),
    }
}

/// Verify this machine's system files.
///
/// Blocks, sometimes for a very long time. Call it from a blocking context and
/// give it a cancellation token that a person can actually reach.
pub fn check(cancel: &CancellationToken) -> Result<IntegrityReport> {
    #[cfg(windows)]
    {
        check_windows(cancel)
    }
    #[cfg(target_os = "linux")]
    {
        check_linux(cancel)
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = cancel;
        Ok(IntegrityReport {
            verdict: IntegrityVerdict::CouldNotCheck {
                reason: "no system file verification is implemented for this operating system"
                    .to_string(),
            },
            checked_with: String::new(),
            log_hint: None,
            altered_config: Vec::new(),
        })
    }
}

#[cfg(windows)]
fn check_windows(cancel: &CancellationToken) -> Result<IntegrityReport> {
    // `/verifyonly` and never `/scannow`: this check reports, it does not
    // replace files. See the module docs.
    let outcome = run_supervised("sfc", &["/verifyonly"], LivenessPolicy::default(), cancel)?;
    let verdict = from_outcome(outcome, |stdout, stderr| {
        let combined = format!("{stdout}\n{stderr}");
        interpret_sfc(&combined)
    });

    Ok(IntegrityReport {
        verdict,
        checked_with: "sfc /verifyonly".to_string(),
        log_hint: Some(r"%WinDir%\Logs\CBS\CBS.log".to_string()),
        altered_config: Vec::new(),
    })
}

#[cfg(target_os = "linux")]
fn check_linux(cancel: &CancellationToken) -> Result<IntegrityReport> {
    use crate::platform::common::which;

    // Whichever package manager this distribution actually uses. Declared as
    // a required tool would mean naming one of them, which would skip the
    // check on every distribution that uses a different one.
    if which("pacman").is_some() {
        let outcome = run_supervised("pacman", &["-Qkk"], LivenessPolicy::default(), cancel)?;
        return Ok(IntegrityReport {
            verdict: from_outcome(outcome, parse_pacman),
            checked_with: "pacman -Qkk".to_string(),
            log_hint: None,
            altered_config: Vec::new(),
        });
    }

    if which("rpm").is_some() {
        let outcome = run_supervised("rpm", &["-Va"], LivenessPolicy::default(), cancel)?;
        let mut config = Vec::new();
        let verdict = from_outcome(outcome, |stdout, _stderr| {
            let (damaged, marked) = parse_rpm(stdout);
            config = marked;
            if damaged.is_empty() {
                IntegrityVerdict::Intact
            } else {
                IntegrityVerdict::Damaged { files: damaged }
            }
        });
        return Ok(IntegrityReport {
            verdict,
            checked_with: "rpm -Va".to_string(),
            log_hint: None,
            altered_config: config,
        });
    }

    if which("debsums").is_some() {
        let outcome = run_supervised("debsums", &["-s"], LivenessPolicy::default(), cancel)?;
        return Ok(IntegrityReport {
            verdict: from_outcome(outcome, parse_debsums),
            checked_with: "debsums -s".to_string(),
            log_hint: None,
            altered_config: Vec::new(),
        });
    }

    Ok(IntegrityReport {
        verdict: IntegrityVerdict::CouldNotCheck {
            reason: "this machine has none of `pacman`, `rpm` or `debsums`, so there is nothing \
                     here that knows what the system files were supposed to look like. On a \
                     Debian or Ubuntu system, installing `debsums` gives this check something \
                     to ask."
                .to_string(),
        },
        checked_with: String::new(),
        log_hint: None,
        altered_config: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Both interpreters are compiled on every platform so that their tests
    // run everywhere. Without this, the parser for the other operating system
    // is dead code and `-D warnings` fails the build.
    #[test]
    fn utf16_console_output_is_readable_again() {
        let windows = "W\0i\0n\0d\0o\0w\0s\0";
        assert_eq!(undo_utf16(windows), "Windows");
    }

    #[test]
    fn ordinary_output_passes_through_untouched() {
        let text = "pacman: /usr/bin/thing (Size mismatch)";
        assert_eq!(undo_utf16(text), text);
    }

    #[test]
    fn sfc_finding_nothing_is_intact() {
        let output = "Beginning system scan.  This process will take some time.\r\n\r\n\
                      Windows Resource Protection did not find any integrity violations.\r\n";
        assert_eq!(interpret_sfc(output), IntegrityVerdict::Intact);
    }

    #[test]
    fn sfc_is_read_correctly_through_utf16() {
        // The real thing arrives like this, and a parser that only works on
        // hand-typed samples would pass its tests and fail on every machine.
        let plain = "Windows Resource Protection did not find any integrity violations.";
        let as_utf16: String = plain.chars().flat_map(|c| [c, '\0']).collect();
        assert_eq!(interpret_sfc(&as_utf16), IntegrityVerdict::Intact);
    }

    #[test]
    fn the_real_thing_refusing_for_want_of_rights_is_read_correctly() {
        // Captured verbatim from `sfc /verifyonly` run without elevation on
        // Windows 10, bytes and all: UTF-16LE, so every character is followed
        // by a zero, and the message is wrapped mid-sentence with `\r\r\n`.
        // This is the only real sample this parser has, so it is kept exactly
        // as it arrived rather than tidied into something easier to read.
        let real = "\r\0\r\0\n\0Y\0o\0u\0 \0m\0u\0s\0t\0 \0b\0e\0 \0a\0n\0 \0a\0d\0m\0i\0n\0\
                    i\0s\0t\0r\0a\0t\0o\0r\0 \0r\0u\0n\0n\0i\0n\0g\0 \0a\0 \0c\0o\0n\0s\0o\0l\0\
                    e\0 \0s\0e\0s\0s\0i\0o\0n\0 \0i\0n\0 \0o\0r\0d\0e\0r\0 \0t\0o\0 \0\r\0\r\0\
                    \n\0u\0s\0e\0 \0t\0h\0e\0 \0s\0f\0c\0 \0u\0t\0i\0l\0i\0t\0y\0.\0\r\0\r\0\n\0";

        let verdict = interpret_sfc(real);
        assert!(!verdict.is_intact(), "{verdict:?}");
        let IntegrityVerdict::CouldNotCheck { reason } = verdict else {
            panic!("being refused for want of rights is not damage");
        };
        assert!(
            reason.contains("administrator"),
            "the reason must say what was missing: {reason}"
        );
    }

    #[test]
    fn sfc_finding_violations_is_damage_without_a_file_list() {
        // sfc writes the names to CBS.log, not to the console. Reporting an
        // empty list is honest; inventing names would not be.
        let output = "Windows Resource Protection found integrity violations.  Details are \
                      included in the CBS.Log windir\\Logs\\CBS\\CBS.log.";
        assert_eq!(
            interpret_sfc(output),
            IntegrityVerdict::Damaged { files: Vec::new() }
        );
    }

    #[test]
    fn sfc_refusing_to_run_is_not_a_pass() {
        let output = "Windows Resource Protection could not perform the requested operation.";
        let verdict = interpret_sfc(output);
        assert!(!verdict.is_intact(), "{verdict:?}");
        assert!(matches!(verdict, IntegrityVerdict::CouldNotCheck { .. }));
    }

    #[test]
    fn sfc_saying_something_unrecognised_is_not_a_pass() {
        // The failure that would matter most: a wording this tool has never
        // seen must never come out as a clean bill of health.
        assert!(!interpret_sfc("something else entirely").is_intact());
        assert!(!interpret_sfc("").is_intact());
    }

    #[test]
    fn pacman_reporting_nothing_is_intact() {
        let stdout = "ffmpeg: 1204 total files, 0 altered files\n\
                      linux: 88 total files, 0 altered files\n";
        assert_eq!(parse_pacman(stdout, ""), IntegrityVerdict::Intact);
    }

    #[test]
    fn pacman_lists_the_files_that_differ() {
        let stdout = "ffmpeg: /usr/lib/libavcodec.so (Size mismatch)\n\
                      ffmpeg: 1204 total files, 1 altered file\n";
        let IntegrityVerdict::Damaged { files } = parse_pacman(stdout, "") else {
            panic!("a size mismatch is damage");
        };
        assert_eq!(files, vec!["/usr/lib/libavcodec.so (Size mismatch)"]);
    }

    #[test]
    fn rpm_keeps_configuration_apart_from_damage() {
        // `/etc/sudoers` differing means somebody edited it. `/usr/bin/sudo`
        // differing means something replaced it. Counting those as the same
        // finding would bury the second one under a pile of the first.
        let stdout = "S.5....T.  c /etc/sudoers\n\
                      S.5....T.    /usr/bin/sudo\n\
                      .......T.    /usr/share/doc/thing\n";
        let (damaged, config) = parse_rpm(stdout);
        assert_eq!(damaged, vec!["/usr/bin/sudo"]);
        assert_eq!(config, vec!["/etc/sudoers"]);
    }

    #[test]
    fn rpm_ignores_a_timestamp_difference_on_its_own() {
        // A file whose mtime moved but whose contents hash the same is not a
        // problem, and listing it would train people to ignore the list.
        let (damaged, config) = parse_rpm(".......T.    /usr/share/doc/thing\n");
        assert!(damaged.is_empty() && config.is_empty());
    }

    #[test]
    fn rpm_treats_a_missing_file_as_damage() {
        let (damaged, _) = parse_rpm("missing     /usr/bin/gone\n");
        assert_eq!(damaged, vec!["/usr/bin/gone"]);
    }

    #[test]
    fn debsums_silence_is_intact() {
        assert_eq!(parse_debsums("", ""), IntegrityVerdict::Intact);
    }

    #[test]
    fn debsums_mismatches_are_collected_from_either_stream() {
        let stderr = "debsums: checksum mismatch /usr/bin/curl\n\
                      debsums: missing file /usr/share/man/man1/curl.1.gz (from curl package)\n";
        let IntegrityVerdict::Damaged { files } = parse_debsums("", stderr) else {
            panic!("a mismatch is damage");
        };
        assert_eq!(
            files,
            vec!["/usr/bin/curl", "/usr/share/man/man1/curl.1.gz"]
        );
    }

    #[test]
    fn an_interrupted_check_never_reads_as_intact() {
        // The one failure mode that would make this check worse than not
        // having it: half a hash check says nothing about the other half.
        let stalled = ExecOutcome::Stalled {
            stdout: String::new(),
            stderr: String::new(),
            idle: std::time::Duration::from_secs(30),
            duration: std::time::Duration::from_secs(90),
        };
        let verdict = from_outcome(stalled, |_, _| IntegrityVerdict::Intact);
        assert!(!verdict.is_intact(), "{verdict:?}");

        let cancelled = ExecOutcome::Cancelled {
            stdout: String::new(),
            stderr: String::new(),
            duration: std::time::Duration::from_secs(5),
        };
        assert!(!from_outcome(cancelled, |_, _| IntegrityVerdict::Intact).is_intact());
    }
}
