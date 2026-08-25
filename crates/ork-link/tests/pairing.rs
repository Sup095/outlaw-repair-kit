//! End-to-end pairing, over real HTTP.
//!
//! The unit tests prove the handshake is sound in isolation. These prove the
//! service actually wires it up: that a wrong code gets nowhere, that a token
//! from one pairing is accepted afterwards, and that guessing runs out.

use std::net::SocketAddr;
use std::sync::Arc;

use ork_link::client;
use ork_link::pair::{self, ClientHandshake, PairRequest, PairResponse};
use ork_link::peer::{PeerBook, Role};
use ork_link::server::{HostState, router};
use ork_link::{MAX_PAIRING_ATTEMPTS, PairingCode};

/// Start a host on a port the operating system picks, so tests never collide.
///
/// Each host gets its own identity too. Tokens live in the real credential
/// store, keyed by the host's id, so two tests sharing an id would overwrite
/// each other's credential and then fail for a reason that has nothing to do
/// with what they were testing.
async fn start_host(machine_id: &str) -> (Arc<HostState>, String, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("temp dir");
    let book_path = dir.path().join("peers.json");

    let book = PeerBook {
        machine_id: machine_id.to_string(),
        machine_name: "main pc".to_string(),
        ..Default::default()
    };

    // No model is running in a test; the pairing routes never touch it.
    let state = Arc::new(HostState::new(
        book,
        book_path,
        "http://127.0.0.1:1/v1".to_string(),
    ));

    let listener = tokio::net::TcpListener::bind::<SocketAddr>(([127, 0, 0, 1], 0).into())
        .await
        .expect("bind");
    let address = format!("http://{}", listener.local_addr().unwrap());

    let serving = router(state.clone());
    tokio::spawn(async move {
        let _ = axum::serve(listener, serving).await;
    });

    (state, address, dir)
}

async fn attempt(address: &str, request: &PairRequest) -> reqwest::Response {
    reqwest::Client::new()
        .post(format!("{address}/ork/v1/pair"))
        .json(request)
        .send()
        .await
        .expect("the host did not answer")
}

#[tokio::test(flavor = "multi_thread")]
async fn a_machine_that_types_the_code_correctly_is_linked() {
    let (state, address, _dir) = start_host("host-a-0").await;
    let code = state.open_pairing();

    let mut book = PeerBook {
        machine_id: "client-machine".into(),
        machine_name: "work rig".into(),
        ..Default::default()
    };
    let peer = client::join(&mut book, &address, code)
        .await
        .expect("pairing failed");

    assert_eq!(peer.name, "main pc");
    assert_eq!(peer.role, Role::Lender);
    assert_eq!(book.lenders().count(), 1);

    // And the host now knows about the machine that joined.
    assert!(
        !state.pairing_open(),
        "the code stayed open after being used"
    );
    let _ = ork_link::peer::forget_token(&peer.id);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_token_it_ends_up_with_actually_opens_the_door() {
    let (state, address, _dir) = start_host("host-the-1").await;
    let code = state.open_pairing();

    let mut book = PeerBook {
        machine_id: "client-machine".into(),
        machine_name: "work rig".into(),
        ..Default::default()
    };
    let peer = client::join(&mut book, &address, code)
        .await
        .expect("pairing failed");

    let link = client::LinkClient::for_peer(&peer).expect("no token was stored");
    let hello = link
        .hello()
        .await
        .expect("the host refused a token it had just issued");
    assert_eq!(hello["you_are"], "work rig");

    let _ = ork_link::peer::forget_token(&peer.id);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_made_up_token_is_refused() {
    let (state, address, _dir) = start_host("host-a-2").await;
    state.open_pairing();

    let refused = reqwest::Client::new()
        .get(format!("{address}/ork/v1/hello"))
        .bearer_auth("not-a-real-token")
        .send()
        .await
        .expect("no answer");
    assert_eq!(refused.status(), reqwest::StatusCode::UNAUTHORIZED);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_wrong_code_gets_nowhere() {
    let (state, address, _dir) = start_host("host-the-3").await;
    state.open_pairing();

    let mut book = PeerBook {
        machine_id: "client".into(),
        machine_name: "rig".into(),
        ..Default::default()
    };
    let wrong = PairingCode::generate();
    let result = client::join(&mut book, &address, wrong).await;

    assert!(result.is_err(), "a wrong code was accepted");
    assert!(book.peers.is_empty(), "a failed pairing left a link behind");
}

#[tokio::test(flavor = "multi_thread")]
async fn guessing_runs_out() {
    // A twelve-character code is only safe because guesses are rationed.
    let (state, address, _dir) = start_host("host-guessing-4").await;
    let real = state.open_pairing();

    for _ in 0..MAX_PAIRING_ATTEMPTS {
        let guess = ClientHandshake::start(PairingCode::generate(), "guesser", "guesser");
        let response = attempt(&address, &guess.request).await;
        assert_eq!(response.status(), reqwest::StatusCode::UNAUTHORIZED);
    }

    assert!(!state.pairing_open(), "the host kept accepting guesses");

    // Even the right code is now too late: the window closed behind it.
    let honest = ClientHandshake::start(real, "honest", "honest");
    let response = attempt(&address, &honest.request).await;
    assert_eq!(response.status(), reqwest::StatusCode::FORBIDDEN);
}

#[tokio::test(flavor = "multi_thread")]
async fn nothing_can_be_paired_when_no_code_is_showing() {
    let (_state, address, _dir) = start_host("host-nothing-5").await;
    let handshake = ClientHandshake::start(PairingCode::generate(), "client", "rig");
    let response = attempt(&address, &handshake.request).await;
    assert_eq!(response.status(), reqwest::StatusCode::FORBIDDEN);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_impostor_host_cannot_hand_out_a_session() {
    // The client checks the host's proof precisely so that a machine which
    // answered first, but does not know the code, gets nothing.
    let code = PairingCode::generate();
    let handshake = ClientHandshake::start(code, "client", "rig");
    let impostor = PairResponse {
        host_id: "impostor".into(),
        host_name: "definitely the main pc".into(),
        platform: "linux".into(),
        version: "0.4.0".into(),
        proof: pair::token_fingerprint("anything at all"),
    };
    assert!(handshake.finish(&impostor).is_err());
}
