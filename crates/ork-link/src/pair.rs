//! The pairing handshake.
//!
//! Two machines end up sharing an access token without that token ever
//! crossing the network, and without either of them trusting the network they
//! are on.
//!
//! The person reads a pairing code off one screen and types it into the other.
//! Both sides then derive everything from it:
//!
//! ```text
//!   client -> host   nonce, and HMAC(code, "client" || nonce || client id)
//!   host   -> client HMAC(code, "host" || nonce)
//!   both derive      token = HMAC(code, "token" || nonce || client id)
//! ```
//!
//! Someone watching the exchange sees the nonce and two HMACs. Without the
//! pairing code they cannot derive the token, and the code is sixty bits that
//! stops being accepted after a few wrong guesses or a few minutes -- whichever
//! comes first.
//!
//! The host proof is not decoration: it is what stops a machine on the same
//! network from answering in the real host's place and being handed a session.
//!
//! No cryptography is invented here. This is HMAC-SHA256 used the way it is
//! meant to be used, with a distinct label per purpose so that a value from one
//! step can never be replayed as a value from another.

use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use crate::code::PairingCode;

type HmacSha256 = Hmac<Sha256>;

/// Labels keep the three derivations from ever colliding.
const LABEL_CLIENT: &[u8] = b"ork-pair-client-v1";
const LABEL_HOST: &[u8] = b"ork-pair-host-v1";
const LABEL_TOKEN: &[u8] = b"ork-pair-token-v1";

fn derive(code: &PairingCode, label: &[u8], parts: &[&[u8]]) -> String {
    let mut mac = HmacSha256::new_from_slice(code.secret())
        .expect("HMAC accepts a key of any length");
    mac.update(label);
    for part in parts {
        // Length-prefixed, so that ("ab", "c") and ("a", "bc") cannot produce
        // the same input to the MAC.
        mac.update(&(part.len() as u64).to_be_bytes());
        mac.update(part);
    }
    hex(&mac.finalize().into_bytes())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Compare two secrets without leaking, through timing, how much of the first
/// one was right.
pub fn secret_eq(left: &str, right: &str) -> bool {
    left.as_bytes().ct_eq(right.as_bytes()).into()
}

/// What the client sends to ask to be paired.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairRequest {
    /// Identifies this machine for as long as the link lasts.
    pub client_id: String,
    /// What to call it on the host's screen.
    pub client_name: String,
    /// Fresh for every attempt, so no exchange can be replayed.
    pub nonce: String,
    /// Proof that the client knows the pairing code.
    pub proof: String,
}

/// What the host sends back once it is satisfied.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairResponse {
    pub host_id: String,
    pub host_name: String,
    pub platform: String,
    pub version: String,
    /// Proof that the host knows the pairing code too, so the client can tell
    /// it is talking to the machine showing the code and not an impostor.
    pub proof: String,
}

/// Everything the client needs to start an exchange.
pub struct ClientHandshake {
    pub request: PairRequest,
    code: PairingCode,
}

impl ClientHandshake {
    /// Begin pairing with the machine showing `code`.
    pub fn start(code: PairingCode, client_id: &str, client_name: &str) -> Self {
        let nonce = hex(&random_bytes::<16>());
        let proof = derive(&code, LABEL_CLIENT, &[nonce.as_bytes(), client_id.as_bytes()]);
        Self {
            request: PairRequest {
                client_id: client_id.to_string(),
                client_name: client_name.to_string(),
                nonce,
                proof,
            },
            code,
        }
    }

    /// Check the host's reply and, if it holds up, produce the access token.
    ///
    /// A wrong proof means something answered that does not know the code.
    /// That is refused rather than reported, because the failure mode of
    /// getting this wrong is handing an access token to the wrong machine.
    pub fn finish(&self, response: &PairResponse) -> anyhow::Result<String> {
        let expected = derive(&self.code, LABEL_HOST, &[self.request.nonce.as_bytes()]);
        anyhow::ensure!(
            secret_eq(&expected, &response.proof),
            "that machine did not prove it knows the pairing code, so it was not trusted"
        );
        Ok(derive(
            &self.code,
            LABEL_TOKEN,
            &[self.request.nonce.as_bytes(), self.request.client_id.as_bytes()],
        ))
    }
}

/// The host's side: check the client's proof, and derive the same token.
///
/// Returns the proof to send back and the token to remember, or an error if
/// the client did not prove it knows the code.
pub fn accept(code: &PairingCode, request: &PairRequest) -> anyhow::Result<(String, String)> {
    let expected = derive(
        code,
        LABEL_CLIENT,
        &[request.nonce.as_bytes(), request.client_id.as_bytes()],
    );
    anyhow::ensure!(secret_eq(&expected, &request.proof), "that pairing code is not right");

    let proof = derive(code, LABEL_HOST, &[request.nonce.as_bytes()]);
    let token = derive(
        code,
        LABEL_TOKEN,
        &[request.nonce.as_bytes(), request.client_id.as_bytes()],
    );
    Ok((proof, token))
}

