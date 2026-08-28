//! Desktop front-end for the Outlaw Repair Kit.
//!
//! Like the command line, this is a thin shell. Every command below is a call
//! into the shared crates, so nothing the desktop can do is unreachable from
//! a script -- which is the rule the whole project is built on.
//!
//! Long-running work (start-up, a scan) reports progress by emitting events
//! rather than blocking until it is done, so the window stays responsive and
//! can be cancelled at any point. Nothing here has a time limit.

mod commands;
mod fixing;
mod linking;
mod manual;
mod reporting;
mod stressing;
mod watching;

/// Set up logging, and start keeping a record of anything that goes wrong.
///
/// A window has no terminal behind it, so without this a crash leaves nothing
/// at all -- which is the whole reason the reporting screen needs a record to
/// draw on. The console filter is attached to the console layer alone, so what
/// gets recorded does not depend on how chatty anyone asked the log to be.
fn start_recording() {
    use tracing_subscriber::Layer as _;
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;

    let filter = tracing_subscriber::EnvFilter::try_from_env("ORK_LOG")
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn"));

    let state_dir = commands::state_dir().ok();
    if let Some(dir) = state_dir.clone() {
        ork_core::incident::catch_crashes(dir);
    }

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(std::io::stderr)
                .with_filter(filter),
        )
        .with(state_dir.map(ork_core::incident::IncidentLayer::new))
        .init();
}

/// Build and run the desktop application.
pub fn run() {
    start_recording();

    tauri::Builder::default()
        .manage(commands::AppState::default())
        .manage(watching::WatchState::default())
        .manage(stressing::StressState::default())
        .invoke_handler(tauri::generate_handler![
            commands::boot,
            commands::host_info,
            commands::probe_list,
            commands::start_scan,
            commands::cancel_scan,
            commands::explain_report,
            commands::settings_load,
            commands::settings_save,
            commands::secret_status,
            commands::secret_set,
            commands::secret_clear,
            commands::routing_status,
            commands::queue_list,
            commands::audit_list,
            watching::watch_status,
            watching::watch_start,
            watching::watch_stop,
            watching::watch_forget,
            stressing::stress_status,
            stressing::stress_start,
            stressing::stress_stop,
            manual::manual_contents,
            manual::manual_page,
            manual::manual_licence,
            fixing::fix_run,
            fixing::fix_answer,
            fixing::fix_cancel,
            reporting::report_build,
            reporting::report_incidents,
            reporting::report_open_issue,
            reporting::report_open_form,
            reporting::report_save,
            reporting::report_clear,
            linking::link_status,
            linking::link_host_start,
            linking::link_host_stop,
            linking::link_pair_reopen,
            linking::link_find,
            linking::link_join,
            linking::link_remove,
            linking::link_view,
            linking::link_check,
        ])
        .run(tauri::generate_context!())
        .expect("the desktop application could not start");
}
