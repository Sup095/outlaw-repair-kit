//! Making a linked machine and a hand-typed endpoint the same thing.
//!
//! The model router already knows how to use "an OpenAI-compatible endpoint
//! somewhere else". A linked machine is exactly that, with the address and the
//! credential filled in by the pairing rather than by a person. So linking
//! adds no new tier and no new code path -- it fills in the one that already
//! existed.
//!
//! A remote endpoint written by hand always wins. Someone who typed an address
//! into their settings meant it.

use ork_core::config::{Config, EndpointConfig};

use crate::peer::{self, Peer, PeerBook};

/// The credential account a peer's token is stored under.
pub fn token_account(peer_id: &str) -> String {
    format!("link-token:{peer_id}")
}

/// Point the remote tier at a linked machine, if there is one to use and
/// nothing has been configured by hand.
///
/// Returns the machine now being used, or `None` if the settings were left
/// exactly as they were.
pub fn apply_to_config(book: &PeerBook, config: &mut Config) -> Option<Peer> {
    if config.ai.remote.endpoint.is_some() {
        return None;
    }

    // A link whose token has gone missing is not usable, and silently trying
    // it would produce a baffling failure later.
    let peer = book
        .lenders()
        .find(|peer| peer::load_token(&peer.id).is_some())?
        .clone();

    config.ai.remote.enabled = true;
    config.ai.remote.endpoint = Some(
        EndpointConfig::new(format!("{}/ork/v1", peer.address.trim_end_matches('/')), "")
            .with_token_ref(token_account(&peer.id)),
    );
    Some(peer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::peer::Role;

    fn lender(id: &str) -> Peer {
        Peer {
            id: id.into(),
            name: "main pc".into(),
            role: Role::Lender,
            address: "http://192.0.2.10:7341".into(),
            platform: "windows".into(),
            version: "0.4.0".into(),
            token_fingerprint: String::new(),
            linked_at: peer::now(),
        }
    }

    #[test]
    fn a_hand_written_endpoint_is_never_overwritten() {
        let mut book = PeerBook::default();
        book.upsert(lender("abc"));
        let mut config = Config::default();
        config.ai.remote.endpoint = Some(EndpointConfig::new("http://mine:1234/v1", "my-model"));

        assert!(apply_to_config(&book, &mut config).is_none());
        assert_eq!(
            config.ai.remote.endpoint.unwrap().url,
            "http://mine:1234/v1"
        );
    }

    #[test]
    fn a_link_with_no_stored_token_is_passed_over() {
        let mut book = PeerBook::default();
        book.upsert(lender("no-token-stored-for-this-id"));
        let mut config = Config::default();
        assert!(apply_to_config(&book, &mut config).is_none());
        assert!(config.ai.remote.endpoint.is_none());
    }

    #[test]
    fn a_linked_machine_fills_in_the_remote_tier() {
        let id = "routing-test-peer";
        peer::store_token(id, "a-token").unwrap();

        let mut book = PeerBook::default();
        book.upsert(lender(id));
        let mut config = Config::default();

        let used = apply_to_config(&book, &mut config).expect("the link was not used");
        assert_eq!(used.id, id);

        let endpoint = config.ai.remote.endpoint.unwrap();
        assert_eq!(endpoint.url, "http://192.0.2.10:7341/ork/v1");
        assert_eq!(
            endpoint.token_ref.as_deref(),
            Some("link-token:routing-test-peer")
        );
        assert!(config.ai.remote.enabled);

        let _ = peer::forget_token(id);
    }

    #[test]
    fn the_token_never_lands_in_the_settings_file() {
        let id = "routing-secrecy-peer";
        peer::store_token(id, "super-secret-token").unwrap();

        let mut book = PeerBook::default();
        book.upsert(lender(id));
        let mut config = Config::default();
        apply_to_config(&book, &mut config).unwrap();

        let written = config.to_toml().unwrap();
        assert!(
            !written.contains("super-secret-token"),
            "the settings file holds a token"
        );
        assert!(
            written.contains("link-token:"),
            "the settings file lost the credential's name"
        );

        let _ = peer::forget_token(id);
    }
}
