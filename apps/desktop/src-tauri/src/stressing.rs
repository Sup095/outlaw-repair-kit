//! The stress test, in the window.
//!
//! All the behaviour is in [`ork_core::stress`], shared with `outlaw stress`.
//! What is here is starting and stopping it, forwarding progress to the
//! window, and holding the last result so that switching tabs does not throw
//! away a report somebody waited an hour for.
//!
//! One thing this module treats differently from every other command here: it
//! keeps the run alive across tab changes but **not** across the window
//! closing. Closing the window ends the process, which drops the run, which
//! stops the workers -- and that is the behaviour we want. A watcher that
//! keeps watching after the window is closed is doing what it exists to do. A
//! stress test that kept heating the machine after the window was closed would
//! be a program that had escaped.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use tauri::{AppHandle, Emitter, State};
use tokio_util::sync::CancellationToken;

use ork_core::stress::{Plan, StressEvent, StressReport, StressTest, memory};

use crate::commands::CmdResult;

/// What the window needs to draw the Stress screen.
#[derive(Default)]
pub struct StressState {
    /// Behind an `Arc` because the task that runs the test has to clear this
    /// when the run ends, and it outlives the command that started it.
    running: Arc<Mutex<Option<CancellationToken>>>,
    /// The last completed report, so that leaving the screen and coming back
    /// does not lose it.
    last: Arc<Mutex<Option<StressReport>>>,
}

/// Whether a test is running, what the machine can offer it, and the last
/// result.
#[derive(serde::Serialize)]
pub struct StressStatus {
    pub running: bool,
    pub last: Option<StressReport>,
    /// How many cores a run would use if it were not told otherwise.
    pub cores: usize,
    /// How much memory a run would test at the given share, and how much is
    /// free -- so the screen can say the real number before anybody presses
    /// the button, rather than after.
    pub memory_bytes: u64,
    pub memory_available_bytes: u64,
    /// The floor that is never taken, whatever share is asked for.
    pub memory_reserved_bytes: u64,
}

fn fail(error: impl std::fmt::Display) -> String {
    format!("{error:#}")
}

/// What a run would do to this machine, asked before it does it.
#[tauri::command]
pub async fn stress_status(
    state: State<'_, StressState>,
    memory_share: f64,
) -> CmdResult<StressStatus> {
    let running = state
        .running
        .lock()
        .map_err(|_| "the stress lock was poisoned".to_string())?
        .is_some();
    let last = state
        .last
        .lock()
        .map_err(|_| "the stress result lock was poisoned".to_string())?
        .clone();

    let platform = ork_core::platform::detect().map_err(fail)?;
    let available = tokio::task::spawn_blocking(move || platform.memory())
        .await
        .map_err(fail)?
        .map_err(fail)?
        .available_bytes;

    let memory_bytes = match memory::budget(available, memory_share) {
        memory::Budget::Test { bytes } => bytes,
        // Zero means "the memory will not be tested", and the screen says so
        // in those words rather than showing "0 B of memory".
        memory::Budget::NotEnoughSpare { .. } => 0,
    };

    Ok(StressStatus {
        running,
        last,
        cores: std::thread::available_parallelism()
            .map(|count| count.get())
            .unwrap_or(1),
        memory_bytes,
        memory_available_bytes: available,
        memory_reserved_bytes: memory::RESERVED_BYTES,
    })
}

#[tauri::command]
pub async fn stress_start(
    app: AppHandle,
    state: State<'_, StressState>,
    cpu: bool,
    memory_test: bool,
    minutes: u64,
    memory_share: f64,
) -> CmdResult<()> {
    let plan = Plan {
        cpu,
        memory: memory_test,
        duration: Duration::from_secs(minutes.saturating_mul(60)),
        memory_share,
        threads: None,
    };
    if plan.is_empty() {
        return Err(
            "nothing to test: with both the processor and the memory turned off there is no \
             work to do"
                .to_string(),
        );
    }

    let platform = ork_core::platform::detect().map_err(fail)?;
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<StressEvent>();
    let mut test = StressTest::new(plan).with_events(sender);
    let cancel = test.cancel_token();

    {
        let mut slot = state
            .running
            .lock()
            .map_err(|_| "the stress lock was poisoned".to_string())?;
        if slot.is_some() {
            return Err("a stress test is already running".to_string());
        }
        *slot = Some(cancel);
    }

    let last = Arc::clone(&state.last);
    let forwarder = app.clone();
    tokio::spawn(async move {
        while let Some(event) = receiver.recv().await {
            // Kept before it is emitted, so a result exists whether or not the
            // window was listening at that moment.
            if let StressEvent::Finished { report } = &event
                && let Ok(mut slot) = last.lock()
            {
                *slot = Some((**report).clone());
            }
            if let Err(error) = forwarder.emit("stress://event", &event) {
                tracing::debug!(%error, "could not deliver a stress event");
            }
        }
    });

    // The run outlives this command, the same way the watcher does: a command
    // that did not return until an hour-long burn-in finished would be a
    // command that never returns, and the window would be frozen for the whole
    // test with no way to press Stop.
    let running = Arc::clone(&state.running);
    tokio::spawn(async move {
        if let Err(error) = test.run(platform).await {
            tracing::warn!(%error, "the stress test stopped with an error");
        }
        // Cleared here rather than in `stress_stop`, because a run that
        // finished on its own, was stopped for heat, or failed all end here
        // too -- and a screen still showing "running" after the machine has
        // gone quiet is a screen nobody trusts again.
        if let Ok(mut slot) = running.lock() {
            *slot = None;
        }
    });

    Ok(())
}

#[tauri::command]
pub fn stress_stop(state: State<'_, StressState>) -> CmdResult<()> {
    let mut slot = state
        .running
        .lock()
        .map_err(|_| "the stress lock was poisoned".to_string())?;
    if let Some(cancel) = slot.take() {
        cancel.cancel();
    }
    Ok(())
}
