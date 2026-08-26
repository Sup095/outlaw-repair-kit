//! Finding out what there is to install, and getting it honestly.
//!
//! Everything here is deliberately dull. An installer is a program that
//! downloads code from the internet and puts it somewhere your machine will
//! run it from, which makes it the most dangerous thing in this repository by
//! a wide margin. So: one host, https only, every file checked against a
//! published digest before it is used, and a refusal rather than a warning
//! when anything does not line up.

use std::io::Read;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use sha2::{Digest, Sha256};

/// The only place this program will download anything from.
///
/// Not configurable, and not by oversight. An installer that can be pointed at
/// a different host is an installer that can be pointed at somebody else's
/// host, and the whole value of the checksum check evaporates if the checksums
/// come from wherever the binaries did.
pub const REPO: &str = "Sup095/outlaw-repair-kit";

const API: &str = "https://api.github.com/repos";
const AGENT: &str = concat!("outlaw-setup/", env!("CARGO_PKG_VERSION"));

/// One published release.
#[derive(Debug, Clone)]
pub struct Release {
    pub tag: String,
    pub prerelease: bool,
    pub assets: Vec<Asset>,
}

#[derive(Debug, Clone)]
pub struct Asset {
    pub name: String,
    pub url: String,
    pub size: u64,
}

#[derive(Deserialize)]
struct RawRelease {
    tag_name: String,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    assets: Vec<RawAsset>,
}

#[derive(Deserialize)]
struct RawAsset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    size: u64,
}

impl Release {
    /// The asset whose name matches, if this release published one.
    pub fn asset(&self, name: &str) -> Option<&Asset> {
        self.assets.iter().find(|asset| asset.name == name)
    }

    /// The first asset whose name ends this way.
    ///
    /// Used for the desktop bundle, whose name carries the version. Matching
    /// on the ending rather than reconstructing the whole name means a release
    /// that named its files slightly differently is still installable.
    pub fn asset_ending(&self, suffix: &str) -> Option<&Asset> {
        self.assets
            .iter()
            .find(|asset| asset.name.ends_with(suffix))
    }
}

fn client() -> Result<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        // A connection that never answers must not hang the window for ever.
        // This is a limit on *establishing* a connection, not on how long a
        // download may take: a slow link is not a broken one.
        .connect_timeout(Duration::from_secs(20))
        .user_agent(AGENT)
        .https_only(true)
        .build()
        .context("could not set up the downloader")
}

/// Every published release, newest first.
///
/// Drafts are dropped -- they are not published and their assets are not
/// downloadable. Pre-releases are kept but flagged, so the window can offer
/// them without pretending they are the recommended choice.
pub fn list() -> Result<Vec<Release>> {
    let url = format!("{API}/{REPO}/releases?per_page=20");
    let response = client()?
        .get(&url)
        .send()
        .context("could not reach GitHub to ask what has been released")?;

    if !response.status().is_success() {
        bail!(
            "GitHub answered {} when asked what has been released",
            response.status()
        );
    }

    let raw: Vec<RawRelease> = response
        .json()
        .context("GitHub's answer was not in a form this understands")?;

    Ok(raw
        .into_iter()
        .filter(|release| !release.draft)
        .map(|release| Release {
            tag: release.tag_name,
            prerelease: release.prerelease,
            assets: release
                .assets
                .into_iter()
                .map(|asset| Asset {
                    name: asset.name,
                    url: asset.browser_download_url,
                    size: asset.size,
                })
                .collect(),
        })
        .collect())
}

/// Download an asset into memory, reporting progress as it goes.
///
/// Held in memory rather than streamed to disk on purpose: nothing should
/// reach the file system until its checksum has been checked, and a file
/// written first and validated afterwards is a file that existed, briefly, as
/// something nobody had verified. The largest thing here is a desktop bundle
/// of a few tens of megabytes.
pub fn download(asset: &Asset, mut progress: impl FnMut(u64, u64)) -> Result<Vec<u8>> {
    let mut response = client()?
        .get(&asset.url)
        .send()
        .with_context(|| format!("could not download {}", asset.name))?;

    if !response.status().is_success() {
        bail!("downloading {} returned {}", asset.name, response.status());
    }

    let expected = response.content_length().unwrap_or(asset.size);
    let mut body = Vec::with_capacity(expected as usize);
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = response
            .read(&mut buffer)
            .with_context(|| format!("the download of {} was cut off", asset.name))?;
        if read == 0 {
            break;
        }
        body.extend_from_slice(&buffer[..read]);
        progress(body.len() as u64, expected);
    }

    Ok(body)
}

/// Download something small, where progress would be noise.
pub fn fetch_text(url: &str) -> Result<String> {
    let response = client()?
        .get(url)
        .send()
        .with_context(|| format!("could not fetch {url}"))?;
    if !response.status().is_success() {
        bail!("{url} returned {}", response.status());
    }
    response.text().context("that download was not text")
}

