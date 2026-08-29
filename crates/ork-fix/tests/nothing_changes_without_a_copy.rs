//! Nothing may change a file that was not copied first.
//!
//! This is the promise the whole tool rests on. Every other safety rail here
//! -- the dry run, the confirmation, one change at a time -- assumes that if
//! the change turns out to be wrong, the machine can be put back. That
//! assumption is only true while every path that writes, deletes, or renames
//! has taken a copy first.
//!
//! `FixEngine::apply` says so in a comment: "there is no path through this
//! code that changes a file without first copying it". A comment is a claim
//! about the code as it was when somebody wrote the comment. Adding a new
//! action is exactly when it stops being true, and adding a new action is a
//! normal thing to do -- the action is the interesting part, and `capture` is
//! one line that is easy to leave for later and then forget.
//!
//! So this reads the source. The behavioural tests next to `Snapshot` prove a
//! copy can be restored; this proves a copy was taken.

use std::path::{Path, PathBuf};

/// Calls that change something on disk.
///
/// Deliberately broad. A name here that turns out to be harmless costs one
/// marker comment; a name missing from here costs somebody their file.
const CHANGES_A_FILE: &[&str] = &[
    "fs::remove_file",
    "fs::remove_dir",
    "fs::write",
    "fs::rename",
    "fs::copy",
    "fs::set_permissions",
    "fs::create_dir",
    "File::create",
    "OpenOptions",
];

/// Written on the same line or just above, where a change is genuinely not
/// undoable by copying and the reason is written down next to it.
const EXPLAINED: &str = "no snapshot:";

/// How many lines above a change the `capture` may be.
///
/// A match arm normally reads: capture, then the change. Ten lines allows for
/// the arm's own setup without reaching back into the arm before it.
const WITHIN: usize = 10;

fn engine_source() -> String {
    let path: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/engine.rs");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("could not read {}: {error}", path.display()))
}

/// The body of `fn apply`, which is the only place an action is carried out.
fn apply_body(source: &str) -> Vec<&str> {
    let lines: Vec<&str> = source.lines().collect();
    let start = lines
        .iter()
        .position(|line| line.contains("fn apply(") && line.contains("Snapshot"))
        .expect("engine.rs has an `fn apply` that takes a Snapshot");
    // Ends at the next line that closes a function at the same indentation.
    let end = lines[start + 1..]
        .iter()
        .position(|line| line.trim_end() == "    }")
        .map(|at| start + 1 + at)
        .expect("`fn apply` is closed");
    lines[start..=end].to_vec()
}

#[test]
fn every_change_in_apply_is_preceded_by_a_copy() {
    let source = engine_source();
    let body = apply_body(&source);

    // If the extraction ever finds nothing, the check has stopped checking,
    // and a check that silently stopped checking reads as a pass.
    assert!(
        body.len() > 15,
        "only {} lines of `fn apply` were found; the scan has stopped working",
        body.len()
    );

    let mut unguarded = Vec::new();
    for (index, line) in body.iter().enumerate() {
        if line.trim_start().starts_with("//") {
            continue;
        }
        let Some(call) = CHANGES_A_FILE.iter().find(|call| line.contains(**call)) else {
            continue;
        };
        let from = index.saturating_sub(WITHIN);
        let explained = body[from..=index]
            .iter()
            .any(|near| near.contains(EXPLAINED));
        let captured = body[from..=index]
            .iter()
            .any(|near| near.contains("snapshot.capture("));
        if !captured && !explained {
            unguarded.push(format!("`{call}` on line {}: {}", index + 1, line.trim()));
        }
    }

    assert!(
        unguarded.is_empty(),
        "these change something on disk with no `snapshot.capture` above them, so \
         the change could not be undone:\n  {}\n\nTake the copy first, or write \
         `// {EXPLAINED} <why>` next to it if a copy genuinely cannot help.",
        unguarded.join("\n  ")
    );
}

#[test]
fn the_scan_finds_the_change_that_is_known_to_be_there() {
    // `RemoveStaleFile` deletes a file, and is the reason this rule exists. If
    // the scan stopped recognising it, the test above would pass by finding
    // nothing to check rather than by finding everything guarded.
    let source = engine_source();
    let body = apply_body(&source);
    let changes: Vec<&&str> = body
        .iter()
        .filter(|line| CHANGES_A_FILE.iter().any(|call| line.contains(*call)))
        .collect();
    assert!(
        !changes.is_empty(),
        "`fn apply` no longer appears to change anything, which is either a very \
         large change to this tool or a scan that has drifted off its target"
    );
}

#[test]
fn the_scan_would_notice_an_unguarded_change() {
    // Proves the rule can fail, without breaking the real file to find out.
    let pretend: Vec<&str> = vec![
        "    fn apply(&self, action: &FixAction, snapshot: &mut Snapshot) -> Result<()> {",
        "        match action {",
        "            FixAction::Something { path } => {",
        "                std::fs::remove_file(path)?;",
        "                Ok(())",
        "            }",
        "        }",
        "    }",
    ];
    let found = pretend
        .iter()
        .enumerate()
        .filter(|(index, line)| {
            CHANGES_A_FILE.iter().any(|call| line.contains(*call))
                && !pretend[index.saturating_sub(WITHIN)..=*index]
                    .iter()
                    .any(|near| near.contains("snapshot.capture("))
        })
        .count();
    assert_eq!(found, 1, "an unguarded delete should be caught");

    // And the same thing with the copy taken must pass.
    let guarded: Vec<&str> = vec![
        "                snapshot.capture(path)?;",
        "                std::fs::remove_file(path)?;",
    ];
    let still_found = guarded
        .iter()
        .enumerate()
        .filter(|(index, line)| {
            CHANGES_A_FILE.iter().any(|call| line.contains(*call))
                && !guarded[index.saturating_sub(WITHIN)..=*index]
                    .iter()
                    .any(|near| near.contains("snapshot.capture("))
        })
        .count();
    assert_eq!(still_found, 0, "a guarded delete must not be flagged");
}
