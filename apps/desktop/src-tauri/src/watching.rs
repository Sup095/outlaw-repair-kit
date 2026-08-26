//! The watcher, in the window.
//!
//! All the behaviour is in [`ork_core::watch`], shared with `outlaw watch`.
//! What is here is starting and stopping it, forwarding what it notices to the
//! window, and one thing the terminal does not have to think about: a watcher
//! that is running while somebody is looking at another screen.
//!
//! That last point is why changes are kept in memory here rather than only
//! emitted as events. A person who starts the watcher, goes to the Scan screen
//! for twenty minutes and comes back should find what happened in those twenty
//! minutes, not an empty panel -- which would be indistinguishable from
//! "nothing happened" and is the same class of bug as a scan report being
//! thrown away by switching tabs.

use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Emitter, State};
use tokio_util::sync::CancellationToken;

use ork_core::tier::ScanTier;
use ork_core::watch::{Baseline, Look, WatchEvent, Watcher};

use crate::commands::CmdResult;

/// What the window needs to draw the Watch screen.
#[derive(Default)]
pub struct WatchState {
    running: Mutex<Option<CancellationToken>>,
    /// Everything noticed since the watcher was started, newest first.
    ///
    /// Held rather than only emitted, so that leaving the screen and coming
    /// back does not read as "nothing happened".
    history: Arc<Mutex<Vec<Look>>>,
}

/// How many looks-with-something-in-them to keep.
///
/// Only looks that carried a change are kept at all, so this is a great many
/// actual events rather than a great many quarter-hours.
const HISTORY_LIMIT: usize = 200;

/// Whether the watcher is running, and what it has noticed.
#[derive(serde::Serialize)]
pub struct WatchStatus {
    pub running: bool,
    /// What is remembered about this machine, including what is held quiet.
    pub baseline: Baseline,
    /// Where that is kept, so the window can offer to reset it by name.
    pub baseline_path: String,
    /// Looks that carried a change, newest first.
    pub history: Vec<Look>,
}

fn fail(error: impl std::fmt::Display) -> String {
    format!("{error:#}")
}

/// Add a look to the history, if it is worth keeping.
///
/// A look that changed nothing is not history: keeping every quiet round would
/// bury the four that mattered under three hundred that did not. Newest first,
/// because somebody returning to this screen is looking for what happened most
/// recently.
///
/// A free function rather than a closure inside the forwarding task, so that
/// the tests below exercise the rule the application actually applies instead
/// of a second copy of it that can drift.
fn remember(history: &Mutex<Vec<Look>>, event: &WatchEvent) {
    let WatchEvent::Looked { look } = event else {
        return;
    };
    if look.changes.is_empty() {
        return;
    }
    if let Ok(mut kept) = history.lock() {
        kept.insert(0, (**look).clone());
        kept.truncate(HISTORY_LIMIT);
    }
}

#[tauri::command]
pub fn watch_status(state: State<'_, WatchState>) -> CmdResult<WatchStatus> {
    let path = Baseline::default_path().map_err(fail)?;
    let running = state
        .running
        .lock()
        .map_err(|_| "the watch lock was poisoned".to_string())?
        .is_some();
    let history = state
        .history
        .lock()
        .map_err(|_| "the watch history lock was poisoned".to_string())?
        .clone();

    Ok(WatchStatus {
        running,
        baseline: Baseline::load(&path),
        baseline_path: path.display().to_string(),
        history,
    })
}

