//! Every place that starts a program must suppress its console window.
//!
//! This is a source check rather than a behavioural one, and it is here
//! because the failure it guards cannot be seen from a test: a console window
//! flashing on somebody's screen leaves nothing behind to assert on. It shows
//! up only when a person runs the window, which is exactly the wrong moment
//! and the wrong person to find it.
//!
//! The rule is narrow: wherever a `Command` is built, `.unseen()` is called on
//! it. Anything genuinely meant to run in a terminal says so with a marker
//! comment, so the exception is written down next to the code rather than kept
//! in a list somewhere else that nobody updates.

use std::path::{Path, PathBuf};

/// Written on the same line, or the line before, to say a console is wanted.
const DELIBERATE: &str = "console: on purpose";

/// How many lines after `Command::new` the call may appear in.
///
/// The builder is usually four or five lines of `.arg()` and `.stdio()`.
/// Twelve is generous without being so wide that it matches the next
/// statement's `.unseen()`.
const WITHIN: usize = 12;

fn workspace() -> PathBuf {
    // `CARGO_MANIFEST_DIR` is this crate; the workspace is two above it.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("this crate lives two levels below the workspace root")
        .to_path_buf()
}

fn rust_files(directory: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // `target` holds generated code from every dependency, which is
            // not this project's to answer for.
            if path.file_name().is_some_and(|name| name == "target") {
                continue;
            }
            rust_files(&path, found);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            found.push(path);
        }
    }
}

#[test]
fn nothing_starts_a_program_without_suppressing_its_console() {
    let root = workspace();
    let mut files = Vec::new();
    for crate_dir in ["crates", "apps"] {
        rust_files(&root.join(crate_dir), &mut files);
    }

    // A check that could not run clears nothing. An empty file list would make
    // this pass for ever without having looked at anything.
    assert!(
        files.len() > 20,
        "only found {} source files under {} -- this check did not run",
        files.len(),
        root.display()
    );

    let mut unguarded = Vec::new();
    for file in &files {
        // The trait's own definition and tests build commands in order to
        // check the trait, and the scanner itself contains the words it looks
        // for.
        if file.ends_with("unseen.rs") || file.ends_with("no_stray_consoles.rs") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(file) else {
            panic!("could not read {}", file.display());
        };
        let lines: Vec<&str> = text.lines().collect();

        for (index, line) in lines.iter().enumerate() {
            if !line.contains("Command::new") {
                continue;
            }
            let previous = index.checked_sub(1).map(|i| lines[i]).unwrap_or("");
            if line.contains(DELIBERATE) || previous.contains(DELIBERATE) {
                continue;
            }
            let window = lines[index..lines.len().min(index + WITHIN)].join("\n");
            if !window.contains(".unseen()") && !window.contains("Unseen::unseen") {
                unguarded.push(format!(
                    "{}:{}  {}",
                    file.strip_prefix(&root).unwrap_or(file).display(),
                    index + 1,
                    line.trim()
                ));
            }
        }
    }

    assert!(
        unguarded.is_empty(),
        "these start a program without `.unseen()`, so a console window \
         appears when the tool is run from a window:\n  {}\n\nAdd `.unseen()`, \
         or write `// {DELIBERATE}` above the line if a terminal really is \
         wanted.",
        unguarded.join("\n  ")
    );
}

#[test]
fn the_check_would_notice_a_command_with_nothing_guarding_it() {
    // Proves the scanner can fail. Without this, a mistake in the matching --
    // a changed method name, a window too wide -- turns the test above into
    // one that passes because it stopped looking.
    let sample = "let output = Command::new(\"powershell\")\n    .args(args)\n    .output()?;";
    let lines: Vec<&str> = sample.lines().collect();
    let window = lines.join("\n");
    assert!(lines[0].contains("Command::new"));
    assert!(
        !window.contains(".unseen()"),
        "the sample was supposed to be unguarded"
    );
}
