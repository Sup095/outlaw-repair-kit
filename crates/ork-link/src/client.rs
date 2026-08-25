//! The borrowing side: talking to a machine that has agreed to lend a model.

use anyhow::{Context, Result};
use serde_json::Value;

use crate::pair::{ClientHandshake, PairResponse};
use crate::peer::{self, Peer, PeerBook, Role, now};
use crate::{DEFAULT_PORT, PairingCode};

/// A machine this one is linked to, and the token that proves it.
pub struct LinkClient {
    base: String,
    token: String,
    http: reqwest::Client,
}

impl LinkClient {
    /// Build a client for a peer that has already been linked.
    pub fn for_peer(peer: &Peer) -> Result<Self> {
        let token = peer::load_token(&peer.id).with_context(|| {
            format!(
                "no access token is stored for {} -- link it again with `outlaw link join`",
                peer.name
            )
        })?;
        Ok(Self {
            base: peer.address.trim_end_matches('/').to_string(),
            token,
            http: reqwest::Client::new(),
        })
    }

    /// The address of this peer's model, in the OpenAI-compatible shape the
    /// rest of the tool already speaks.
    ///
    /// This is what makes a linked machine and a hand-configured endpoint the
    /// same thing as far as the model router is concerned.
    pub fn openai_base_url(&self) -> String {
        format!("{}/ork/v1", self.base)
    }

    pub fn token(&self) -> &str {
        &self.token
    }

    /// What is wrong with the machine at the other end.
    ///
    /// Read-only. This answers "what is going on over there", which is the
    /// question you have when the other computer is somewhere you are not.
    /// Doing something about it is a job for that machine's own keyboard.
    pub async fn status(&self) -> Result<Value> {
        let response = self
            .http
            .get(format!("{}/ork/v1/status", self.base))
            .bearer_auth(&self.token)
            .timeout(std::time::Duration::from_secs(15))
            .send()
            .await
            .context("that machine did not answer")?;

        if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            anyhow::bail!("that machine no longer recognises this one -- link them again");
        }
        response
            .error_for_status()
            .context("that machine answered with an error")?
            .json()
            .await
            .context("that machine answered with something unreadable")
    }

    /// Ask the peer who it is. The cheapest way to find out whether a link
    /// still works.
    pub async fn hello(&self) -> Result<Value> {
        let response = self
            .http
            .get(format!("{}/ork/v1/hello", self.base))
            .bearer_auth(&self.token)
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .await
            .context("that machine did not answer")?;

        if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            anyhow::bail!("that machine no longer recognises this one -- link them again");
        }
        response
            .error_for_status()
            .context("that machine answered with an error")?
            .json()
            .await
            .context("that machine answered with something unreadable")
    }
}

/// Pair with a machine that is showing a code.
///
/// On success the link is recorded and the token is put in the operating
/// system's credential store. The token is derived, never received, so it does
/// not travel over the network at any point.
pub async fn join(book: &mut PeerBook, address: &str, code: PairingCode) -> Result<Peer> {
    let base = normalise_address(address);
    let handshake = ClientHandshake::start(code, &book.machine_id, &book.machine_name);

    let response = reqwest::Client::new()
        .post(format!("{base}/ork/v1/pair"))
        .json(&handshake.request)
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .with_context(|| format!("could not reach {base}"))?;

    if !response.status().is_success() {
        // The host's own words. It knows whether the code was wrong, expired,
        // or never shown, and the person typing needs to know which.
        let status = response.status();
        let detail: Value = response.json().await.unwrap_or(Value::Null);
        let message = detail
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("that machine refused the pairing");
        anyhow::bail!("{message} ({status})");
    }

    let reply: PairResponse = response.json().await.context("that machine answered with something unreadable")?;
    let token = handshake.finish(&reply)?;

    let peer = Peer {
        id: reply.host_id,
        name: reply.host_name,
        role: Role::Lender,
        address: base,
        platform: reply.platform,
        version: reply.version,
        token_fingerprint: String::new(),
        linked_at: now(),
    };
    peer::store_token(&peer.id, &token)?;
    book.upsert(peer.clone());
    Ok(peer)
}

/// Accept the shapes people actually type: a bare hostname, a host and port,
/// or a full address.
pub fn normalise_address(input: &str) -> String {
    let trimmed = input.trim().trim_end_matches('/');
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        return trimmed.to_string();
    }
    if trimmed.contains(':') && !trimmed.contains("::") {
        return format!("http://{trimmed}");
    }
    format!("http://{trimmed}:{DEFAULT_PORT}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_are_accepted_in_the_shapes_people_type_them() {
        assert_eq!(normalise_address("192.168.1.5"), "http://192.168.1.5:7341");
        assert_eq!(normalise_address("main-pc"), "http://main-pc:7341");
        assert_eq!(normalise_address("192.168.1.5:9000"), "http://192.168.1.5:9000");
        assert_eq!(normalise_address("http://192.168.1.5:7341/"), "http://192.168.1.5:7341");
        assert_eq!(normalise_address(" https://rig.example  "), "https://rig.example");
    }
}