#[tauri::command]
pub async fn watch_start(
    app: AppHandle,
    state: State<'_, WatchState>,
    tier: String,
    every_minutes: u64,
) -> CmdResult<()> {
    let tier: ScanTier = tier.parse().map_err(fail)?;
    let interval = std::time::Duration::from_secs(every_minutes.saturating_mul(60));

    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<WatchEvent>();
    let watcher = Watcher::new()
        .tier(tier)
        .interval(interval)
        .with_events(sender);
    let cancel = watcher.cancel_token();

    {
        let mut slot = state
            .running
            .lock()
            .map_err(|_| "the watch lock was poisoned".to_string())?;
        if slot.is_some() {
            return Err("the watcher is already running".to_string());
        }
        *slot = Some(cancel);
    }

    let history = Arc::clone(&state.history);
    tokio::spawn(async move {
        while let Some(event) = receiver.recv().await {
            remember(&history, &event);
            if let Err(error) = app.emit("watch://event", &event) {
                tracing::debug!(%error, "could not deliver a watch event");
            }
        }
    });

    // The watcher outlives this command: `watch_start` returns as soon as it
    // is running, and the window hears about it through events. A command that
    // did not return until the watcher stopped would be a command that never
    // returns.
    tokio::spawn(async move {
        if let Err(error) = watcher.run().await {
            tracing::warn!(%error, "the watcher stopped with an error");
        }
    });

    Ok(())
}

#[tauri::command]
pub fn watch_stop(state: State<'_, WatchState>) -> CmdResult<()> {
    let mut slot = state
        .running
        .lock()
        .map_err(|_| "the watch lock was poisoned".to_string())?;
    if let Some(cancel) = slot.take() {
        cancel.cancel();
    }
    Ok(())
}

/// Forget everything the watcher remembers and start over.
///
/// Deliberately a separate, named action rather than something that happens
/// quietly: the next look after this records a fresh starting point and
/// reports nothing, so a person who did this without meaning to would see a
/// watcher that has apparently stopped noticing anything.
#[tauri::command]
pub fn watch_forget(state: State<'_, WatchState>) -> CmdResult<()> {
    let path = Baseline::default_path().map_err(fail)?;
    match std::fs::remove_file(&path) {
        Ok(()) => {}
        // Nothing to forget is the same outcome as having forgotten it.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(fail(error)),
    }
    if let Ok(mut kept) = state.history.lock() {
        kept.clear();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ork_core::finding::{Category, Finding, Severity, Triage};
    use ork_core::watch::Change;
    use time::OffsetDateTime;

    fn look(changes: Vec<Change>) -> Look {
        Look {
            at: OffsetDateTime::now_utc(),
            changes,
            established_baseline: false,
            recorded: 0,
            did_not_run: Vec::new(),
        }
    }

    fn change() -> Change {
        Change::Appeared {
            finding: Box::new(
                Finding::builder("storage", "storage.full")
                    .severity(Severity::High)
                    .category(Category::Storage)
                    .title("The system drive is nearly full")
                    .detail("detail")
                    .triage(Triage::None)
                    .build(),
            ),
        }
    }

    fn looked(changes: Vec<Change>) -> WatchEvent {
        WatchEvent::Looked {
            look: Box::new(look(changes)),
        }
    }

    #[test]
    fn a_look_that_changed_nothing_is_not_kept() {
        // Keeping every quiet round would bury the four that mattered under
        // three hundred that did not.
        let history = Mutex::new(Vec::new());
        remember(&history, &looked(Vec::new()));
        assert!(history.lock().unwrap().is_empty());

        remember(&history, &looked(vec![change()]));
        assert_eq!(history.lock().unwrap().len(), 1);
    }

    #[test]
    fn the_newest_thing_that_happened_is_first() {
        // Somebody coming back to this screen is looking for what happened
        // most recently, not for what happened first.
        let history = Mutex::new(Vec::new());
        for _ in 0..3 {
            remember(&history, &looked(vec![change()]));
        }
        let kept = history.lock().unwrap();
        assert_eq!(kept.len(), 3);
        assert!(kept[0].at >= kept[2].at);
    }

    #[test]
    fn history_does_not_grow_without_limit() {
        // This runs for as long as somebody leaves the window open, which may
        // be weeks.
        let history = Mutex::new(Vec::new());
        for _ in 0..(HISTORY_LIMIT + 50) {
            remember(&history, &looked(vec![change()]));
        }
        assert_eq!(history.lock().unwrap().len(), HISTORY_LIMIT);
    }
}
