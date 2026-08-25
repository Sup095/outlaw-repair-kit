//! The lending side: a small HTTP service that answers linked machines.
//!
//! Four things are on offer and no more:
//!
//! | Route | What it does |
//! | --- | --- |
//! | `POST /ork/v1/pair` | Accepts a pairing attempt, but only while a code is showing |
//! | `GET /ork/v1/hello` | Says who this machine is |
//! | `GET /ork/v1/models` | Lists the models it can run |
//! | `POST /ork/v1/chat/completions` | Runs one |
//! | `GET /ork/v1/status` | Says what is wrong with this machine |
//!
//! There is no route that changes this machine. That is the security model:
//! not a permission that could be granted later, but a capability that was
//! never built. Someone who steals a token can ask this computer to think, and
//! that is the whole of what they can do with it.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use anyhow::{Context, Result};
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Serialize;
use serde_json::Value;

use crate::pair::{self, PairRequest, PairResponse};
use crate::peer::{Peer, PeerBook, Role, now};
use crate::{MAX_PAIRING_ATTEMPTS, PAIRING_WINDOW, PairingCode};

/// Something worth telling the person sitting at the lending machine.
///
/// Without this the host is silent: you read a code out to someone in the next
/// room and get no sign of whether it worked. Emitted rather than printed,
/// because a library that writes to a terminal is a library with an opinion
/// about who is running it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "event", rename_all = "kebab-case")]
pub enum HostEvent {
    /// A machine paired successfully.
    Linked { name: String },
    /// Somebody typed a code that was not right.
    WrongCode { attempts_left: usize },
    /// A linked machine asked this one to run its model.
    ModelRequested { name: String },
}

/// A pairing code that is currently being shown to somebody.
struct ActivePairing {
    code: PairingCode,
    opened: Instant,
    wrong_attempts: usize,
}

impl ActivePairing {
    fn usable(&self) -> bool {
        self.opened.elapsed() < PAIRING_WINDOW && self.wrong_attempts < MAX_PAIRING_ATTEMPTS
    }
}

/// Everything the service needs while it runs.
pub struct HostState {
    book: Mutex<PeerBook>,
    book_path: std::path::PathBuf,
    pairing: Mutex<Option<ActivePairing>>,
    /// The OpenAI-compatible model this machine will run on a peer's behalf.
    upstream: String,
    http: reqwest::Client,
    events: Mutex<Option<tokio::sync::mpsc::UnboundedSender<HostEvent>>>,
}

impl HostState {
    pub fn new(book: PeerBook, book_path: std::path::PathBuf, upstream: String) -> Self {
        Self {
            book: Mutex::new(book),
            book_path,
            pairing: Mutex::new(None),
            upstream: upstream.trim_end_matches('/').to_string(),
            http: reqwest::Client::new(),
            events: Mutex::new(None),
        }
    }

    /// Report what happens to whoever is watching.
    pub fn events(&self) -> tokio::sync::mpsc::UnboundedReceiver<HostEvent> {
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        *self.events.lock().expect("event lock") = Some(sender);
        receiver
    }

    fn announce(&self, event: HostEvent) {
        // A front-end that has stopped listening is not a reason to fail a
        // request that has otherwise gone perfectly well.
        if let Some(sender) = self.events.lock().expect("event lock").as_ref() {
            let _ = sender.send(event);
        }
    }

    /// Start showing a pairing code, replacing any code already showing.
    pub fn open_pairing(&self) -> PairingCode {
        let code = PairingCode::generate();
        *self.pairing.lock().expect("pairing lock") = Some(ActivePairing {
            code: code.clone(),
            opened: Instant::now(),
            wrong_attempts: 0,
        });
        code
    }

    /// Stop accepting new machines.
    pub fn close_pairing(&self) {
        *self.pairing.lock().expect("pairing lock") = None;
    }

    /// Whether a code is currently being shown and still usable.
    pub fn pairing_open(&self) -> bool {
        self.pairing
            .lock()
            .expect("pairing lock")
            .as_ref()
            .is_some_and(ActivePairing::usable)
    }

    fn machine(&self) -> (String, String) {
        let book = self.book.lock().expect("peer book lock");
        (book.machine_id.clone(), book.machine_name.clone())
    }

