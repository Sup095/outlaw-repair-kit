//! Checking whether a newer release exists.
//!
//! This checks and reports. It does not install. A tool that replaces its own
//! binary while running is a class of problem nobody wants to debug at the
//! moment they most need the tool working, and installing software is an
//! outward-facing, hard-to-undo action -- so the decision stays with the
//! person, and this only tells them there is one to make.
//!
//! Being offline is normal and expected, not an error worth interrupting a
//! start-up for. Every failure path here ends in "unknown", quietly.

use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Where release information is read from.
const RELEASES_URL: &str = "https://api.github.com/repos/Sup095/outlaw-repair-kit/releases/latest";

/// The version this binary was built as.
pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Short on purpose. This sits between the user and the thing they asked for.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(4);

/// What the update check found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum UpdateStatus {
    /// This is the newest release.
    UpToDate { version: String },
    /// A newer release exists. `url` is the page a person can download from.
    Available {
        current: String,
        latest: String,
        url: String,
    },
    /// The check could not be completed. Not an error the user must act on.
    Unknown { reason: String },
}

impl UpdateStatus {
    /// A single line suitable for a boot-screen log pane.
    pub fn summary(&self) -> String {
        match self {
            UpdateStatus::UpToDate { version } => format!("up to date (v{version})"),
            UpdateStatus::Available {
                current, latest, ..
            } => {
                format!("v{latest} is available -- this is v{current}")
            }
            UpdateStatus::Unknown { reason } => format!("update check skipped: {reason}"),
        }
    }
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    #[serde(default)]
    html_url: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
}

/// Ask whether a newer release exists.
///
/// Never returns an error: an unreachable network is an ordinary condition
/// here, and the caller has nothing useful to do with a failure but ignore it.
pub async fn check() -> UpdateStatus {
    match try_check().await {
        Ok(status) => status,
        Err(error) => UpdateStatus::Unknown {
            reason: short_reason(&error),
        },
    }
}

async fn try_check() -> anyhow::Result<UpdateStatus> {
    let client = reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        // GitHub rejects requests without one.
        .user_agent(concat!("outlaw-repair-kit/", env!("CARGO_PKG_VERSION")))
        .build()?;

    let response = client
        .get(RELEASES_URL)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await?
        .error_for_status()?;

    let release: Release = response.json().await?;
    Ok(compare(CURRENT_VERSION, &release))
}

fn compare(current: &str, release: &Release) -> UpdateStatus {
    if release.draft || release.prerelease {
        return UpdateStatus::UpToDate {
            version: current.to_string(),
        };
    }

    let latest = release.tag_name.trim_start_matches('v');
    match (parse_version(current), parse_version(latest)) {
        (Some(mine), Some(theirs)) if theirs > mine => UpdateStatus::Available {
            current: current.to_string(),
            latest: latest.to_string(),
            url: if release.html_url.is_empty() {
                "https://github.com/Sup095/outlaw-repair-kit/releases".to_string()
            } else {
                release.html_url.clone()
            },
        },
        (Some(_), Some(_)) => UpdateStatus::UpToDate {
            version: current.to_string(),
        },
        // An unparseable tag means someone changed the naming scheme. Claiming
        // "up to date" would be a guess; say so instead.
        _ => UpdateStatus::Unknown {
            reason: format!("cannot read release tag '{}'", release.tag_name),
        },
    }
}

/// Parse `major.minor.patch`, ignoring any pre-release or build suffix.
fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    let core = text.split(['-', '+']).next()?;
    let mut parts = core.split('.');
    let major = parts.next()?.trim().parse().ok()?;
    let minor = parts.next().unwrap_or("0").trim().parse().ok()?;
    let patch = parts.next().unwrap_or("0").trim().parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

fn short_reason(error: &anyhow::Error) -> String {
    for cause in error.chain() {
        if let Some(request) = cause.downcast_ref::<reqwest::Error>() {
            if request.is_timeout() {
                return "no answer in time".to_string();
            }
            if request.is_connect() {
                return "no connection".to_string();
            }
        }
    }
    error.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(tag: &str) -> Release {
        Release {
            tag_name: tag.to_string(),
            html_url: "https://example.invalid/release".to_string(),
            draft: false,
            prerelease: false,
        }
    }

    #[test]
    fn versions_parse_with_or_without_all_three_parts() {
        assert_eq!(parse_version("1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_version("0.4"), Some((0, 4, 0)));
        assert_eq!(parse_version("2"), Some((2, 0, 0)));
        assert_eq!(parse_version("1.2.3-rc1"), Some((1, 2, 3)));
        assert_eq!(parse_version("1.2.3.4"), None);
        assert_eq!(parse_version("nightly"), None);
    }

    #[test]
    fn a_newer_tag_is_offered_and_an_older_one_is_not() {
        assert!(matches!(
            compare("0.4.0", &release("v0.5.0")),
            UpdateStatus::Available { .. }
        ));
        assert!(matches!(
            compare("0.4.0", &release("v0.4.1")),
            UpdateStatus::Available { .. }
        ));
        assert!(matches!(
            compare("0.4.0", &release("v0.4.0")),
            UpdateStatus::UpToDate { .. }
        ));
        // Running ahead of the last release is normal when building from source.
        assert!(matches!(
            compare("0.5.0", &release("v0.4.0")),
            UpdateStatus::UpToDate { .. }
        ));
    }

    #[test]
    fn ten_sorts_after_nine_rather_than_before_it() {
        // The bug you get for free by comparing tags as strings.
        assert!(matches!(
            compare("0.9.0", &release("v0.10.0")),
            UpdateStatus::Available { .. }
        ));
    }

    #[test]
    fn drafts_and_prereleases_are_not_offered() {
        let mut draft = release("v9.0.0");
        draft.draft = true;
        assert!(matches!(
            compare("0.4.0", &draft),
            UpdateStatus::UpToDate { .. }
        ));

        let mut early = release("v9.0.0");
        early.prerelease = true;
        assert!(matches!(
            compare("0.4.0", &early),
            UpdateStatus::UpToDate { .. }
        ));
    }

    #[test]
    fn an_unreadable_tag_says_unknown_rather_than_guessing() {
        assert!(matches!(
            compare("0.4.0", &release("nightly")),
            UpdateStatus::Unknown { .. }
        ));
    }

    #[test]
    fn the_shipped_version_is_readable() {
        assert!(
            parse_version(CURRENT_VERSION).is_some(),
            "CARGO_PKG_VERSION must be comparable"
        );
    }
}
