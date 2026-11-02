//! Runs every scenario file under `tests/scenarios/` and lists each file with a hard failure. See `docs/scenario_format.md` and `ScenarioReport::has_hard_failures` for what counts as a hard failure.

use std::fs;
use std::panic::{self, AssertUnwindSafe};
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

fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        s.to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "non-string panic payload".to_string()
    }
}

// Load and setup panics escape `run_path`, so one bad file must not hide the rest.
fn failure_lines(path: &Path) -> Vec<String> {
    match panic::catch_unwind(AssertUnwindSafe(|| stackfish_scenario::run_path(path))) {
        Ok(report) => report.hard_failure_lines(),
        Err(payload) => vec![format!("panicked: {}", panic_message(payload))],
    }
}

#[test]
fn all_scenarios_have_no_hard_failures() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/scenarios");
    let mut scenario_files = Vec::new();
    collect_toml_files(&dir, &mut scenario_files);
    scenario_files.sort();

    assert!(
        !scenario_files.is_empty(),
        "expected at least one scenario file under {}",
        dir.display()
    );

    let mut failures = Vec::new();
    for path in &scenario_files {
        let lines = failure_lines(path);
        if !lines.is_empty() {
            let relative = path.strip_prefix(&dir).unwrap_or(path);
            failures.push(format!(
                "{}\n    {}",
                relative.display(),
                lines.join("\n").replace('\n', "\n    ")
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "{} of {} scenario files had hard failures:\n\n{}",
        failures.len(),
        scenario_files.len(),
        failures.join("\n\n")
    );
}