    /// Whether this token belongs to a machine that has been linked.
    fn authorised(&self, token: &str) -> Option<Peer> {
        let book = self.book.lock().expect("peer book lock");
        book.peers
            .iter()
            .filter(|peer| peer.role == Role::Borrower && !peer.token_fingerprint.is_empty())
            .find(|peer| pair::token_matches(token, &peer.token_fingerprint))
            .cloned()
    }
}

fn bearer(headers: &HeaderMap) -> Option<String> {
    let value = headers
        .get(axum::http::header::AUTHORIZATION)?
        .to_str()
        .ok()?;
    value
        .strip_prefix("Bearer ")
        .map(|token| token.trim().to_string())
}

#[derive(Serialize)]
struct ApiError {
    error: String,
}

fn refuse(status: StatusCode, message: &str) -> axum::response::Response {
    (
        status,
        Json(ApiError {
            error: message.to_string(),
        }),
    )
        .into_response()
}

async fn handle_pair(
    State(state): State<Arc<HostState>>,
    Json(request): Json<PairRequest>,
) -> axum::response::Response {
    let (proof, token) = {
        let mut slot = state.pairing.lock().expect("pairing lock");
        let Some(active) = slot.as_mut() else {
            // Not "wrong code" -- there is no code. Saying so is not a leak,
            // and it saves somebody a long hunt for a typo.
            return refuse(
                StatusCode::FORBIDDEN,
                "that machine is not showing a pairing code",
            );
        };
        if !active.usable() {
            *slot = None;
            return refuse(StatusCode::FORBIDDEN, "that pairing code has expired");
        }

        match pair::accept(&active.code, &request) {
            Ok(result) => result,
            Err(_) => {
                // Guessing has to be expensive, or a short code is not safe.
                active.wrong_attempts += 1;
                let left = MAX_PAIRING_ATTEMPTS.saturating_sub(active.wrong_attempts);
                if left == 0 {
                    *slot = None;
                    tracing::warn!("pairing closed after too many wrong codes");
                }
                state.announce(HostEvent::WrongCode {
                    attempts_left: left,
                });
                return refuse(StatusCode::UNAUTHORIZED, "that pairing code is not right");
            }
        }
    };

    let (host_id, host_name) = state.machine();
    let platform = ork_core::platform::detect()
        .map(|platform| platform.kind().to_string())
        .unwrap_or_else(|_| "unknown".to_string());

    {
        let mut book = state.book.lock().expect("peer book lock");
        book.upsert(Peer {
            id: request.client_id.clone(),
            name: request.client_name.clone(),
            role: Role::Borrower,
            address: String::new(),
            platform: String::new(),
            version: String::new(),
            token_fingerprint: pair::token_fingerprint(&token),
            linked_at: now(),
        });
        if let Err(error) = book.save(&state.book_path) {
            tracing::error!(%error, "could not record the new link");
            return refuse(
                StatusCode::INTERNAL_SERVER_ERROR,
                "could not record the link",
            );
        }
    }

    // One code, one machine. Leaving it open would turn a code somebody read
    // out loud into a standing invitation.
    state.close_pairing();
    tracing::info!(peer = %request.client_name, "linked");
    state.announce(HostEvent::Linked {
        name: request.client_name.clone(),
    });

    Json(PairResponse {
        host_id,
        host_name,
        platform,
        version: env!("CARGO_PKG_VERSION").to_string(),
        proof,
    })
    .into_response()
}

async fn handle_hello(
    State(state): State<Arc<HostState>>,
    headers: HeaderMap,
) -> axum::response::Response {
    let Some(peer) = bearer(&headers).and_then(|token| state.authorised(&token)) else {
        return refuse(StatusCode::UNAUTHORIZED, "not linked to this machine");
    };
    let (host_id, host_name) = state.machine();
    Json(serde_json::json!({
        "host_id": host_id,
        "host_name": host_name,
        "version": env!("CARGO_PKG_VERSION"),
        "you_are": peer.name,
    }))
    .into_response()
}

async fn handle_models(
    State(state): State<Arc<HostState>>,
    headers: HeaderMap,
) -> axum::response::Response {
    if bearer(&headers)
        .and_then(|token| state.authorised(&token))
        .is_none()
    {
        return refuse(StatusCode::UNAUTHORIZED, "not linked to this machine");
    }
    forward(&state, "models", None).await
}

