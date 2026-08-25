//! Everything the window can ask the tool to do.
//!
//! Each command is a call into the shared crates and nothing more. Errors are
//! turned into strings at this boundary because that is what crosses to the
//! front-end, but the message is the full error chain, not a shrug.

use std::sync::Mutex;

use ork_ai::analysis::Analyst;
use ork_ai::router::{ModelRouter, ModelTier, advise_for_vram};
use ork_ai::runbook::RunbookLibrary;
use ork_ai::secrets::{self, SecretKind};
use ork_core::scan::{ScanEvent, ScanReport};
use ork_core::{Config, ScanTier, Scanner};
use ork_fix::store::FixStore;
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tokio_util::sync::CancellationToken;

/// The error type crossing to the front-end: a readable message.
type CmdResult<T> = Result<T, String>;

fn fail(error: impl std::fmt::Display) -> String {
    format!("{error:#}")
}

/// What a running scan needs so the user can stop it.
#[derive(Default)]
pub struct AppState {
    scan: Mutex<Option<CancellationToken>>,
}

fn state_dir() -> anyhow::Result<std::path::PathBuf> {
    let path = Config::default_path()?;
    Ok(path.parent().map(|dir| dir.to_path_buf()).unwrap_or_default())
}

fn runbook_dir() -> Option<std::path::PathBuf> {
    state_dir().ok().map(|dir| dir.join("runbooks"))
}

fn open_store() -> anyhow::Result<FixStore> {
    FixStore::open(&state_dir()?.join("state.db"))
}

fn load_config() -> anyhow::Result<Config> {
    Config::load_or_default(&Config::default_path()?)
}

/// Run the start-up sequence, emitting `boot://event` as each step finishes.
#[tauri::command]
pub async fn boot(app: AppHandle) -> CmdResult<ork_boot::BootReport> {
    Ok(ork_boot::boot(|event| {
        // A dropped event only costs a line on the boot screen, so a failure
        // to emit is not worth abandoning start-up over.
        if let Err(error) = app.emit("boot://event", &event) {
            tracing::debug!(%error, "could not deliver a start-up event");
        }
    })
    .await)
}

#[tauri::command]
pub fn host_info() -> CmdResult<ork_core::HostInfo> {
    ork_core::platform::detect()
        .and_then(|platform| platform.host())
        .map_err(fail)
}

/// The checks this build knows how to run, and what each one needs.
#[tauri::command]
pub fn probe_list() -> CmdResult<serde_json::Value> {
    let metas = ork_core::probes::all_meta();
    Ok(serde_json::json!(
        metas
            .iter()
            .map(|meta| serde_json::json!({
                "id": meta.id,
                "name": meta.name,
                "description": meta.description,
                "tier": meta.min_tier.as_str(),
                "platforms": meta.platforms.iter().map(|p| p.to_string()).collect::<Vec<_>>(),
                "requires_elevation": meta.requires_elevation,
                "required_tools": meta.requires_tools,
            }))
            .collect::<Vec<_>>()
    ))
}

/// Run a scan, emitting `scan://event` as each check finishes.
///
/// No tier has a time limit. The only thing that ends a scan early is
/// [`cancel_scan`], which is the user's decision.
#[tauri::command]
pub async fn start_scan(app: AppHandle, state: State<'_, AppState>, tier: String) -> CmdResult<ScanReport> {
    let tier: ScanTier = tier.parse().map_err(fail)?;

    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<ScanEvent>();
    let scanner = Scanner::new().map_err(fail)?.with_events(sender);
    let cancel = scanner.cancel_token();

    {
        let mut slot = state.scan.lock().map_err(|_| "the scan lock was poisoned".to_string())?;
        if slot.is_some() {
            return Err("a scan is already running".to_string());
        }
        *slot = Some(cancel);
    }

    let forwarder = tokio::spawn(async move {
        while let Some(event) = receiver.recv().await {
            if let Err(error) = app.emit("scan://event", &event) {
                tracing::debug!(%error, "could not deliver a scan event");
            }
        }
    });

    let outcome = scanner.run(tier).await;

    // Cleared however the scan ended, or a cancelled scan would leave the
    // next one convinced it is still running.
    if let Ok(mut slot) = state.scan.lock() {
        *slot = None;
    }
    forwarder.abort();

    let report = outcome.map_err(fail)?;

    // Anything worth a person's attention goes on the triage queue, so the
    // desktop and the command line are working from the same list.
    if let Ok(store) = open_store() {
        for finding in report.findings() {
            if finding.triage == ork_core::Triage::Queue {
                let _ = store.enqueue(finding);
            }
        }
    }

    Ok(report)
}