/// The digest of some bytes, lower-case hex, as `sha256sum` writes it.
pub fn digest(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Read a `SHA256SUMS` file into (name, digest) pairs.
///
/// The format is `<digest><space><space-or-star><name>`, which is what
/// `sha256sum` writes and what the release workflow publishes. Lines that do
/// not look like that are ignored rather than guessed at.
pub fn parse_sums(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            let (digest, name) = line.split_once(' ')?;
            // A SHA-256 digest is 64 hex characters. Anything else on this
            // line is not one, and treating it as one would mean comparing a
            // download against a value that means nothing.
            if digest.len() != 64 || !digest.chars().all(|c| c.is_ascii_hexdigit()) {
                return None;
            }
            let name = name.trim_start_matches([' ', '*']).trim();
            if name.is_empty() {
                return None;
            }
            Some((name.to_string(), digest.to_ascii_lowercase()))
        })
        .collect()
}

/// What checking a download against the published sums concluded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// The digest matched the published one.
    Matches,
    /// It did not. Nothing gets installed.
    Mismatch { expected: String, actual: String },
    /// The release published no digest for this file.
    NotPublished,
}

/// Check one downloaded file against a parsed `SHA256SUMS`.
pub fn verify(name: &str, bytes: &[u8], sums: &[(String, String)]) -> Verdict {
    let actual = digest(bytes);
    match sums.iter().find(|(file, _)| file == name) {
        None => Verdict::NotPublished,
        Some((_, expected)) if *expected == actual => Verdict::Matches,
        Some((_, expected)) => Verdict::Mismatch {
            expected: expected.clone(),
            actual,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sums_file_is_read_the_way_sha256sum_writes_one() {
        let text = "\
e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  outlaw.exe
5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03 *outlaw.zip
";
        let sums = parse_sums(text);
        assert_eq!(sums.len(), 2);
        assert_eq!(sums[0].0, "outlaw.exe");
        // The `*` marks a binary file to sha256sum and is not part of the name.
        assert_eq!(sums[1].0, "outlaw.zip");
    }

    #[test]
    fn lines_that_are_not_checksums_are_ignored_rather_than_guessed_at() {
        // A short or non-hex value is not a digest, and treating one as a
        // digest means comparing a download against something meaningless --
        // which would pass or fail for no reason at all.
        let text = "\
# a comment
not-a-digest  outlaw.exe
abc123  outlaw.exe
e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  real.bin
zzz0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  fake.bin
";
        let sums = parse_sums(text);
        assert_eq!(sums.len(), 1);
        assert_eq!(sums[0].0, "real.bin");
    }

    #[test]
    fn an_empty_file_hashes_to_the_known_value() {
        // The one SHA-256 digest worth hard-coding: if this is wrong, every
        // other check in this module is wrong in the same direction.
        assert_eq!(
            digest(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn a_matching_download_matches() {
        let sums = parse_sums(&format!("{}  outlaw.exe", digest(b"hello")));
        assert_eq!(verify("outlaw.exe", b"hello", &sums), Verdict::Matches);
    }

    #[test]
    fn a_single_altered_byte_is_a_mismatch() {
        // The case this whole module exists for.
        let sums = parse_sums(&format!("{}  outlaw.exe", digest(b"hello")));
        let verdict = verify("outlaw.exe", b"hellp", &sums);
        assert!(matches!(verdict, Verdict::Mismatch { .. }), "{verdict:?}");
    }

    #[test]
    fn a_file_with_no_published_digest_is_not_quietly_accepted() {
        // "No digest" and "digest matched" must never be the same answer. The
        // window says which it was and makes the person decide.
        let sums = parse_sums(&format!("{}  something-else", digest(b"hello")));
        assert_eq!(verify("outlaw.exe", b"hello", &sums), Verdict::NotPublished);
    }

    #[test]
    fn the_digest_is_compared_regardless_of_letter_case() {
        // sha256sum writes lower case; a hand-written sums file might not.
        let upper = digest(b"hello").to_ascii_uppercase();
        let sums = parse_sums(&format!("{upper}  outlaw.exe"));
        assert_eq!(verify("outlaw.exe", b"hello", &sums), Verdict::Matches);
    }

    #[test]
    fn an_asset_is_found_by_its_whole_name_or_by_its_ending() {
        let release = Release {
            tag: "v0.6.0".to_string(),
            prerelease: false,
            assets: vec![
                Asset {
                    name: "SHA256SUMS".to_string(),
                    url: "https://example.invalid/SHA256SUMS".to_string(),
                    size: 200,
                },
                Asset {
                    name: "outlaw-repair-kit-v0.6.0-x64-setup.exe".to_string(),
                    url: "https://example.invalid/setup.exe".to_string(),
                    size: 9_000_000,
                },
            ],
        };
        assert!(release.asset("SHA256SUMS").is_some());
        assert!(release.asset("nothing-like-this").is_none());
        assert_eq!(
            release.asset_ending("-x64-setup.exe").map(|a| a.size),
            Some(9_000_000)
        );
    }
}
