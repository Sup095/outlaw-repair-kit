//! The crash recorder, exercised by an actual crash.
//!
//! A panic hook cannot be tested in the process running the tests: installing
//! it replaces the harness's own hook for every other test, and a panic that
//! is caught is not the thing being tested anyway. So this test runs itself
//! again in a child process, tells the child to install the hook and fall
//! over, and then reads what the child left behind.
//!
//! Worth the trouble. This is the path that only ever runs when everything
//! else has already gone wrong, which is exactly the kind of code that quietly
//! stops working and is never noticed.

use std::path::PathBuf;

/// Set on the child. Its presence is what tells the test which half it is.
const MARKER: &str = "ORK_CRASH_TEST_STATE_DIR";
const MESSAGE: &str = "a deliberate crash, for the test";

#[test]
fn a_crash_is_recorded_where_a_report_can_find_it() {
    if let Ok(dir) = std::env::var(MARKER) {
        ork_core::incident::catch_crashes(PathBuf::from(dir));
        panic!("{MESSAGE}");
    }

    let dir = std::env::temp_dir().join(format!("ork-crash-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch directory");

    let binary = std::env::current_exe().expect("the test binary");
    let output = ork_core::unseen::Unseen::unseen(&mut std::process::Command::new(binary))
        .args([
            "a_crash_is_recorded_where_a_report_can_find_it",
            "--exact",
            "--test-threads=1",
            // Without this the harness swallows the crash message, and the
            // half of this test that checks it still reaches a terminal would
            // pass or fail for the wrong reason.
            "--nocapture",
        ])
        .env(MARKER, &dir)
        .output()
        .expect("the child test run");

    // The child is *meant* to fail: it crashed on purpose. What matters is
    // what it wrote on the way down.
    assert!(
        !output.status.success(),
        "the child was supposed to crash and did not"
    );

    let recorded = ork_core::incident::all(&dir);
    assert_eq!(
        recorded.len(),
        1,
        "expected exactly one crash, got {recorded:?}"
    );
    let crash = &recorded[0];
    assert_eq!(crash.kind, ork_core::incident::IncidentKind::Panic);
    assert_eq!(crash.message, MESSAGE);
    assert!(
        crash
            .location
            .as_deref()
            .is_some_and(|at| at.contains("crash.rs")),
        "a crash must say where it happened, got {:?}",
        crash.location
    );

    // And the usual message still reached the terminal: somebody watching a
    // crash happen should not have to go looking in a file to see what it
    // said.
    let printed = String::from_utf8_lossy(&output.stderr);
    assert!(
        printed.contains(MESSAGE),
        "the crash was recorded but not printed: {printed}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
