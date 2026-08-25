//! Taking personal details out of text bound for a public bug tracker.
//!
//! An error report is only useful if somebody is willing to post it, and
//! people are right to be wary: log lines are full of home directory paths,
//! machine names, network addresses, and occasionally something much worse
//! that a library decided to print. A report that leaks any of that onto a
//! public issue tracker cannot be taken back.
//!
//! So this errs heavily towards removing too much. Two kinds of rule:
//!
//! * **Things known about this machine** -- the user's name, home directory,
//!   and hostname are looked up and replaced wherever they appear. This is the
//!   half that catches `C:\Users\jane\...` and `jane-desktop`.
//! * **Things that look dangerous anywhere** -- anything shaped like a key, a
//!   token, an email address, or an IP address, whether or not this machine
//!   has ever seen it.
//!
//! The second half is pattern matching, and pattern matching over-reaches. It
//! is allowed to: a report with a version number wrongly blanked is a nuisance,
//! and a report with a credential in it is a disaster. When the two conflict,
//! this file chooses the nuisance.
//!
//! **Nothing here is a substitute for reading the report.** The tool always
//! shows the finished text and never submits anything on its own, precisely
//! because no redactor is good enough to be trusted unread.

/// A placeholder is deliberately conspicuous, so a reader can tell the
/// difference between "the tool removed something here" and "this was empty".
const USER: &str = "<user>";
const HOME: &str = "<home>";
const MACHINE: &str = "<machine>";
const ADDRESS: &str = "<address>";
const EMAIL: &str = "<email>";
const SECRET: &str = "<redacted>";

/// Replaces personal details with placeholders.
#[derive(Debug, Default, Clone)]
pub struct Redactor {
    /// Longest first, so `C:\Users\jane\AppData` is replaced before `jane`.
    /// Replacing the short one first would leave `C:\Users\<user>\AppData`,
    /// which still says where the file was.
    literals: Vec<(String, &'static str)>,
}

impl Redactor {
    /// A redactor that knows nothing about any machine. Pattern rules only.
    pub fn generic() -> Self {
        Self::default()
    }

    /// A redactor that also knows this machine's own identifying details.
    pub fn for_this_machine() -> Self {
        let mut redactor = Self::generic();

        if let Some(home) = dirs::home_dir() {
            let home = home.to_string_lossy().to_string();
            redactor.add(&home, HOME);
            // The account name is usually the last part of the home path, and
            // it turns up on its own in service names and window titles.
            if let Some(name) = home.rsplit(['/', '\\']).find(|part| !part.is_empty()) {
                redactor.add(name, USER);
            }
        }
        for key in ["USER", "USERNAME", "LOGNAME"] {
            if let Ok(name) = std::env::var(key) {
                redactor.add(&name, USER);
            }
        }
        if let Ok(host) = crate::platform::detect().and_then(|platform| platform.host()) {
            redactor.add(&host.hostname, MACHINE);
        }

        redactor
    }

