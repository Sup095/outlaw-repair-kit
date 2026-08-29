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
use ork_core::processes::Survey;
use ork_core::scan::{ScanEvent, ScanReport};
use ork_core::{Config, ScanTier, Scanner};
use ork_fix::store::FixStore;
use tauri::{AppHandle, Emitter, State};
use tokio_util::sync::CancellationToken;

/// The error type crossing to the front-end: a readable message.
pub type CmdResult<T> = Result<T, String>;

pub fn fail(error: impl std::fmt::Display) -> String {
    format!("{error:#}")
}

/// What a running scan needs so the user can stop it, what a lending session
/// needs so it can be stopped too, and what a fix run needs so its questions
/// can be answered.
#[derive(Default)]
pub struct AppState {
    scan: Mutex<Option<CancellationToken>>,
    pub link: crate::linking::LinkState,
    pub fix: crate::fixing::FixState,
}

pub(crate) fn state_dir() -> anyhow::Result<std::path::PathBuf> {
    let path = Config::default_path()?;
    Ok(path
        .parent()
        .map(|dir| dir.to_path_buf())
        .unwrap_or_default())
}

pub(crate) fn runbook_dir() -> Option<std::path::PathBuf> {
    state_dir().ok().map(|dir| dir.join("runbooks"))
}

