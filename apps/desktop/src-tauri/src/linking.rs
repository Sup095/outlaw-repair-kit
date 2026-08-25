//! Linking machines, from the window.
//!
//! Exactly the same operations as `outlaw link`, because they are the same
//! calls into the same crate. Pairing from a window is the case most people
//! will actually use, so it should not be the one that sends them to a
//! terminal.

use std::sync::{Arc, Mutex};

use ork_link::client::{self, LinkClient};
use ork_link::peer::PeerBook;
use ork_link::server::{HostState, serve};
use ork_link::{DEFAULT_PORT, PairingCode, discovery};
use tauri::State;

use crate::commands::{AppState, CmdResult, fail};

/// A lending session that is currently running.
pub struct Hosting {
    state: Arc<HostState>,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    /// Stops answering discovery. Separate from `stop` because they are two
    /// listeners, and a machine that has stopped lending must also stop
    /// telling the network that it lends.
    stop_discovery: Option<tokio::sync::oneshot::Sender<()>>,
    port: u16,
    /// Kept so the screen can still show it after a refresh. It is already on
    /// this machine's display; remembering it changes nothing about who can
    /// see it.
    code: String,
}

#[derive(Default)]
pub struct LinkState {
    hosting: Mutex<Option<Hosting>>,
}

fn book_path() -> anyhow::Result<std::path::PathBuf> {
    PeerBook::default_path()
}

fn load_book() -> anyhow::Result<PeerBook> {
    PeerBook::load(&book_path()?)
}

/// What this machine is linked to.
#[tauri::command]
pub fn link_status(state: State<'_, AppState>) -> CmdResult<serde_json::Value> {
    let book = load_book().map_err(fail)?;
    let hosting = state
        .link
        .hosting
        .lock()
        .map_err(|_| "the link lock was poisoned".to_string())?;

    Ok(serde_json::json!({
        "machine_id": book.machine_id,
        "machine_name": book.machine_name,
        "peers": book.peers,
        "hosting": hosting.as_ref().map(|session| serde_json::json!({
            "port": session.port,
            "pairing_open": session.state.pairing_open(),
            "pairing_code": session.code,
        })),
    }))
}

/// Start lending this machine's model, and show a pairing code.
#[tauri::command]
pub async fn link_host_start(
    state: State<'_, AppState>,
    port: Option<u16>,
    model_url: Option<String>,
) -> CmdResult<String> {
    let port = port.unwrap_or(DEFAULT_PORT);
    {
        let running = state
            .link
            .hosting
            .lock()
            .map_err(|_| "the link lock was poisoned".to_string())?;
        if running.is_some() {
            return Err("this machine is already lending its model".to_string());
        }
    }

    let path = book_path().map_err(fail)?;
    let book = PeerBook::load(&path).map_err(fail)?;
    let config =
        ork_core::Config::load_or_default(&ork_core::Config::default_path().map_err(fail)?)
            .map_err(fail)?;
    let upstream = model_url
        .or_else(|| config.ai.local.urls.first().cloned())
        .ok_or_else(|| {
            "no local model address is set -- add one on the Settings screen".to_string()
        })?;

    let machine_id = book.machine_id.clone();
    let machine_name = book.machine_name.clone();
    let host = Arc::new(HostState::new(book, path, upstream));
    let code = host.open_pairing();

    let (stop, stopped) = tokio::sync::oneshot::channel();
    let serving = host.clone();
    tokio::spawn(async move {
        let address = std::net::SocketAddr::from(([0, 0, 0, 0], port));
        if let Err(error) = serve(serving, address, async {
            let _ = stopped.await;
        })
        .await
        {
            tracing::error!(%error, "the link service stopped");
        }
    });

    // Answering on the local network is what saves the other machine's owner
    // typing an address.
    let (stop_discovery, discovery_stopped) = tokio::sync::oneshot::channel();
    let announcing = host.clone();
    tokio::spawn(async move {
        let describe = move || discovery::Discovered {
            machine_id: machine_id.clone(),
            name: machine_name.clone(),
            platform: ork_core::platform::detect()
                .map(|platform| platform.kind().to_string())
                .unwrap_or_default(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            pairing_open: announcing.pairing_open(),
            address: String::new(),
        };
        let _ = discovery::respond(port, describe, async {
            let _ = discovery_stopped.await;
        })
        .await;
    });

    *state
        .link
        .hosting
        .lock()
        .map_err(|_| "the link lock was poisoned".to_string())? = Some(Hosting {
        state: host,
        stop: Some(stop),
        stop_discovery: Some(stop_discovery),
        port,
        code: code.display(),
    });

    Ok(code.display())
}

/// Stop lending. Always available while a session is running.
#[tauri::command]
pub fn link_host_stop(state: State<'_, AppState>) -> CmdResult<bool> {
    let mut slot = state
        .link
        .hosting
        .lock()
        .map_err(|_| "the link lock was poisoned".to_string())?;
    match slot.take() {
        Some(mut session) => {
            session.state.close_pairing();
            if let Some(stop) = session.stop.take() {
                let _ = stop.send(());
            }
            if let Some(stop) = session.stop_discovery.take() {
                let _ = stop.send(());
            }
            Ok(true)
        }
        None => Ok(false),
    }
}

/// Who on this network is lending a model.
#[tauri::command]
pub async fn link_find(port: Option<u16>) -> CmdResult<Vec<discovery::Discovered>> {
    discovery::search(port.unwrap_or(DEFAULT_PORT))
        .await
        .map_err(fail)
}

/// Pair with a machine that is showing a code.
#[tauri::command]
pub async fn link_join(code: String, address: String) -> CmdResult<serde_json::Value> {
    let code = PairingCode::parse(&code).map_err(fail)?;
    let path = book_path().map_err(fail)?;
    let mut book = PeerBook::load(&path).map_err(fail)?;

    let peer = client::join(&mut book, &address, code)
        .await
        .map_err(fail)?;
    book.save(&path).map_err(fail)?;
    Ok(serde_json::json!(peer))
}

/// Cut a link, and forget its token with it.
#[tauri::command]
pub fn link_remove(name: String) -> CmdResult<usize> {
    let path = book_path().map_err(fail)?;
    let mut book = PeerBook::load(&path).map_err(fail)?;
    let removed = book.remove(&name);
    if removed.is_empty() {
        return Err(format!("nothing here is linked as `{name}`"));
    }
    book.save(&path).map_err(fail)?;
    Ok(removed.len())
}

/// What is wrong with a linked machine. Read-only: there is no route that
/// changes the machine at the other end.
#[tauri::command]
pub async fn link_view(name: Option<String>) -> CmdResult<serde_json::Value> {
    let book = load_book().map_err(fail)?;
    let peer = match &name {
        Some(name) => book
            .find(name)
            .ok_or_else(|| format!("nothing here is linked as `{name}`"))?,
        None => book
            .lenders()
            .next()
            .ok_or_else(|| "this machine is not linked to anything".to_string())?,
    };
    LinkClient::for_peer(peer)
        .map_err(fail)?
        .status()
        .await
        .map_err(fail)
}

/// Ask one linked machine whether it is still answering.
#[tauri::command]
pub async fn link_check(name: String) -> CmdResult<serde_json::Value> {
    let book = load_book().map_err(fail)?;
    let peer = book
        .find(&name)
        .ok_or_else(|| format!("nothing here is linked as `{name}`"))?;
    LinkClient::for_peer(peer)
        .map_err(fail)?
        .hello()
        .await
        .map_err(fail)
}