    /// Also remove this exact string wherever it appears.
    ///
    /// Very short values are ignored. A one- or two-character account name
    /// would match inside half the words in the report and turn it into
    /// nonsense, which helps nobody.
    pub fn add(&mut self, literal: &str, placeholder: &'static str) {
        let literal = literal.trim();
        if literal.len() < 3 {
            return;
        }
        if self.literals.iter().any(|(known, _)| known == literal) {
            return;
        }
        self.literals.push((literal.to_string(), placeholder));
        self.literals
            .sort_by(|(a, _), (b, _)| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
    }

    /// Clean a piece of text.
    pub fn apply(&self, text: &str) -> String {
        let mut out = text.to_string();
        for (literal, placeholder) in &self.literals {
            out = replace_ignoring_case(&out, literal, placeholder);
        }
        out = strip_home_paths(&out);
        out = strip_emails(&out);
        out = strip_addresses(&out);
        out = strip_secrets(&out);
        out
    }
}

/// Case-insensitive replacement.
///
/// Windows says `C:\Users\Jane` in one place and `c:\users\jane` in another,
/// and a redactor that only catches one spelling has not caught it.
fn replace_ignoring_case(haystack: &str, needle: &str, replacement: &str) -> String {
    let lower_haystack = haystack.to_lowercase();
    let lower_needle = needle.to_lowercase();
    if lower_needle.is_empty() {
        return haystack.to_string();
    }

    // Lowercasing can change a string's byte length, so indices from the
    // lowercased copy are not safe to use against the original. Both are
    // walked together instead.
    let mut out = String::with_capacity(haystack.len());
    let mut rest = haystack;
    let mut lower_rest = lower_haystack.as_str();
    while let Some(at) = lower_rest.find(&lower_needle) {
        // Only splits on a boundary that exists in both copies.
        let Some(prefix) = rest.get(..at) else {
            break;
        };
        let Some(tail) = rest.get(at + lower_needle.len()..) else {
            break;
        };
        out.push_str(prefix);
        out.push_str(replacement);
        rest = tail;
        lower_rest = &lower_rest[at + lower_needle.len()..];
    }
    out.push_str(rest);
    out
}

/// Home directory paths belonging to somebody other than this account.
///
/// A report can carry a path from another machine -- a linked computer, a
/// pasted log, a path recorded before the account was renamed -- and this
/// machine's own name will not match it.
fn strip_home_paths(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;

    const ROOTS: &[&str] = &["/home/", "/Users/", "\\Users\\", "/users/", "\\users\\"];
    'outer: loop {
        let found = ROOTS
            .iter()
            .filter_map(|root| rest.find(root).map(|at| (at, *root)))
            .min_by_key(|(at, _)| *at);

        let Some((at, root)) = found else {
            break 'outer;
        };

        // A drive letter in front belongs to the path, not to the sentence.
        // Without this, `C:/Users/jane/x` becomes `C:<home>/x`, which reads as
        // though the tool half-finished the job.
        let mut cut = at;
        let before = &rest[..at];
        if before.ends_with(':') {
            let letters = before.trim_end_matches(':');
            if letters
                .chars()
                .next_back()
                .is_some_and(|c| c.is_ascii_alphabetic())
                && (letters.len() == 1
                    || letters
                        .chars()
                        .nth_back(1)
                        .is_some_and(|c| !c.is_ascii_alphanumeric()))
            {
                cut -= 2;
            }
        }

        out.push_str(&rest[..cut]);
        out.push_str(HOME);

        let after = &rest[at + root.len()..];
        // Everything up to the next separator is the account name, and goes
        // with it. What follows is a path inside the account, which is worth
        // keeping: "<home>/.config/outlaw" is diagnostic, "<home>" is not.
        let end = after
            .find(['/', '\\', ' ', '"', '\'', ',', ')', ']'])
            .unwrap_or(after.len());
        rest = &after[end..];
    }
    out.push_str(rest);
    out
}

fn strip_emails(text: &str) -> String {
    map_tokens(text, |token| {
        // Punctuation around the word goes first: an address at the end of a
        // sentence is `someone@example.com,` and the comma would otherwise
        // make the domain look malformed.
        let bare = token.trim_matches(|c: char| !c.is_ascii_alphanumeric());
        let (local, domain) = bare.split_once('@')?;
        let plausible = !local.is_empty()
            && domain.contains('.')
            && !domain.starts_with('.')
            && !domain.ends_with('.')
            && domain
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
        plausible.then(|| EMAIL.to_string())
    })
}

/// Network addresses, which say where somebody is and who they are with.
///
/// Loopback and the unspecified address are kept: they carry no information
/// about anyone, and "it tried to reach 127.0.0.1:11434" is often the whole
/// explanation for a local model failing.
fn strip_addresses(text: &str) -> String {
    map_tokens(text, |token| {
        let bare = token.trim_matches(|c: char| !c.is_ascii_alphanumeric());
        let (host, port) = match bare.rsplit_once(':') {
            Some((host, port)) if port.chars().all(|c| c.is_ascii_digit()) && !port.is_empty() => {
                (host, Some(port))
            }
            _ => (bare, None),
        };

        let octets: Vec<&str> = host.split('.').collect();
        if octets.len() != 4 || !octets.iter().all(|part| is_octet(part)) {
            return None;
        }
        if host == "127.0.0.1" || host == "0.0.0.0" {
            return None;
        }

        Some(match port {
            Some(port) => format!("{ADDRESS}:{port}"),
            None => ADDRESS.to_string(),
        })
    })
}

fn is_octet(part: &str) -> bool {
    !part.is_empty()
        && part.len() <= 3
        && part.chars().all(|c| c.is_ascii_digit())
        && part.parse::<u16>().is_ok_and(|value| value <= 255)
}

/// Anything shaped like a credential.
///
/// This is the rule that is allowed to be wrong. A long opaque run of
/// characters is either a key, a hash, or an identifier, and there is no
/// reliable way to tell them apart from the outside -- so all three go. A
/// report missing a commit hash is a nuisance; a report carrying somebody's
/// API key is not recoverable.
fn strip_secrets(text: &str) -> String {
    map_tokens(text, |token| {
        let bare = token.trim_matches(|c: char| !c.is_ascii_alphanumeric());

        // Named prefixes first: these are unambiguous even when short.
        const PREFIXES: &[&str] = &[
            "sk-",
            "sk_",
            "pk-",
            "pk_",
            "ghp_",
            "gho_",
            "ghs_",
            "github_pat_",
            "xoxb-",
            "xoxp-",
            "AKIA",
            "ASIA",
            "AIza",
            "hf_",
            "Bearer",
        ];
        if PREFIXES
            .iter()
            .any(|prefix| bare.len() > prefix.len() && bare.starts_with(prefix))
        {
            return Some(SECRET.to_string());
        }

        looks_opaque(bare).then(|| SECRET.to_string())
    })
}

/// Whether a run of characters is long and mixed enough to be a secret.
fn looks_opaque(token: &str) -> bool {
    // Twenty is above anything that turns up in ordinary prose or in a version
    // string, and below the length of every credential format worth worrying
    // about.
    if token.len() < 20 {
        return false;
    }
    if !token
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return false;
    }
    let digits = token.chars().filter(char::is_ascii_digit).count();
    let letters = token.chars().filter(|c| c.is_ascii_alphabetic()).count();
    // Both kinds of character, and not a word. `unrecognised-configuration` is
    // long but is all letters and separators; a key is not.
    digits > 0 && letters > 0
}

