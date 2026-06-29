//! Runs every scenario file in `tests/scenarios/*.toml` and fails the test
//! if any assertion produced a wrong answer. Actions that hit `todo!()`
//! (expected pre-Phase 7) are reported but don't fail the test — see
//! `stackfish_scenario::ScenarioReport::has_hard_failures`.
//!
//! This is the Phase 4 baseline: with zero implementation done yet, this
//! test passes because `tests/scenarios/basic_setup.toml` only exercises
//! setup + assertions (no actions), which doesn't touch any `todo!()` code
//! path at all. `pass_priority.toml` exercises the action path and is
//! expected to report "not implemented" until Phase 5/7 - that's a pass
//! here, not a failure, by design.

use std::fs;
use std::path::Path;

#[test]
fn all_scenarios_have_no_hard_failures() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/scenarios");
    let mut ran_any = false;

    for entry in fs::read_dir(&dir).expect("tests/scenarios directory should exist") {
        let entry = entry.expect("readable directory entry");
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("toml") {
            continue;
        }
        ran_any = true;

        let report = stackfish_scenario::run_path(&path);
        assert!(
            !report.has_hard_failures(),
            "scenario {} had a hard assertion failure",
            path.display()
        );
    }

    assert!(ran_any, "expected at least one scenario file in {}", dir.display());
}
