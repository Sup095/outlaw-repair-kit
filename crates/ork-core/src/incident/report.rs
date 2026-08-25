//! Turning what went wrong into something postable.
//!
//! The result is a piece of Markdown and a link that opens GitHub's "new
//! issue" form with that Markdown already filled in. The user reads it, edits
//! anything they like, and presses the button themselves.
//!
//! **The tool never posts anything.** It holds no credentials for anyone's
//! account and asks for none; it opens a form. That is not a limitation to be
//! worked around later -- a bug reporter that can publish on your behalf is a
//! thing that can publish your logs without your having read them, and no
//! redactor is good enough to make that acceptable.
//!
//! Everything is put through [`Redactor`] on the way in. See that module for
//! what it removes and what it deliberately leaves alone.

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::{Incident, IncidentKind, Redactor, recent};

/// Where this project lives. Defined once, in the crate root.
pub use crate::REPOSITORY;

/// How many recorded problems to include.
///
/// A report is read by a person. Forty lines is enough to show a pattern and
/// short enough that somebody will actually read it.
const INCLUDED: usize = 40;

/// GitHub's prefilled-issue links are a URL, and a URL that is too long is
/// rejected by the browser rather than truncated. Well under the limit, so
/// there is room for the title and the rest of the query string.
const URL_BUDGET: usize = 6000;

/// A finished report: what to post, and where.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Report {
    pub title: String,
    /// Markdown, already redacted. This is exactly what would be posted.
    pub body: String,
    /// How many recorded problems it covers.
    pub incident_count: usize,
    /// Whether any of them was a crash rather than a handled error.
    pub includes_crash: bool,
    /// The prefilled issue link, when the body is short enough to carry in
    /// one. Otherwise `None`, and the body is meant to be attached to the
    /// issue by hand.
    pub issue_url: Option<String>,
    /// A plain link to the new-issue form, always present. Used when the body
    /// is too long to prefill.
    pub issue_form_url: String,
}

impl Report {
    /// Whether there is anything to report at all.
    pub fn is_empty(&self) -> bool {
        self.incident_count == 0
    }
}

/// What the report should say about the machine it came from.
///
/// Passed in rather than looked up here so the caller decides how much to
/// disclose, and so this can be tested without a real computer.
#[derive(Debug, Clone, Default)]
pub struct Context {
    pub version: String,
    pub platform: String,
    pub os_name: String,
    pub architecture: String,
    /// Optional extra section, e.g. the start-up self-test result.
    pub extra: Vec<(String, String)>,
}

/// Build a report from what has been recorded.
pub fn build(state_dir: &Path, context: &Context) -> Report {
    from_incidents(&recent(state_dir, INCLUDED), context)
}

/// Build a report from a specific set of records.
pub fn from_incidents(incidents: &[Incident], context: &Context) -> Report {
    let redactor = Redactor::for_this_machine();
    let includes_crash = incidents
        .iter()
        .any(|incident| incident.kind == IncidentKind::Panic);

    let title = title_for(incidents, &redactor);
    let body = body_for(incidents, context, &redactor);
    let issue_form_url = format!("{REPOSITORY}/issues/new");
    let issue_url = prefilled_url(&title, &body).filter(|url| url.len() <= URL_BUDGET);

    Report {
        title,
        body,
        incident_count: incidents.len(),
        includes_crash,
        issue_url,
        issue_form_url,
    }
}

/// A title that says what happened, taken from the most recent record.
///
/// The newest is used rather than the first because it is nearly always the
/// one the person is chasing -- the earlier ones are often consequences of it
/// or unrelated noise from the same session.
fn title_for(incidents: &[Incident], redactor: &Redactor) -> String {
    let Some(latest) = incidents.last() else {
        return "Problem report".to_string();
    };

    let what = match latest.kind {
        IncidentKind::Panic => "Crash",
        IncidentKind::Error => "Error",
    };
    let message = redactor.apply(&latest.message);
    let message = message.lines().next().unwrap_or("").trim();
    let message = truncate(message, 90);

    if message.is_empty() {
        format!("{what} in {}", latest.source)
    } else {
        format!("{what}: {message}")
    }
}