pub(crate) fn open_store() -> anyhow::Result<FixStore> {
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

/// The checks this build knows how to run, and whether each would run here.
#[tauri::command]
pub fn probe_list() -> CmdResult<ork_core::probes::Catalogue> {
    let platform = ork_core::platform::detect().map_err(fail)?;
    Ok(ork_core::probes::catalogue(
        platform.as_ref(),
        ork_core::platform::is_elevated(),
    ))
}

/// Run a scan, emitting `scan://event` as each check finishes.
///
/// No tier has a time limit. The only thing that ends a scan early is
/// [`cancel_scan`], which is the user's decision.
#[tauri::command]
pub async fn start_scan(
    app: AppHandle,
    state: State<'_, AppState>,
    tier: String,
) -> CmdResult<ScanReport> {
    let tier: ScanTier = tier.parse().map_err(fail)?;

    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<ScanEvent>();
    let scanner = Scanner::new().map_err(fail)?.with_events(sender);
    let cancel = scanner.cancel_token();

    {
        let mut slot = state
            .scan
            .lock()
            .map_err(|_| "the scan lock was poisoned".to_string())?;
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
    let mut slot = state
        .scan
        .lock()
        .map_err(|_| "the scan lock was poisoned".to_string())?;
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
    let platform = ork_core::platform::detect()
        .map_err(fail)?
        .kind()
        .to_string();

    let analysis = Analyst::new(library, platform)
        .analyse(&report, &routing)
        .await
        .map_err(fail)?;
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
    let gpus = ork_core::platform::detect()
        .map_err(fail)?
        .gpus()
        .map_err(fail)?;
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

/// Everything on the triage queue, whatever state it is in.
///
/// All of it, not only what is still pending -- the same list `outlaw queue`
/// prints. The window works the queue now, and an item that vanishes the
/// instant it is fixed takes the evidence of the fix with it, leaving only the
/// audit log to say anything happened. Each row carries its own state, so
/// showing the finished ones costs nothing and answers "did that work?".
#[tauri::command]
pub fn queue_list() -> CmdResult<serde_json::Value> {
    let store = open_store().map_err(fail)?;
    let items = store.all().map_err(fail)?;
    Ok(serde_json::json!(items))
}

/// Everything the tool has checked, found, attempted, and changed.
///
/// Handed over exactly as the store keeps it, readable timestamp included.
/// This used to rebuild the rows into a near-identical struct of its own,
/// which is how the window and the command line ended up formatting the same
/// timestamp two different ways -- badly, in both cases.
#[tauri::command]
pub fn audit_list(limit: usize) -> CmdResult<Vec<ork_fix::store::AuditLine>> {
    let store = open_store().map_err(fail)?;
    // The clamp lives in the store now, so both front-ends get the same
    // answer to the same question without either of them knowing the number.
    store.audit_log(limit).map_err(fail)
}

/// What is running, and what a sweep would do to each.
///
/// The same [`Survey`] the terminal prints, handed over whole rather than
/// summarised here. Both front-ends showing the same judgement is the point;
/// a second place that decided what "held back" meant would eventually decide
/// it differently, and the difference would be found by somebody trusting the
/// wrong one.
///
/// Nothing here stops anything. See `docs/proposals/process-control.md`.
#[tauri::command]
pub fn process_survey() -> CmdResult<serde_json::Value> {
    let config = load_config().map_err(fail)?;
    let survey = Survey::of_this_machine(&config.processes.pinned).map_err(fail)?;
    // Shaped in `ork-core`, not here. The terminal's `--json` publishes the
    // same object from the same function, so the window and a script cannot be
    // told different things about one machine.
    Ok(survey.as_report())
}

/// Add a program to the leave-alone list, or take it off it.
///
/// The reason this is a command and not a line in the manual: the setting has
/// existed since the list did, and the only way to use it was to find a TOML
/// file and edit it by hand. A control whose entire meaning is "leave this one
/// alone" is exactly the sort a person wants to reach for while they are
/// looking at the thing they want left alone.
///
/// By name rather than by process id, deliberately. A browser is forty
/// processes and pinning one of them would leave the other thirty-nine
/// offered, which is not what anybody means by it -- and identifiers are
/// reused, so a pin against one would eventually apply to something else.
#[tauri::command]
pub fn process_pin(name: String, pinned: bool) -> CmdResult<bool> {
    let path = Config::default_path().map_err(fail)?;
    let mut config = Config::load_or_default(&path).map_err(fail)?;
    let changed = if pinned {
        config.processes.pin(&name)
    } else {
        config.processes.unpin(&name)
    };
    // Nothing to write is not a failure, and writing anyway would rewrite a
    // file the person may have laid out themselves for no reason at all.
    if changed {
        config.save(&path).map_err(fail)?;
    }
    Ok(changed)
}

/// Stop what a sweep offers. The window asks first; this does not.
///
/// The confirmation lives in the window because that is where the list being
/// agreed to is on screen. What lives here is everything that cannot be
/// trusted to a front-end: each target is judged again against a fresh look at
/// the machine, one at a time, and every attempt is written to the audit log
/// whether or not it changed anything.
///
/// Targets carry a name as well as an identifier. Identifiers are reused, and
/// the gap between a list being drawn and a button being pressed is long
/// enough for one to be handed to something else.
///
/// Nothing is put back. There is no snapshot of a running process and there
/// cannot be one, so the promise here is different from the rest of the tool
/// and is stated rather than implied: what was stopped is recorded and
/// returned, and starting anything again is the person's to do. See
/// `crates/ork-fix/src/processes.rs` for why capturing enough to restart
/// faithfully would be the wrong trade.
#[tauri::command]
pub fn process_stop(targets: Vec<ork_fix::processes::Target>) -> CmdResult<serde_json::Value> {
    let config = load_config().map_err(fail)?;
    let store = open_store().map_err(fail)?;
    let report =
        ork_fix::processes::stop_these(&targets, &config.processes.pinned, &store).map_err(fail)?;
    Ok(serde_json::json!({
        "stopped": report.stopped_count(),
        // Named as it is measured here as well. This is what they were holding
        // when last seen, not what came back to the machine.
        "memory_held_by_stopped": report.memory_held_by_stopped(),
        // Each attempt carries the sentence for it, written where the outcome
        // is decided rather than again in the window. The terminal prints the
        // same string; two front-ends describing one outcome in two ways is
        // two chances to describe it wrongly.
        "attempts": report
            .attempts
            .iter()
            .map(|attempt| {
                let mut value = serde_json::to_value(attempt).unwrap_or(serde_json::Value::Null);
                if let Some(object) = value.as_object_mut() {
                    object.insert(
                        "says".to_string(),
                        serde_json::Value::String(attempt.outcome.describe()),
                    );
                    object.insert(
                        "changed_anything".to_string(),
                        serde_json::Value::Bool(attempt.outcome.changed_anything()),
                    );
                }
                value
            })
            .collect::<Vec<_>>(),
    }))
}
