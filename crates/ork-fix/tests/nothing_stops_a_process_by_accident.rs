//! No test may reach the code that ends a real program.
//!
//! This exists because it already happened. A test written to check that
//! `Steam.exe` and `steam.exe` are treated as one program called the function
//! that judges *and acts*, with a made-up survey and a real process identifier
//! -- so on a machine where that identifier existed, running the test suite
//! would have stopped whatever was wearing it. It passed, because the number
//! chosen happened to be free.
//!
//! That is the shape of the fault: a test that means to check a comparison
//! reaches a kill, and the only thing standing between the suite and somebody
//! losing work is which numbers were free that day. Nothing in the type system
//! catches it, and it reads as a perfectly ordinary test.
//!
//! So the judging is separate from the acting, and this checks that it stays
//! that way: exactly one place calls the thing that stops a process, and it is
//! not in a test.

use std::path::{Path, PathBuf};

/// The call that ends a program.
const THE_ACT: &str = "stop_process(";

fn source() -> String {
    let path: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/processes.rs");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("could not read {}: {error}", path.display()))
}

/// The file with its `#[cfg(test)]` module removed.
fn without_tests(text: &str) -> String {
    let mut kept = Vec::new();
    let mut skipping = false;
    for line in text.lines() {
        if !skipping && line == "#[cfg(test)]" {
            skipping = true;
            continue;
        }
        if skipping {
            if line == "}" {
                skipping = false;
            }
            continue;
        }
        kept.push(line);
    }
    assert!(
        !skipping,
        "a #[cfg(test)] item was never closed at the margin"
    );
    kept.join("\n")
}

fn calls(text: &str) -> Vec<&str> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.starts_with("//") && !line.starts_with("///"))
        .filter(|line| line.contains(THE_ACT))
        .collect()
}

#[test]
fn only_one_place_stops_a_process() {
    // More than one caller is more than one place where the judging could be
    // skipped, and the judging is the whole of the safety here.
    let program = without_tests(&source());
    let found = calls(&program);
    assert_eq!(
        found.len(),
        1,
        "`{THE_ACT}` should be called from exactly one place and was called from \
         {}:\n  {}\n\nEverything that ends a process goes through the same \
         judgement, or the judgement is optional.",
        found.len(),
        found.join("\n  ")
    );
}

#[test]
fn no_test_in_this_crate_reaches_it() {
    // The failure this file exists for. A test that calls the acting function
    // with a made-up survey will happily act on the real machine, because the
    // survey is the only made-up part.
    let text = source();
    let program = without_tests(&text);
    let in_tests = calls(&text).len() - calls(&program).len();
    assert_eq!(
        in_tests, 0,
        "a test calls `{THE_ACT}`, which stops a real process on the machine \
         running the tests. Test `judge` instead: it answers whether something \
         may be stopped and touches nothing."
    );
}

#[test]
fn the_judging_can_be_tested_without_the_acting() {
    // The reason the split is worth keeping. If `judge` disappeared, the only
    // way to test any of this would be to call the acting function, and the
    // check above would have to be deleted to let the tests run.
    let program = without_tests(&source());
    assert!(
        program.contains("fn judge(target: &Target, survey: &Survey)"),
        "`judge` is gone, so there is no way to test the rules without ending \
         a real program to find out what they decided"
    );
    // And it is what the tests actually use, so the split is not decorative.
    let text = source();
    assert!(
        text.contains("judge(\n") || text.contains("judge("),
        "nothing calls `judge`"
    );
}

#[test]
fn the_scan_is_looking_at_the_right_file() {
    // Without this, a moved or renamed file makes all three above pass by
    // finding nothing -- which reads as "no test stops a process" and would be
    // true only in the sense that nothing was read at all.
    let text = source();
    assert!(
        text.contains("pub fn stop_these("),
        "processes.rs no longer has the function this file was written about"
    );
    assert!(
        text.contains("#[cfg(test)]"),
        "processes.rs has no tests, so the check above proves nothing"
    );
    assert!(
        text.len() > 4_000,
        "processes.rs came back suspiciously short"
    );
}