fn body_for(incidents: &[Incident], context: &Context, redactor: &Redactor) -> String {
    let mut out = String::new();

    // The blank first, so the form opens with the cursor somewhere useful and
    // the report reads as something a person wrote rather than a dump.
    out.push_str("### What were you doing?\n\n");
    out.push_str("<!-- Anything you remember helps. Delete this line and write here. -->\n\n");

    out.push_str("### Version\n\n");
    out.push_str(&format!(
        "`{}` on {} ({}, {})\n\n",
        blank_to_unknown(&context.version),
        blank_to_unknown(&context.platform),
        blank_to_unknown(&context.os_name),
        blank_to_unknown(&context.architecture),
    ));

    for (heading, text) in &context.extra {
        out.push_str(&format!("### {heading}\n\n"));
        out.push_str(redactor.apply(text).trim());
        out.push_str("\n\n");
    }

    if incidents.is_empty() {
        out.push_str("### What was recorded\n\n");
        out.push_str("Nothing. No errors or crashes have been recorded on this machine.\n\n");
    } else {
        let crashes = incidents
            .iter()
            .filter(|incident| incident.kind == IncidentKind::Panic)
            .count();
        out.push_str(&format!(
            "### What was recorded ({} entr{}, {crashes} crash{})\n\n",
            incidents.len(),
            if incidents.len() == 1 { "y" } else { "ies" },
            if crashes == 1 { "" } else { "es" },
        ));
        out.push_str("```text\n");
        for incident in incidents {
            out.push_str(redactor.apply(&incident.line()).trim_end());
            out.push('\n');
        }
        out.push_str("```\n\n");

        // A backtrace is long and only one of them is ever useful, so the most
        // recent crash gets one and the rest are represented by their line
        // above.
        if let Some(backtrace) = incidents
            .iter()
            .rev()
            .find(|incident| incident.kind == IncidentKind::Panic)
            .and_then(|incident| incident.backtrace.as_deref())
        {
            out.push_str("<details><summary>Backtrace of the most recent crash</summary>\n\n");
            out.push_str("```text\n");
            out.push_str(redactor.apply(backtrace).trim_end());
            out.push_str("\n```\n\n</details>\n\n");
        }
    }

    out.push_str("---\n\n");
    out.push_str(
        "*Personal details were removed automatically before this was shown: home \
         directory paths, account and machine names, email and network addresses, and \
         anything shaped like a key. Please read it through before posting anyway.*\n",
    );
    out
}

fn blank_to_unknown(text: &str) -> &str {
    if text.trim().is_empty() {
        "unknown"
    } else {
        text
    }
}

fn truncate(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let cut: String = text.chars().take(limit.saturating_sub(1)).collect();
    format!("{}…", cut.trim_end())
}

/// GitHub's prefilled new-issue link.
pub fn prefilled_url(title: &str, body: &str) -> Option<String> {
    Some(format!(
        "{REPOSITORY}/issues/new?title={}&body={}",
        encode(title),
        encode(body)
    ))
}

