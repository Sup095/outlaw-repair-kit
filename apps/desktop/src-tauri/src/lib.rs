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
mod linking;

/// Build and run the desktop application.
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("ORK_LOG")
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .with_writer(std::io::stderr)
        .init();

    tauri::Builder::default()
        .manage(commands::AppState::default())
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
