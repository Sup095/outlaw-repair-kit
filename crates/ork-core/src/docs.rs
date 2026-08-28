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
        id: "startup",
        title: "What starts with your computer",
        summary: "Everything that runs on its own, and why this is not a rootkit scan.",
        body: include_str!("../../../docs/startup.md"),
    },
    Page {
        id: "stress",
        title: "Stress and burn-in",
        summary: "Work the machine hard on purpose, to find what watching cannot.",
        body: include_str!("../../../docs/stress.md"),
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

    /// Pages in `docs/` that deliberately are not carried in the binary.
    ///
    /// Only one, and only because it is the index of the folder for somebody
    /// browsing the repository -- a list of links to the other files, which is
    /// exactly what `outlaw docs` with no argument already prints, from the
    /// list below rather than from a file that could disagree with it.
    const NOT_CARRIED: &[&str] = &["README"];

    #[test]
    fn every_page_in_the_folder_is_carried_in_the_binary() {
        // Reads the directory rather than trusting a list, because `PAGES` is
        // written by hand and a page added to `docs/` without a line here
        // compiles perfectly and is simply absent from the program. Nothing
        // else would notice: the file exists, the links to it work on the web,
        // and the machine that cannot reach the web is the one that needed it.
        let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs");
        let mut missing = Vec::new();

        for entry in std::fs::read_dir(&folder)
            .expect("the docs folder")
            .flatten()
        {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("md") {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
                continue;
            };
            if NOT_CARRIED.contains(&stem) {
                continue;
            }
            if find(stem).is_none() {
                missing.push(stem.to_string());
            }
        }

        assert!(
            missing.is_empty(),
            "written but not carried: {}. Add each to `PAGES`, or to `NOT_CARRIED` with a reason.",
            missing.join(", ")
        );
    }

    #[test]
    fn every_carried_page_is_a_file_that_exists() {
        // The other direction. A page whose file was renamed would fail to
        // compile, but one pointed at a file elsewhere in the tree would not,
        // and `outlaw docs` is meant to be the same text the repository has.
        let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs");
        for page in PAGES {
            // The changelog is the exception by design: it lives at the top
            // of the repository because that is where people look for it.
            if page.id == "changelog" {
                continue;
            }
            let path = folder.join(format!("{}.md", page.id));
            assert!(
                path.exists(),
                "`{}` is carried but `docs/{}.md` does not exist",
                page.id,
                page.id
            );
        }
    }

    #[test]
    fn no_page_links_to_a_file_that_is_not_there() {
        // The front page was checked for broken links and the pages it leads
        // to were not, which is the wrong way round: somebody following
        // `docs/watching.md` to a page that was renamed is already several
        // steps in and has no reason to doubt the trail.
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let docs = root.join("docs");
        let mut broken = Vec::new();

        for page in PAGES {
            // Links are written relative to the file they are in, and every
            // page except the changelog lives in `docs/`.
            let from = if page.id == "changelog" { &root } else { &docs };
            for target in links(page.body) {
                if target.starts_with("http") || target.starts_with('#') || target.is_empty() {
                    continue;
                }
                let path = target.split('#').next().unwrap_or(&target);
                if !from.join(path).exists() {
                    broken.push(format!("{} -> {path}", page.id));
                }
            }
        }

        assert!(broken.is_empty(), "links to nothing: {broken:?}");
    }

    /// Every markdown link target in a page.
    fn links(body: &str) -> Vec<String> {
        let mut found = Vec::new();
        let mut rest = body;
        while let Some(at) = rest.find("](") {
            let after = &rest[at + 2..];
            let Some(end) = after.find(')') else { break };
            found.push(after[..end].to_string());
            rest = &after[end..];
        }
        found
    }

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
            "startup",
            "stress",
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
