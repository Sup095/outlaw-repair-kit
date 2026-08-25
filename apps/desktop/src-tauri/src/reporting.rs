//! Reporting a crash or an error from the window.
//!
//! The same three steps as `outlaw report`: build the text, show it, hand over
//! a link. The window can do one thing the terminal cannot, which is let
//! somebody edit the report before it goes anywhere -- so what is posted is
//! whatever is on screen when they press the button, not what the tool
//! generated.
//!
//! Nothing here posts anything, and there is no command that could. The window
//! opens GitHub's form; the person reads it and presses the button.

use ork_core::incident;
use ork_core::incident::report::{Context, Report};
use tauri::State;

use crate::commands::{AppState, CmdResult, fail, state_dir};

/// What this build and this machine are, for the report's header.
fn context() -> Context {
    let host = ork_core::platform::detect()
        .and_then(|platform| Ok((platform.kind().to_string(), platform.host()?)))
        .ok();

    Context {
        version: env!("CARGO_PKG_VERSION").to_string(),
        platform: host
            .as_ref()
            .map(|(kind, _)| kind.clone())
            .unwrap_or_default(),
        os_name: host
            .as_ref()
            .map(|(_, host)| host.os_name.clone())
            .unwrap_or_default(),
        architecture: host
            .as_ref()
            .map(|(_, host)| host.arch.clone())
            .unwrap_or_default(),
        extra: Vec::new(),
    }
}

/// Build the report. This is exactly what would be posted.
#[tauri::command]
pub fn report_build() -> CmdResult<Report> {
    let state_dir = state_dir().map_err(fail)?;
    Ok(incident::report::build(&state_dir, &context()))
}

/// Everything recorded, newest last, for the window to list.
#[tauri::command]
pub fn report_incidents(limit: usize) -> CmdResult<Vec<incident::Incident>> {
    let state_dir = state_dir().map_err(fail)?;
    Ok(incident::recent(&state_dir, limit))
}

/// Open the issue form, prefilled with whatever the window is showing.
///
/// The title and body come from the window rather than being rebuilt here, so
/// edits the person made are what gets carried across. Rebuilding would
/// silently discard them, which is the sort of thing that is only noticed
/// after the issue is posted.
#[tauri::command]
pub fn report_open_issue(title: String, body: String) -> CmdResult<String> {
    let url = incident::report::prefilled_url(&title, &body)
        .ok_or_else(|| "the report could not be turned into a link".to_string())?;

    // Too long to carry in a link. Saying so beats opening a form with the end
    // of the report missing.
    if url.len() > 6000 {
        return Err(
            "This report is too long to carry in a link. Save it and attach it to the issue \
             instead."
                .to_string(),
        );
    }

    ork_core::platform::open_url(&url).map_err(fail)?;
    Ok(url)
}

/// Open the plain new-issue form, for a report that has to be attached.
#[tauri::command]
pub fn report_open_form() -> CmdResult<String> {
    let url = format!("{}/issues/new", ork_core::REPOSITORY);
    ork_core::platform::open_url(&url).map_err(fail)?;
    Ok(url)
}

/// Write the report where the person can attach it to an issue.
#[tauri::command]
pub fn report_save(body: String) -> CmdResult<String> {
    let path = state_dir().map_err(fail)?.join("problem-report.md");
    std::fs::write(&path, body).map_err(fail)?;
    Ok(path.display().to_string())
}

/// Forget everything recorded so far.
#[tauri::command]
pub fn report_clear(_state: State<'_, AppState>) -> CmdResult<()> {
    let state_dir = state_dir().map_err(fail)?;
    incident::clear(&state_dir).map_err(fail)
}
