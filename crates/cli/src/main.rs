//! Scenario-file runner. `cargo run -p stackfish-cli -- path/to/scenario.toml`
//! loads, runs, and prints a pass/fail report for one scenario file — see
//! `stackfish_scenario` for the scenario format and execution model.
//!
//! No live/interactive REPL mode yet — file-driven covers both a human
//! (write/edit a .toml, rerun) and an AI agent (run via shell, read stdout)
//! without needing real interactivity. Revisit if live typed-command
//! exploration turns out to be wanted on top of this.

use std::path::PathBuf;
use std::process::ExitCode;

use stackfish_scenario::{AssertionOutcome, EntryOutcome, StepOutcome};

fn main() -> ExitCode {
    let path = match std::env::args().nth(1) {
        Some(p) => PathBuf::from(p),
        None => {
            eprintln!("usage: stackfish-cli <scenario.toml>");
            return ExitCode::FAILURE;
        }
    };

    println!("Running scenario: {}", path.display());
    let report = stackfish_scenario::run_path(&path);

    println!("\n-- steps --");
    let mut hard_failures = 0;
    for (i, entry) in report.entries.iter().enumerate() {
        match &entry.outcome {
            EntryOutcome::Action(StepOutcome::Ran) => {
                println!("  [{i}] action | ran   - {}", entry.description);
            }
            EntryOutcome::Action(StepOutcome::NotImplemented(msg)) => {
                println!("  [{i}] action | not yet implemented - {} ({msg})", entry.description);
            }
            EntryOutcome::Action(StepOutcome::Errored(msg)) => {
                println!("  [{i}] action | errored - {} ({msg})", entry.description);
            }
            EntryOutcome::Assertion(AssertionOutcome::Passed) => {
                println!("  [{i}] assert | PASS  - {}", entry.description);
            }
            EntryOutcome::Assertion(AssertionOutcome::Failed { expected, actual }) => {
                println!(
                    "  [{i}] assert | FAIL  - {} (expected {expected}, got {actual})",
                    entry.description
                );
                hard_failures += 1;
            }
        }
    }

    if hard_failures > 0 {
        println!("\n{hard_failures} assertion(s) failed.");
        ExitCode::FAILURE
    } else {
        println!("\nNo hard failures (unimplemented actions are expected pre-Phase 7).");
        ExitCode::SUCCESS
    }
}