/// What the host keeps instead of the token itself.
///
/// A stolen peers file is then worth nothing: it holds hashes, and a hash
/// cannot be presented as a bearer token.
pub fn token_fingerprint(token: &str) -> String {
    hex(&Sha256::digest(token.as_bytes()))
}

/// Whether a presented token matches a stored fingerprint.
pub fn token_matches(presented: &str, fingerprint: &str) -> bool {
    secret_eq(&token_fingerprint(presented), fingerprint)
}

/// A random identifier for this machine, made once and then kept.
pub fn new_machine_id() -> String {
    hex(&random_bytes::<16>())
}

fn random_bytes<const N: usize>() -> [u8; N] {
    use rand::RngCore;
    let mut bytes = [0u8; N];
    rand::rng().fill_bytes(&mut bytes);
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host_reply(proof: String) -> PairResponse {
        PairResponse {
            host_id: "host".into(),
            host_name: "main pc".into(),
            platform: "windows".into(),
            version: "0.4.0".into(),
            proof,
        }
    }

    #[test]
    fn both_sides_end_up_with_the_same_token_without_sending_it() {
        let code = PairingCode::generate();
        let client = ClientHandshake::start(code.clone(), "client-id", "work rig");

        let (proof, host_token) = accept(&code, &client.request).unwrap();
        let client_token = client.finish(&host_reply(proof)).unwrap();

        assert_eq!(client_token, host_token);
        // The token appears in neither direction on the wire.
        let sent = serde_json::to_string(&client.request).unwrap();
        assert!(!sent.contains(&client_token), "the request carried the token");
    }

    #[test]
    fn the_wrong_code_is_refused() {
        let shown = PairingCode::generate();
        let typed = PairingCode::generate();
        let client = ClientHandshake::start(typed, "client-id", "work rig");
        assert!(accept(&shown, &client.request).is_err());
    }

    #[test]
    fn a_host_that_cannot_prove_itself_is_not_trusted() {
        // This is the check that stops another machine on the network from
        // answering in the host's place and being handed a session.
        let code = PairingCode::generate();
        let client = ClientHandshake::start(code, "client-id", "work rig");
        let impostor = derive(&PairingCode::generate(), LABEL_HOST, &[client.request.nonce.as_bytes()]);
        assert!(client.finish(&host_reply(impostor)).is_err());
    }

    #[test]
    fn a_recorded_exchange_cannot_be_replayed_for_a_second_machine() {
        let code = PairingCode::generate();
        let first = ClientHandshake::start(code.clone(), "client-a", "work rig");
        let (_, first_token) = accept(&code, &first.request).unwrap();

        // Same nonce and proof, different machine claiming them.
        let mut stolen = first.request.clone();
        stolen.client_id = "client-b".into();
        assert!(accept(&code, &stolen).is_err(), "the proof was accepted for another machine");

        let second = ClientHandshake::start(code.clone(), "client-b", "other");
        let (_, second_token) = accept(&code, &second.request).unwrap();
        assert_ne!(first_token, second_token, "two machines share one token");
    }

    #[test]
    fn each_pairing_produces_a_different_token() {
        let code = PairingCode::generate();
        let one = ClientHandshake::start(code.clone(), "same-id", "rig");
        let two = ClientHandshake::start(code.clone(), "same-id", "rig");
        let (_, first) = accept(&code, &one.request).unwrap();
        let (_, second) = accept(&code, &two.request).unwrap();
        assert_ne!(first, second, "the nonce is not doing its job");
    }

    #[test]
    fn the_three_derivations_never_collide() {
        // Without distinct labels, a value proving one thing could be replayed
        // as a value proving another.
        let code = PairingCode::generate();
        let client = ClientHandshake::start(code.clone(), "id", "name");
        let (host_proof, token) = accept(&code, &client.request).unwrap();
        assert_ne!(client.request.proof, host_proof);
        assert_ne!(client.request.proof, token);
        assert_ne!(host_proof, token);
    }

    #[test]
    fn the_host_stores_a_fingerprint_that_cannot_be_presented_as_a_token() {
        let token = "abc123";
        let fingerprint = token_fingerprint(token);
        assert_ne!(fingerprint, token);
        assert!(token_matches(token, &fingerprint));
        assert!(!token_matches("abc124", &fingerprint));
        // Presenting the stored value itself must not open the door.
        assert!(!token_matches(&fingerprint, &fingerprint));
    }

    #[test]
    fn machine_ids_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for _ in 0..200 {
            assert!(seen.insert(new_machine_id()));
        }
    }
}
