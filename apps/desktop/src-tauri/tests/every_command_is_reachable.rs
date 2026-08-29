//! Every `#[tauri::command]` must be handed to `generate_handler!`.
//!
//! A source check, because the failure it guards is invisible everywhere else.
//! An unregistered command compiles, passes clippy, passes every other test,
//! and produces a screen that loads and then fails the moment somebody uses
//! it -- with an error naming a command that plainly exists in the source.
//! The list in `lib.rs` is hand-maintained, and a hand-maintained list that
//! nothing checks is a list that will eventually be wrong.
//!
//! It is written the other way round too. A name in the handler list that no
//! longer has a command behind it will not compile, so that direction is
//! already guarded; what is not guarded is the direction where somebody adds
//! a command and forgets the line. This session added `process_survey`, which
//! is exactly how the omission happens: the interesting work is the screen,
//! and the registration is one line somewhere else.

use std::path::{Path, PathBuf};

fn crate_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn source_files() -> Vec<PathBuf> {
    let mut found = Vec::new();
    let source = crate_root().join("src");
    let entries = std::fs::read_dir(&source)
        .unwrap_or_else(|error| panic!("could not read {}: {error}", source.display()));
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|kind| kind == "rs") {
            found.push(path);
        }
    }
    found.sort();
    found
}

/// The function name on the line after a `#[tauri::command]` attribute.
///
/// Attributes may be stacked and the signature may be `pub fn`, `pub async
/// fn`, or have a doc comment between -- no, a doc comment goes above the
/// attribute, not below it. What can appear between is another attribute.
fn commands_in(text: &str) -> Vec<String> {
    let lines: Vec<&str> = text.lines().collect();
    let mut names = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        if line.trim() != "#[tauri::command]" {
            continue;
        }
        // Walk forward past any further attributes to the signature itself.
        let signature = lines[index + 1..]
            .iter()
            .find(|next| !next.trim().starts_with('#'))
            .unwrap_or_else(|| panic!("a #[tauri::command] on line {} has no function", index + 1));
        let name = signature
            .split("fn ")
            .nth(1)
            .and_then(|rest| rest.split(['(', '<']).next())
            .unwrap_or_else(|| panic!("could not read a function name from {signature:?}"))
            .trim()
            .to_string();
        names.push(name);
    }
    names
}

/// Everything named inside `generate_handler![ ... ]`, without its module.
fn registered() -> Vec<String> {
    let text = std::fs::read_to_string(crate_root().join("src/lib.rs"))
        .expect("the crate has a lib.rs to register commands in");
    let start = text
        .find("generate_handler![")
        .expect("lib.rs registers its commands with generate_handler!");
    let rest = &text[start..];
    let end = rest.find(']').expect("the handler list is closed");
    rest[..end]
        .lines()
        .skip(1)
        .filter_map(|line| {
            let line = line.trim().trim_end_matches(',');
            if line.is_empty() || line.starts_with("//") {
                return None;
            }
            // `commands::audit_list` -> `audit_list`.
            line.rsplit("::").next().map(str::to_string)
        })
        .collect()
}

#[test]
fn every_command_is_registered_with_the_window() {
    let listed = registered();
    let mut defined: Vec<(String, String)> = Vec::new();
    for path in source_files() {
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("could not read {}: {error}", path.display()));
        let file = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        for name in commands_in(&text) {
            defined.push((name, file.clone()));
        }
    }

    // If this ever finds nothing it has stopped checking, and a check that
    // silently stopped checking is worse than no check -- it reads as a pass.
    assert!(
        defined.len() > 20,
        "only {} commands found; the scan has stopped working",
        defined.len()
    );
    assert!(
        listed.len() > 20,
        "only {} names read out of generate_handler!; the parse has stopped working",
        listed.len()
    );

    let missing: Vec<String> = defined
        .iter()
        .filter(|(name, _)| !listed.contains(name))
        .map(|(name, file)| format!("{name} (in {file})"))
        .collect();

    assert!(
        missing.is_empty(),
        "these commands exist but are not in generate_handler! in lib.rs, so the \
         window cannot call them:\n  {}",
        missing.join("\n  ")
    );
}

#[test]
fn the_handler_list_has_no_repeats() {
    // Two of the same name is a merge that went wrong, and Tauri will take it
    // without complaint.
    let mut listed = registered();
    let before = listed.len();
    listed.sort();
    listed.dedup();
    assert_eq!(
        before,
        listed.len(),
        "generate_handler! names something twice"
    );
}

#[test]
fn the_scan_can_read_a_command_it_is_shown() {
    // The parser itself, on shapes that actually occur in this crate: a plain
    // one, an async one, and one with another attribute in between. Without
    // this, a parser that quietly matched nothing would make the test above
    // pass for the worst possible reason.
    let sample = "\
#[tauri::command]
pub fn plain() -> u8 { 0 }

/// Doc comment above the attribute.
#[tauri::command]
pub async fn with_await() -> u8 { 0 }

#[tauri::command]
#[allow(dead_code)]
pub fn behind_another_attribute(argument: u8) -> u8 { argument }

pub fn not_a_command() -> u8 { 0 }
";
    assert_eq!(
        commands_in(sample),
        vec!["plain", "with_await", "behind_another_attribute"]
    );
}
