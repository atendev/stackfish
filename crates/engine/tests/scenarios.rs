//! Runs every scenario file under `tests/scenarios/` and fails if any assertion is wrong. See `docs/scenario_format.md` and `ScenarioReport::has_hard_failures` for what counts as a hard failure.

use std::fs;
use std::path::{Path, PathBuf};

fn collect_toml_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in
        fs::read_dir(dir).unwrap_or_else(|e| panic!("failed to read {}: {e}", dir.display()))
    {
        let path = entry.expect("readable directory entry").path();
        if path.is_dir() {
            collect_toml_files(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("toml") {
            out.push(path);
        }
    }
}

#[test]
fn all_scenarios_have_no_hard_failures() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/scenarios");
    let mut scenario_files = Vec::new();
    collect_toml_files(&dir, &mut scenario_files);

    assert!(
        !scenario_files.is_empty(),
        "expected at least one scenario file under {}",
        dir.display()
    );

    for path in scenario_files {
        let report = stackfish_scenario::run_path(&path);
        assert!(
            !report.has_hard_failures(),
            "scenario {} had a hard assertion failure",
            path.display()
        );
    }
}
