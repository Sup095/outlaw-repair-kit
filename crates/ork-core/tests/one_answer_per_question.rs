//! The window and the terminal must not describe one machine two ways.
//!
//! Both publish the process survey as machine-readable output, and both used
//! to build that object by hand -- twenty lines of `serde_json::json!` in
//! `crates/ork-cli/src/processes.rs`, and twenty more in
//! `apps/desktop/src-tauri/src/commands.rs`. They had already drifted: the
//! terminal published `protected`, `held_back` and `candidates` and the window
//! did not.
//!
//! That is a small difference and it is the wrong thing to fix on its own,
//! because the next one arrives the next time somebody adds a field to one
//! caller and not the other. Nothing would fail. Both would keep compiling,
//! both would keep returning valid JSON, and the two would keep answering the
//! same question about the same machine differently -- which is the exact
//! failure this tool exists not to have.
//!
//! So the object is built once, in `Survey::as_report`, and this reads both
//! callers to make sure it stays that way. A source check, because the fault
//! is a shape rather than a behaviour: by the time a second copy exists, an
//! assertion about today's output proves nothing about tomorrow's.

use std::path::{Path, PathBuf};

/// A key nothing but the survey report has any reason to name.
const A_KEY_OF_THE_REPORT: &str = "memory_held_by_candidates";

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("this crate lives two levels below the workspace root")
        .to_path_buf()
}

fn read(relative: &str) -> String {
    let path = workspace().join(relative);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("could not read {}: {error}", path.display()))
}

/// The two places the survey reaches the outside world.
fn publishers() -> Vec<(&'static str, String)> {
    vec![
        ("the terminal", read("crates/ork-cli/src/processes.rs")),
        ("the window", read("apps/desktop/src-tauri/src/commands.rs")),
    ]
}

#[test]
fn both_front_ends_publish_the_same_object() {
    for (who, source) in publishers() {
        assert!(
            source.contains("as_report()"),
            "{who} no longer publishes the survey through `Survey::as_report`, so \
             it is describing the machine in its own words again"
        );
    }
}

#[test]
fn neither_front_end_builds_a_second_copy_of_it() {
    // The key as a *string*, which is somebody assembling the object again by
    // hand. Calling `memory_held_by_candidates()` is a different thing and a
    // correct one -- the terminal asks for that number to print it in words --
    // so the rule is about the quoted name, not the method.
    let written_as_a_key = format!("\"{A_KEY_OF_THE_REPORT}\"");
    for (who, source) in publishers() {
        let offending: Vec<&str> = source
            .lines()
            .map(str::trim)
            .filter(|line| line.contains(&written_as_a_key))
            .filter(|line| !line.starts_with("//"))
            .collect();
        assert!(
            offending.is_empty(),
            "{who} names `{A_KEY_OF_THE_REPORT}` itself:\n  {}\n\nThe report is built \
             in `Survey::as_report` so that there is one of it. Add the field there \
             and both front-ends get it.",
            offending.join("\n  ")
        );
    }
}

#[test]
fn the_check_is_looking_at_files_that_exist_and_do_the_job() {
    // Without this, a renamed or moved file would make both checks above pass
    // by reading nothing -- and "reads as a pass" is the failure mode every
    // source check in this repository has to be defended against.
    for (who, source) in publishers() {
        assert!(
            source.len() > 2_000,
            "{who}'s source came back suspiciously short; this check has drifted \
             off the file it was written for"
        );
        assert!(
            source.contains("Survey::of_this_machine"),
            "{who} no longer takes a survey at all"
        );
    }
    // And the key really is a key of the report, so the rule above is about
    // something real.
    let core = read("crates/ork-core/src/processes/survey.rs");
    assert!(
        core.contains(A_KEY_OF_THE_REPORT),
        "`{A_KEY_OF_THE_REPORT}` is not in the report any more, so the rule above \
         is guarding a name that means nothing"
    );
}
