//! The machines this one is linked to, and the tokens that prove it.
//!
//! Two directions, kept apart on purpose:
//!
//! * a **lender** is a machine that has agreed to run a model for this one,
//! * a **borrower** is a machine this one has agreed to run a model for.
//!
//! The distinction is the whole security model. A borrower can ask this
//! machine to think about a problem and can read what a scan found. It cannot
//! ask this machine to *do* anything -- there is no command in the protocol
//! that changes a borrower's machine, because there is no reason for a link to
//! carry one.
//!
//! Tokens are never written to this file. The file records a fingerprint (for
//! checking an incoming one) or nothing at all (for an outgoing one, which
//! lives in the operating system's credential store).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

const SERVICE: &str = "outlaw-repair-kit";

/// Which way a link points.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// This machine borrows a model from that one.
    Lender,
    /// That machine borrows a model from this one.
    Borrower,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Lender => "lends to us",
            Role::Borrower => "borrows from us",
        }
    }
}

/// One linked machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Peer {
    pub id: String,
    /// What to call it on screen. Chosen by that machine, so it is a label and
    /// never an identifier.
    pub name: String,
    pub role: Role,
    /// Where to reach it, for a lender. Empty for a borrower, which is only
    /// ever seen when it connects.
    #[serde(default)]
    pub address: String,
    #[serde(default)]
    pub platform: String,
    #[serde(default)]
    pub version: String,
    /// For a borrower: the hash of the token it will present. Never the token.
    #[serde(default)]
    pub token_fingerprint: String,
    pub linked_at: String,
}

/// Everything this machine has linked, on disk.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PeerBook {
    /// Identifies this machine to the ones it links with.
    #[serde(default)]
    pub machine_id: String,
    /// What this machine calls itself when introducing itself.
    #[serde(default)]
    pub machine_name: String,
    #[serde(default)]
    pub peers: Vec<Peer>,
}

impl PeerBook {
    /// Where the list of linked machines lives.
    pub fn default_path() -> Result<PathBuf> {
        Ok(ork_core::Config::default_path()?.with_file_name("peers.json"))
    }

    /// Load the list, creating an identity for this machine the first time.
    pub fn load(path: &Path) -> Result<Self> {
        let mut book: Self = if path.exists() {
            let text = std::fs::read_to_string(path)
                .with_context(|| format!("could not read {}", path.display()))?;
            serde_json::from_str(&text).with_context(|| {
                format!(
                    "{} is not readable as a list of linked machines",
                    path.display()
                )
            })?
        } else {
            Self::default()
        };

        // Written out straight away rather than on the next change. An
        // identity that only becomes real once something else happens is an
        // identity that reads differently every time you look at it.
        let fresh = book.machine_id.is_empty() || book.machine_name.is_empty();
        if book.machine_id.is_empty() {
            book.machine_id = crate::pair::new_machine_id();
        }
        if book.machine_name.is_empty() {
            book.machine_name = default_machine_name();
        }
        if fresh {
            book.save(path)?;
        }
        Ok(book)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("could not create {}", parent.display()))?;
        }
        let text = serde_json::to_string_pretty(self)?;
        std::fs::write(path, text)
            .with_context(|| format!("could not write {}", path.display()))?;
        Ok(())
    }

    pub fn find(&self, id_or_name: &str) -> Option<&Peer> {
        self.peers
            .iter()
            .find(|peer| peer.id == id_or_name)
            .or_else(|| {
                self.peers
                    .iter()
                    .find(|peer| peer.name.eq_ignore_ascii_case(id_or_name))
            })
    }

    /// Machines this one can borrow a model from.
    pub fn lenders(&self) -> impl Iterator<Item = &Peer> {
        self.peers.iter().filter(|peer| peer.role == Role::Lender)
    }

    /// Add or replace a link. Re-pairing an existing machine replaces it
    /// rather than leaving two entries that disagree about its token.
    pub fn upsert(&mut self, peer: Peer) {
        match self
            .peers
            .iter_mut()
            .find(|existing| existing.id == peer.id && existing.role == peer.role)
        {
            Some(existing) => *existing = peer,
            None => self.peers.push(peer),
        }
    }

    /// Remove a link, returning what was removed.
    ///
    /// The token goes with it: a link that has been cut should not leave a
    /// working credential behind.
    pub fn remove(&mut self, id_or_name: &str) -> Vec<Peer> {
        let matching: Vec<Peer> = self
            .peers
            .iter()
            .filter(|peer| peer.id == id_or_name || peer.name.eq_ignore_ascii_case(id_or_name))
            .cloned()
            .collect();
        self.peers.retain(|peer| {
            !matching
                .iter()
                .any(|gone| gone.id == peer.id && gone.role == peer.role)
        });
        for peer in &matching {
            let _ = forget_token(&peer.id);
        }
        matching
    }
}