async fn handle_completions(
    State(state): State<Arc<HostState>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> axum::response::Response {
    let Some(peer) = bearer(&headers).and_then(|token| state.authorised(&token)) else {
        return refuse(StatusCode::UNAUTHORIZED, "not linked to this machine");
    };
    tracing::info!(peer = %peer.name, "running a model for a linked machine");
    state.announce(HostEvent::ModelRequested {
        name: peer.name.clone(),
    });
    forward(&state, "chat/completions", Some(body)).await
}

/// Pass a request on to the model running on this machine.
///
/// Nothing is inspected or rewritten on the way through. This is a relay for
/// the machine's model, not a second opinion about what should be asked of it.
async fn forward(state: &HostState, path: &str, body: Option<Value>) -> axum::response::Response {
    let url = format!("{}/{path}", state.upstream);
    let request = match body {
        Some(body) => state.http.post(&url).json(&body),
        None => state.http.get(&url),
    };

    match request.send().await {
        Ok(response) => {
            let status =
                StatusCode::from_u16(response.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
            match response.json::<Value>().await {
                Ok(payload) => (status, Json(payload)).into_response(),
                Err(error) => refuse(
                    StatusCode::BAD_GATEWAY,
                    &format!("the model answered with something unreadable: {error}"),
                ),
            }
        }
        // The borrower cannot fix this and should not be left guessing: the
        // lender's model is simply not running.
        Err(error) => refuse(
            StatusCode::BAD_GATEWAY,
            &format!("no model is running on that machine ({error})"),
        ),
    }
}

/// What is wrong with this machine, for a linked machine to read.
///
/// Read-only, and deliberately so. This is the "my other computer is across
/// town and will not boot properly" case: you can see what it found, and then
/// you go and deal with it. Nothing here offers to deal with it for you.
async fn handle_status(
    State(state): State<Arc<HostState>>,
    headers: HeaderMap,
) -> axum::response::Response {
    if bearer(&headers)
        .and_then(|token| state.authorised(&token))
        .is_none()
    {
        return refuse(StatusCode::UNAUTHORIZED, "not linked to this machine");
    }

    let (_, host_name) = state.machine();
    let host = ork_core::platform::detect()
        .ok()
        .and_then(|platform| platform.host().ok());

    // A missing queue is a fact about that machine worth reporting, not an
    // error worth refusing the whole request over.
    let (queue, queue_error) = match queue_summary() {
        Ok(items) => (items, None),
        Err(error) => (Vec::new(), Some(format!("{error:#}"))),
    };

    Json(serde_json::json!({
        "host_name": host_name,
        "host": host,
        "waiting": queue,
        "queue_error": queue_error,
        "version": env!("CARGO_PKG_VERSION"),
        "at": now(),
    }))
    .into_response()
}

fn queue_summary() -> anyhow::Result<Vec<Value>> {
    let path = ork_core::Config::default_path()?.with_file_name("state.db");
    let store = ork_fix::store::FixStore::open(&path)?;
    Ok(store
        .pending()?
        .into_iter()
        .map(|item| {
            serde_json::json!({
                "title": item.title,
                "subject": item.subject,
                "severity": item.severity,
                "state": item.state.as_str(),
                "attempts": item.attempts,
                "detail": item.finding.detail,
            })
        })
        .collect())
}

/// Build the service.
pub fn router(state: Arc<HostState>) -> Router {
    Router::new()
        .route("/ork/v1/pair", post(handle_pair))
        .route("/ork/v1/hello", get(handle_hello))
        .route("/ork/v1/models", get(handle_models))
        .route("/ork/v1/status", get(handle_status))
        .route("/ork/v1/chat/completions", post(handle_completions))
        .with_state(state)
}

/// Listen until the future returned by `shutdown` completes.
pub async fn serve(
    state: Arc<HostState>,
    address: SocketAddr,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> Result<()> {
    let listener = tokio::net::TcpListener::bind(address)
        .await
        .with_context(|| format!("could not listen on {address}"))?;
    tracing::info!(%address, "lending a model to linked machines");
    axum::serve(listener, router(state))
        .with_graceful_shutdown(shutdown)
        .await
        .context("the link service stopped unexpectedly")
}
