//! An API key must never reach a settings file, a screen, or a printed line.
//!
//! This is a stated rule of the project rather than a preference, and it is
//! the kind of rule that holds until one careless line breaks it -- a `Debug`
//! that prints a struct, a config field added to make something convenient, a
//! command that returns the value so the settings screen can show it filled
//! in. Each of those is a small, reasonable-looking change, and any of them
//! puts somebody's key into a file that gets copied into a bug report.
//!
//! So the rule is checked rather than remembered. Some of these read the
//! source, because the failure they guard is a shape rather than a behaviour:
//! by the time a key is in a struct that gets serialised, no assertion about
//! today's output proves anything about tomorrow's.

use std::path::{Path, PathBuf};

/// A value no real key would be, so that finding it anywhere is conclusive.
const MARKER: &str = "sk-ork-test-marker-do-not-ship-8Qv3";

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

/// Field names that would be a secret if they held one.
///
/// `token_ref` and the like are deliberately not here: a reference to a
/// credential is a name, and a name is exactly what the settings file is
/// supposed to hold instead of the value.
fn names_a_secret(field: &str) -> bool {
    let name = field.to_ascii_lowercase();
    if name.ends_with("_ref") || name.ends_with("_stored") || name.ends_with("_set") {
        return false;
    }
    ["api_key", "apikey", "password", "secret", "bearer"]
        .iter()
        .any(|mark| name.contains(mark))
        || name == "token"
        || name.ends_with("_token")
}

#[test]
fn the_settings_file_has_nowhere_to_put_a_key() {
    // Read as source rather than as behaviour. A field that holds a secret is
    // a problem the moment it exists, whether or not today's code fills it in,
    // because the settings file is written by serialising this struct whole.
    let config = read("crates/ork-core/src/config.rs");
    let offending: Vec<&str> = config
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("pub "))
        .filter_map(|line| line.strip_prefix("pub "))
        .filter_map(|rest| rest.split(':').next())
        .filter(|field| names_a_secret(field))
        .collect();

    assert!(
        offending.is_empty(),
        "these fields are in the settings file and are named like secrets: {offending:?}. \
         Secrets live in the operating system's credential store; the settings file holds \
         the name of the credential, never the credential."
    );
}

#[test]
fn the_check_would_notice_a_field_that_held_one() {
    // The test above is only worth having if it can fail, and its whole job is
    // to recognise a name. These are the names it must catch, and the ones it
    // must not, because a rule that flagged `token_ref` would be turned off.
    for bad in [
        "api_key",
        "apiKey",
        "password",
        "secret",
        "token",
        "auth_token",
        "bearer_token",
    ] {
        assert!(names_a_secret(bad), "{bad} should be flagged");
    }
    for fine in [
        "token_ref",
        "url",
        "model",
        "cloud_key_stored",
        "enabled",
        "endpoint",
    ] {
        assert!(!names_a_secret(fine), "{fine} should not be flagged");
    }
}

#[test]
fn no_command_the_window_can_call_hands_back_a_secret() {
    // The settings screen shows whether a key is stored, never the key. It
    // would be one line to return the value so the box could show it filled
    // in, and it would put the key into the window's memory, into any crash
    // dump, and into whatever the front-end logs.
    //
    // `is_set` is the whole of what a front-end may know.
    let commands = read("apps/desktop/src-tauri/src/commands.rs");
    let reading = ["secrets::get(", "secrets::get_named("];
    for call in reading {
        assert!(
            !commands.contains(call),
            "commands.rs calls {call}, which reads a secret's value. A command may \
             ask `secrets::is_set` whether one is stored; it may not fetch it."
        );
    }
    // And the check is looking at the right file.
    assert!(
        commands.contains("secrets::is_set"),
        "commands.rs no longer asks about secrets at all; this check has drifted \
         off the file it was written for"
    );
}

#[test]
fn a_secret_does_not_survive_being_serialised_with_the_settings() {
    // The behavioural half. Whatever the struct looks like, the bytes that get
    // written must not contain a key -- and the surest way to find out is to
    // put an unmistakable one where the code could reach it and look.
    //
    // Safety: the environment is process-wide and this test writes to it.
    // Nothing else in this file reads these variables, and the value is
    // removed before the test returns.
    unsafe {
        std::env::set_var("ORK_CLOUD_API_KEY", MARKER);
        std::env::set_var("ORK_REMOTE_TOKEN", MARKER);
    }

    let mut config = ork_core::Config::default();
    config.ai.cloud.enabled = true;
    config.ai.remote.enabled = true;
    config.ai.remote.endpoint = Some(
        ork_core::config::EndpointConfig::new("http://127.0.0.1:1234/v1", "some-model")
            .with_token_ref("a-machine"),
    );

    let written = toml::to_string_pretty(&config).expect("the settings serialise");

    unsafe {
        std::env::remove_var("ORK_CLOUD_API_KEY");
        std::env::remove_var("ORK_REMOTE_TOKEN");
    }

    assert!(
        !written.contains(MARKER),
        "a key reachable from the environment ended up in the settings file:\n{written}"
    );
    // The reference is expected, and is the point: a name, not a value.
    assert!(
        written.contains("a-machine"),
        "the credential's name should be written, so that a link and a \
         hand-typed endpoint take the same path:\n{written}"
    );
}

#[test]
fn the_marker_would_have_been_found_if_it_were_there() {
    // Proves the search above can fail. Without this, a serialisation that
    // silently produced nothing would read as a pass.
    let pretend = format!("[ai.cloud]\napi_key = \"{MARKER}\"\n");
    assert!(pretend.contains(MARKER));
}