/// Apply a rule to every whitespace-separated token, keeping the spacing.
///
/// Punctuation attached to a token is preserved around the replacement, so a
/// redacted address at the end of a sentence still has its full stop.
fn map_tokens(text: &str, rule: impl Fn(&str) -> Option<String>) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;

    while !rest.is_empty() {
        let start = rest
            .find(|c: char| !c.is_whitespace())
            .unwrap_or(rest.len());
        out.push_str(&rest[..start]);
        rest = &rest[start..];
        if rest.is_empty() {
            break;
        }

        let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        let token = &rest[..end];
        match rule(token) {
            Some(replacement) => {
                let leading: String = token
                    .chars()
                    .take_while(|c| !c.is_ascii_alphanumeric())
                    .collect();
                let trailing: String = token
                    .chars()
                    .rev()
                    .take_while(|c| !c.is_ascii_alphanumeric())
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect();
                // A token that is entirely punctuation has no inside to
                // replace, and would otherwise be printed twice.
                if leading.len() + trailing.len() >= token.len() {
                    out.push_str(token);
                } else {
                    out.push_str(&leading);
                    out.push_str(&replacement);
                    out.push_str(&trailing);
                }
            }
            None => out.push_str(token),
        }
        rest = &rest[end..];
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with(literals: &[(&str, &'static str)]) -> Redactor {
        let mut redactor = Redactor::generic();
        for (literal, placeholder) in literals {
            redactor.add(literal, placeholder);
        }
        redactor
    }

    #[test]
    fn a_home_directory_goes_before_the_account_name_inside_it() {
        // Replacing the short one first would leave "C:\Users\<user>\...",
        // which still says exactly where the file was.
        let redactor = with(&[("C:\\Users\\jane", HOME), ("jane", USER)]);
        let cleaned = redactor.apply("failed to open C:\\Users\\jane\\AppData\\state.db");
        assert!(cleaned.contains("<home>\\AppData\\state.db"), "{cleaned}");
        assert!(!cleaned.contains("jane"));
    }

    #[test]
    fn a_name_is_caught_whatever_its_capitalisation() {
        // Windows says C:\Users\Jane in one place and c:\users\jane in
        // another. Catching one spelling is not catching it.
        let redactor = with(&[("Jane", USER)]);
        assert_eq!(
            redactor.apply("user jane and JANE"),
            "user <user> and <user>"
        );
    }

    #[test]
    fn a_home_path_from_another_machine_is_still_removed() {
        // This one belongs to nobody on this computer, so no literal matches
        // it -- the shape has to be enough.
        let cleaned = Redactor::generic().apply("copied from /home/otherperson/.config/outlaw");
        assert_eq!(cleaned, "copied from <home>/.config/outlaw");
    }

    #[test]
    fn the_path_inside_a_home_directory_is_kept() {
        // "<home>/.steam/steam.pid" is the diagnostic part. Removing it too
        // would leave a report that says a file failed without saying which.
        let cleaned = Redactor::generic().apply("stale lock at /Users/sam/.steam/steam.pid");
        assert_eq!(cleaned, "stale lock at <home>/.steam/steam.pid");
    }

    #[test]
    fn a_drive_letter_in_front_of_a_home_path_goes_with_it() {
        // Windows paths turn up with mixed separators, so the literal rule
        // often misses and the shape rule has to catch it -- leaving
        // "C:<home>/x", which reads as a half-finished job.
        let cleaned = Redactor::generic().apply("could not write C:/Users/jane/x.md");
        assert_eq!(cleaned, "could not write <home>/x.md");

        let backslashes = Redactor::generic().apply(r"at D:\Users\sam\.outlaw");
        assert_eq!(backslashes, r"at <home>\.outlaw");
    }

    #[test]
    fn a_word_ending_in_a_colon_is_not_mistaken_for_a_drive() {
        // "path:" and "note:" precede paths in log lines all the time.
        let cleaned = Redactor::generic().apply("path: /Users/jane/x.md");
        assert_eq!(cleaned, "path: <home>/x.md");
    }

    #[test]
    fn an_email_address_is_removed() {
        let cleaned = Redactor::generic().apply("reported by someone@example.com, thanks");
        assert_eq!(cleaned, "reported by <email>, thanks");
    }

    #[test]
    fn a_network_address_is_removed_but_its_port_is_kept() {
        // Which port was refused is the useful half and identifies nobody.
        let cleaned = Redactor::generic().apply("could not reach 192.168.1.44:11434");
        assert_eq!(cleaned, "could not reach <address>:11434");
    }

    #[test]
    fn loopback_survives_because_it_says_nothing_about_anyone() {
        // And it is frequently the entire explanation for a local model that
        // is not answering.
        let cleaned = Redactor::generic().apply("connection refused at 127.0.0.1:11434");
        assert_eq!(cleaned, "connection refused at 127.0.0.1:11434");
    }

    #[test]
    fn a_version_number_is_not_mistaken_for_an_address() {
        let cleaned = Redactor::generic().apply("outlaw 0.5.1 on windows");
        assert_eq!(cleaned, "outlaw 0.5.1 on windows");
    }

    #[test]
    fn anything_shaped_like_a_key_is_removed() {
        for token in [
            "sk-ant-api03-abcdefghijklmnop",
            "ghp_16CharactersOrMoreHere00",
            "AKIAIOSFODNN7EXAMPLE",
            "AIzaSyD-1234567890abcdefgh",
            "a1b2c3d4e5f6a7b8c9d0e1f2a3b4",
        ] {
            let cleaned = Redactor::generic().apply(&format!("token {token} used"));
            assert_eq!(cleaned, "token <redacted> used", "{token} survived");
        }
    }

    #[test]
    fn ordinary_words_and_identifiers_survive() {
        // Over-reach is tolerated, but a report where every word is <redacted>
        // is not a report.
        for text in [
            "the snapshot directory is read-only",
            "probe apps.launch-check failed",
            "service.stopped for spooler",
            "unrecognised-configuration-value",
        ] {
            assert_eq!(Redactor::generic().apply(text), text, "{text} was mangled");
        }
    }

    #[test]
    fn punctuation_around_a_redacted_word_is_kept() {
        let cleaned = Redactor::generic().apply("contact (someone@example.com).");
        assert_eq!(cleaned, "contact (<email>).");
    }

    #[test]
    fn a_very_short_literal_is_refused_rather_than_shredding_the_report() {
        // An account called "jo" would otherwise match inside "job",
        // "json", and "major".
        let mut redactor = Redactor::generic();
        redactor.add("jo", USER);
        assert_eq!(redactor.apply("the json job"), "the json job");
    }

    #[test]
    fn this_machines_own_home_directory_is_removed() {
        // The literal rules are looked up from the running machine, so this is
        // the only test that proves they were wired up at all. Everything else
        // here feeds the redactor names by hand.
        let Some(home) = dirs::home_dir() else {
            return;
        };
        let home = home.to_string_lossy().to_string();
        let line = format!("could not write {home}/.outlaw/state.db");

        let cleaned = Redactor::for_this_machine().apply(&line);
        assert!(
            !cleaned.contains(&home),
            "the home directory survived: {cleaned}"
        );
        assert!(
            cleaned.contains("state.db"),
            "the useful part was lost: {cleaned}"
        );
    }

    #[test]
    fn this_machines_own_name_is_removed() {
        let Ok(host) = crate::platform::detect().and_then(|platform| platform.host()) else {
            return;
        };
        if host.hostname.trim().len() < 3 {
            return;
        }
        let cleaned =
            Redactor::for_this_machine().apply(&format!("scan of {} failed", host.hostname));
        assert!(
            !cleaned.contains(&host.hostname),
            "the machine name survived: {cleaned}"
        );
    }

    #[test]
    fn redaction_never_panics_on_awkward_text() {
        // Log lines contain whatever a library decided to print, including
        // text where lowercasing changes the byte length.
        for text in [
            "",
            "     ",
            "İstanbul C:\\Users\\İrem\\file",
            "ß".repeat(50).as_str(),
            "@@@ ... :::",
            "user@",
            "@example.com",
        ] {
            let _ = Redactor::generic().apply(text);
            let _ = with(&[("İrem", USER)]).apply(text);
        }
    }
}