fn default_machine_name() -> String {
    ork_core::platform::detect()
        .ok()
        .and_then(|platform| platform.host().ok())
        .map(|host| host.hostname)
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| "unnamed machine".to_string())
}

fn entry(peer_id: &str) -> Result<keyring::Entry> {
    keyring::Entry::new(SERVICE, &format!("link-token:{peer_id}"))
        .context("could not reach the credential store")
}

/// Remember the token for talking to a lender.
pub fn store_token(peer_id: &str, token: &str) -> Result<()> {
    entry(peer_id)?
        .set_password(token)
        .context("could not save the access token")
}

/// The token for talking to a lender, if there is one.
pub fn load_token(peer_id: &str) -> Option<String> {
    match entry(peer_id).and_then(|entry| entry.get_password().map_err(Into::into)) {
        Ok(token) if !token.trim().is_empty() => Some(token),
        _ => None,
    }
}

/// Forget a token. Missing is success -- the point is that it is gone.
pub fn forget_token(peer_id: &str) -> Result<()> {
    match entry(peer_id)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(error).context("could not remove the access token"),
    }
}

/// The time, formatted the way every other record in this tool is.
pub fn now() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peer(id: &str, role: Role) -> Peer {
        Peer {
            id: id.to_string(),
            name: format!("machine {id}"),
            role,
            address: "http://192.0.2.10:7341".to_string(),
            platform: "linux".to_string(),
            version: "0.4.0".to_string(),
            token_fingerprint: String::new(),
            linked_at: now(),
        }
    }

    fn temp_path(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ork-link-{}-{name}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        dir.join("peers.json")
    }

    #[test]
    fn a_new_book_gives_this_machine_an_identity_that_then_stays_put() {
        let path = temp_path("identity");
        let _ = std::fs::remove_file(&path);

        let first = PeerBook::load(&path).unwrap();
        assert!(!first.machine_id.is_empty());
        // Saved by loading it, so simply looking at the identity twice does
        // not show two different answers.
        assert!(path.exists(), "a new identity was not written out");

        let second = PeerBook::load(&path).unwrap();
        assert_eq!(
            first.machine_id, second.machine_id,
            "the identity changed between runs"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn re_pairing_replaces_a_link_rather_than_duplicating_it() {
        let mut book = PeerBook::default();
        book.upsert(peer("abc", Role::Lender));
        let mut again = peer("abc", Role::Lender);
        again.address = "http://192.0.2.99:7341".into();
        book.upsert(again);

        assert_eq!(book.peers.len(), 1);
        assert_eq!(book.peers[0].address, "http://192.0.2.99:7341");
    }

    #[test]
    fn the_same_machine_can_be_linked_both_ways_at_once() {
        // Two computers that each lend to the other is a perfectly ordinary
        // arrangement, and the two links are separate things.
        let mut book = PeerBook::default();
        book.upsert(peer("abc", Role::Lender));
        book.upsert(peer("abc", Role::Borrower));
        assert_eq!(book.peers.len(), 2);
        assert_eq!(book.lenders().count(), 1);
    }

    #[test]
    fn a_peer_can_be_found_by_name_or_by_id() {
        let mut book = PeerBook::default();
        book.upsert(peer("abc", Role::Lender));
        assert!(book.find("abc").is_some());
        assert!(book.find("MACHINE ABC").is_some());
        assert!(book.find("nothing").is_none());
    }

    #[test]
    fn removing_a_link_removes_both_directions_of_it() {
        let mut book = PeerBook::default();
        book.upsert(peer("abc", Role::Lender));
        book.upsert(peer("abc", Role::Borrower));
        let removed = book.remove("abc");
        assert_eq!(removed.len(), 2);
        assert!(book.peers.is_empty());
    }

    #[test]
    fn a_saved_book_never_contains_a_token() {
        let mut book = PeerBook::default();
        let mut borrower = peer("abc", Role::Borrower);
        borrower.token_fingerprint = crate::pair::token_fingerprint("the-real-token");
        book.upsert(borrower);

        let written = serde_json::to_string(&book).unwrap();
        assert!(
            !written.contains("the-real-token"),
            "the token was written to disk"
        );
        assert!(written.contains(&crate::pair::token_fingerprint("the-real-token")));
    }
}
