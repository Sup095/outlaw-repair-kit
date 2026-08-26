//! The documentation, carried inside the program.
//!
//! Every page in `docs/` is compiled into the binary. Not fetched, not linked
//! to -- carried. There are three reasons, and the first is the one that
//! matters.
//!
//! A machine that has gone wrong is frequently a machine that cannot reach the
//! internet, and "your network adapter is not working, see the web page about
//! it" is not help. The pages most likely to be needed are the ones least
//! likely to be reachable when they are needed.
//!
//! Second, a page compiled into this build describes *this* build. A link to
//! the project page describes whatever has been written since, which for
//! somebody running a version from six months ago is a document about a
//! program they do not have.
//!
//! Third, reading the manual should not require handing a browser a request
//! that says which page of a diagnostic tool's documentation somebody is
//! looking at, on the day their computer broke.
//!
//! The cost is a few tens of kilobytes of text in the binary, which is
//! nothing, and the discipline that these files stay accurate -- which they
//! have to be anyway.

/// One page of the manual, as written.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Page {
    /// Stable identifier, matching the file name without its extension.
    pub id: &'static str,
    pub title: &'static str,
    /// One line, for a list of pages.
    pub summary: &'static str,
    /// The page itself, in Markdown, exactly as it is in the repository.
    pub body: &'static str,
}

/// Every page, in the order somebody meeting this for the first time would
/// want them: what it is, how to get it, how to use it, then the details.
pub const PAGES: &[Page] = &[
    Page {
        id: "getting-started",
        title: "Getting started",
        summary: "Install it and run your first scan.",
        body: include_str!("../../../docs/getting-started.md"),
    },
    Page {
        id: "changelog",
        title: "What changed",
        summary: "Every released version, and what it changed for somebody using it.",
        body: include_str!("../../../CHANGELOG.md"),
    },
    Page {
        id: "install",
        title: "Installing",
        summary: "Every way in, including building from source.",
        body: include_str!("../../../docs/install.md"),
    },
    Page {
        id: "commands",
        title: "Command reference",
        summary: "What every command does, and every option it takes.",
        body: include_str!("../../../docs/commands.md"),
    },
    Page {
        id: "desktop",
        title: "The desktop app",
        summary: "The window, and the start-up self-test.",
        body: include_str!("../../../docs/desktop.md"),
    },
    Page {
        id: "fixing",
        title: "Fixing problems safely",
        summary: "What it will and will not change, and what it does first.",
        body: include_str!("../../../docs/fixing.md"),
    },
    Page {
        id: "watching",
        title: "Watching for changes",
        summary: "Notice a problem appearing, instead of going looking for one.",
        body: include_str!("../../../docs/watching.md"),
    },
    Page {
        id: "ai-setup",
        title: "Setting up a model",
        summary: "Local, another machine, or hosted -- and why none is required.",
        body: include_str!("../../../docs/ai-setup.md"),
    },
    Page {
        id: "linking",
        title: "Linking two machines",
        summary: "Pair two computers so one can lend the other a model.",
        body: include_str!("../../../docs/linking.md"),
    },
    Page {
        id: "remote-machine",
        title: "Using another machine",
        summary: "Point at an endpoint by hand, over any network.",
        body: include_str!("../../../docs/remote-machine.md"),
    },
    Page {
        id: "runbooks",
        title: "Writing runbooks",
        summary: "Teach it about a problem it does not know.",
        body: include_str!("../../../docs/runbooks.md"),
    },
    Page {
        id: "reporting",
        title: "Reporting a problem",
        summary: "Turn a crash into an issue, with your details taken out.",
        body: include_str!("../../../docs/reporting.md"),
    },
    Page {
        id: "troubleshooting",
        title: "Troubleshooting",
        summary: "When the tool itself is not working.",
        body: include_str!("../../../docs/troubleshooting.md"),
    },
    Page {
        id: "architecture",
        title: "Architecture",
        summary: "How the pieces fit together, and why they are separate.",
        body: include_str!("../../../docs/architecture.md"),
    },
];

/// The licence, carried for the same reason as everything else here.
pub const LICENCE: &str = include_str!("../../../LICENSE");

/// One page by its identifier.
pub fn find(id: &str) -> Option<&'static Page> {
    PAGES.iter().find(|page| page.id == id)
}

/// A page's identifiers and titles, without the text of any of them.
///
/// For a list of contents, where sending every page would mean sending the
/// whole manual to draw a dozen headings.
pub fn contents() -> Vec<(&'static str, &'static str, &'static str)> {
    PAGES
        .iter()
        .map(|page| (page.id, page.title, page.summary))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_page_carries_something() {
        // `include_str!` on a file that has been emptied still compiles. This
        // is what notices.
        for page in PAGES {
            assert!(
                page.body.len() > 200,
                "{} is {} bytes, which is not a page",
                page.id,
                page.body.len()
            );
            assert!(!page.title.is_empty(), "{} has no title", page.id);
            assert!(!page.summary.is_empty(), "{} has no summary", page.id);
        }
        assert!(LICENCE.contains("MIT"), "the licence is not the licence");
    }

    #[test]
    fn a_page_starts_with_the_heading_it_claims() {
        // The title shown in the list and the heading at the top of the page
        // are written in two different places, so they can drift. A person
        // clicking "Installing" and landing on something headed differently
        // has been told the tool is confused about itself.
        for page in PAGES {
            let heading = page
                .body
                .lines()
                .find(|line| line.starts_with("# "))
                .unwrap_or_default()
                .trim_start_matches("# ")
                .trim();
            assert!(!heading.is_empty(), "{} has no heading at all", page.id);
            // The changelog is titled for what somebody wants from it rather
            // than for what the file is called, which is the one place these
            // two are allowed to differ.
            if page.id == "changelog" {
                continue;
            }
            assert!(
                heading.eq_ignore_ascii_case(page.title)
                    || heading.to_lowercase().contains(&page.title.to_lowercase()),
                "{}: listed as {:?} but headed {:?}",
                page.id,
                page.title,
                heading
            );
        }
    }

    #[test]
    fn identifiers_are_unique_and_findable() {
        let mut seen = std::collections::HashSet::new();
        for page in PAGES {
            assert!(seen.insert(page.id), "{} appears twice", page.id);
            assert_eq!(find(page.id).map(|found| found.id), Some(page.id));
        }
        assert!(find("no-such-page").is_none());
    }

    #[test]
    fn the_contents_list_carries_no_page_bodies() {
        // The whole point of it: drawing a list of a dozen headings should not
        // mean shipping the entire manual across to do it.
        let contents = contents();
        assert_eq!(contents.len(), PAGES.len());
        let rendered = format!("{contents:?}");
        assert!(
            rendered.len() < 4_000,
            "the contents list is {} bytes, which means it is carrying page bodies",
            rendered.len()
        );
    }

    #[test]
    fn the_manual_covers_what_the_project_says_it_covers() {
        // A page added to `docs/` and not added here is a page nobody using
        // the window will ever see. This is the reminder.
        for expected in [
            "getting-started",
            "changelog",
            "install",
            "commands",
            "desktop",
            "fixing",
            "watching",
            "ai-setup",
            "linking",
            "remote-machine",
            "runbooks",
            "reporting",
            "troubleshooting",
            "architecture",
        ] {
            assert!(find(expected).is_some(), "{expected} is missing");
        }
    }
}