/// Stop the running scan. Always available, never automatic.
#[tauri::command]
pub fn cancel_scan(state: State<'_, AppState>) -> CmdResult<bool> {
    let mut slot = state.scan.lock().map_err(|_| "the scan lock was poisoned".to_string())?;
    match slot.take() {
        Some(cancel) => {
            cancel.cancel();
            Ok(true)
        }
        None => Ok(false),
    }
}

/// Explain a report: runbook matches first, a model only where none matched.
#[tauri::command]
pub async fn explain_report(report: ScanReport) -> CmdResult<serde_json::Value> {
    let config = load_config().map_err(fail)?;
    let routing = ModelRouter::new(config.ai.clone()).resolve().await;
    let library = RunbookLibrary::load(runbook_dir().as_deref()).map_err(fail)?;
    let platform = ork_core::platform::detect().map_err(fail)?.kind().to_string();

    let analysis = Analyst::new(library, platform).analyse(&report, &routing).await.map_err(fail)?;
    Ok(serde_json::json!({ "routing": routing.summary(), "analysis": analysis }))
}

/// Settings, as they currently stand, plus where they live.
#[tauri::command]
pub fn settings_load() -> CmdResult<serde_json::Value> {
    let path = Config::default_path().map_err(fail)?;
    let config = Config::load_or_default(&path).map_err(fail)?;
    Ok(serde_json::json!({
        "path": path.display().to_string(),
        "exists": path.exists(),
        "runbook_dir": runbook_dir().map(|dir| dir.display().to_string()),
        "config": config,
    }))
}

/// Save settings edited in the window.
///
/// This is the reason the settings screen exists: nobody should have to hand-
/// edit a file to point the tool at their own machine.
#[tauri::command]
pub fn settings_save(config: Config) -> CmdResult<String> {
    let path = Config::default_path().map_err(fail)?;
    config.save(&path).map_err(fail)?;
    Ok(path.display().to_string())
}

fn secret_kind(which: &str) -> Result<SecretKind, String> {
    match which {
        "cloud" => Ok(SecretKind::CloudApiKey),
        "remote" => Ok(SecretKind::RemoteEndpointToken),
        other => Err(format!("unknown credential `{other}`")),
    }
}

/// Whether each credential is stored -- never the value itself.
#[tauri::command]
pub fn secret_status() -> CmdResult<serde_json::Value> {
    Ok(serde_json::json!({
        "cloud": secrets::is_set(SecretKind::CloudApiKey),
        "remote": secrets::is_set(SecretKind::RemoteEndpointToken),
    }))
}

/// Store a credential in the operating system's credential store.
///
/// It never touches the settings file, and it is never sent back to the
/// window once saved.
#[tauri::command]
pub fn secret_set(which: String, value: String) -> CmdResult<()> {
    secrets::set(secret_kind(&which)?, value.trim()).map_err(fail)
}

#[tauri::command]
pub fn secret_clear(which: String) -> CmdResult<()> {
    secrets::delete(secret_kind(&which)?).map_err(fail)
}

/// Which model would handle this run, and how that was decided.
#[tauri::command]
pub async fn routing_status() -> CmdResult<serde_json::Value> {
    let config = load_config().map_err(fail)?;
    let routing = ModelRouter::new(config.ai.clone()).resolve().await;
    let gpus = ork_core::platform::detect().map_err(fail)?.gpus().map_err(fail)?;
    let advice = advise_for_vram(&gpus);
    let library = RunbookLibrary::load(runbook_dir().as_deref()).map_err(fail)?;

    Ok(serde_json::json!({
        "selected": routing.tier.map(ModelTier::as_str),
        "summary": routing.summary(),
        "attempts": routing.attempts.iter().map(|attempt| serde_json::json!({
            "tier": attempt.tier.as_str(),
            "outcome": attempt.outcome.explain(),
            "selected": attempt.outcome.is_selected(),
        })).collect::<Vec<_>>(),
        "gpus": gpus,
        "vram_recommendation": advice.recommendation,
        "runbook_entries": library.len(),
        "cloud_key_stored": secrets::is_set(SecretKind::CloudApiKey),
        "remote_token_stored": secrets::is_set(SecretKind::RemoteEndpointToken),
    }))
}

/// Problems waiting to be worked through.
#[tauri::command]
pub fn queue_list() -> CmdResult<serde_json::Value> {
    let store = open_store().map_err(fail)?;
    let items = store.pending().map_err(fail)?;
    Ok(serde_json::json!(items))
}

/// Everything the tool has checked, found, attempted, and changed.
#[derive(Serialize)]
pub struct AuditLine {
    at: String,
    kind: String,
    message: String,
}

#[tauri::command]
pub fn audit_list(limit: usize) -> CmdResult<Vec<AuditLine>> {
    let store = open_store().map_err(fail)?;
    let rows = store.audit_log(limit.clamp(1, 500)).map_err(fail)?;
    Ok(rows.into_iter().map(|(at, kind, message)| AuditLine { at, kind, message }).collect())
}