/// Percent-encode for a query string.
///
/// Written out rather than pulled in, because the set of characters that may
/// go through unescaped is small and the cost of getting it wrong is a link
/// that silently loses half the report.
fn encode(text: &str) -> String {
    const UNRESERVED: &[u8] = b"-_.~";
    let mut out = String::with_capacity(text.len() * 3);
    for byte in text.as_bytes() {
        if byte.is_ascii_alphanumeric() || UNRESERVED.contains(byte) {
            out.push(*byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context() -> Context {
        Context {
            version: "0.5.1".to_string(),
            platform: "windows".to_string(),
            os_name: "Windows 10 Pro".to_string(),
            architecture: "x86_64".to_string(),
            extra: Vec::new(),
        }
    }

    fn incident(kind: IncidentKind, message: &str) -> Incident {
        Incident {
            at: "2026-08-25T12:00:00Z".to_string(),
            kind,
            source: "ork_fix::engine".to_string(),
            message: message.to_string(),
            location: Some("crates/ork-fix/src/engine.rs:210".to_string()),
            backtrace: None,
        }
    }

    #[test]
    fn a_report_says_what_it_covers_and_where_it_came_from() {
        let report = from_incidents(&[incident(IncidentKind::Error, "disk full")], &context());
        assert_eq!(report.incident_count, 1);
        assert!(!report.includes_crash);
        assert!(report.body.contains("0.5.1"));
        assert!(report.body.contains("Windows 10 Pro"));
        assert!(report.body.contains("disk full"));
        assert!(report.title.contains("disk full"));
    }

    #[test]
    fn the_title_describes_the_most_recent_problem() {
        // The newest is the one being chased; the earlier ones are usually
        // consequences of it or noise from the same session.
        let report = from_incidents(
            &[
                incident(IncidentKind::Error, "an earlier thing"),
                incident(IncidentKind::Panic, "index out of bounds"),
            ],
            &context(),
        );
        assert!(report.title.starts_with("Crash: "), "{}", report.title);
        assert!(report.title.contains("index out of bounds"));
        assert!(report.includes_crash);
    }

    #[test]
    fn personal_details_never_reach_the_body() {
        // The whole feature rests on this. If it fails, the tool has helped
        // somebody publish their own home directory.
        let report = from_incidents(
            &[incident(
                IncidentKind::Error,
                "could not open /home/jane/.config/outlaw with key sk-ant-abcdefghijklmnop",
            )],
            &context(),
        );
        assert!(!report.body.contains("jane"), "{}", report.body);
        assert!(!report.body.contains("sk-ant"), "{}", report.body);
        assert!(report.body.contains("<home>/.config/outlaw"));
        assert!(!report.title.contains("jane"));
    }

    #[test]
    fn a_machine_with_nothing_wrong_still_produces_a_usable_report() {
        // Somebody may want to report something the tool did not notice, and
        // being told "there is nothing to report" would be unhelpful.
        let report = from_incidents(&[], &context());
        assert!(report.is_empty());
        assert!(report.body.contains("Nothing."));
        assert!(report.issue_url.is_some());
        assert_eq!(report.title, "Problem report");
    }

    #[test]
    fn a_backtrace_is_included_once_and_folded_away() {
        let mut crash = incident(IncidentKind::Panic, "it broke");
        crash.backtrace = Some("frame one\nframe two".to_string());
        let report = from_incidents(&[crash], &context());
        assert!(report.body.contains("<details>"));
        assert!(report.body.contains("frame two"));
    }

    #[test]
    fn a_report_too_long_to_prefill_says_so_rather_than_being_cut_short() {
        // A truncated URL loses the end of the report silently, which is the
        // worst of both outcomes.
        let long = incident(IncidentKind::Error, &"a repeated failure. ".repeat(600));
        let report = from_incidents(&[long], &context());
        assert!(report.issue_url.is_none());
        assert!(report.issue_form_url.ends_with("/issues/new"));
        // The full text still exists; it is just not carried in a link.
        assert!(report.body.len() > URL_BUDGET);
    }

    #[test]
    fn the_link_points_at_this_project_and_survives_a_round_trip() {
        let report = from_incidents(&[incident(IncidentKind::Error, "a & b = c")], &context());
        let url = report.issue_url.expect("a short report prefills");
        assert!(url.starts_with(REPOSITORY), "{url}");
        // Ampersands and equals signs would otherwise end the query parameter
        // early and take the rest of the report with them.
        assert!(!url.contains("a & b"));
        assert!(url.contains("%26"));
    }

    #[test]
    fn extra_sections_are_redacted_like_everything_else() {
        let mut context = context();
        context.extra.push((
            "Start-up self-test".to_string(),
            "snapshot area: cannot write to /home/jane/.outlaw/snapshots".to_string(),
        ));
        let report = from_incidents(&[], &context);
        assert!(report.body.contains("Start-up self-test"));
        assert!(!report.body.contains("jane"));
    }

    #[test]
    fn encoding_leaves_nothing_that_could_end_the_query_early() {
        for text in ["a&b", "a=b", "a#b", "a b", "a+b", "100%", "\"quoted\""] {
            let encoded = encode(text);
            for bad in ['&', '=', '#', ' ', '+', '"'] {
                assert!(
                    !encoded.contains(bad),
                    "{bad:?} survived encoding of {text:?}"
                );
            }
        }
    }
}
