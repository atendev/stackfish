//! Command-line runner for scenario files and the `card_tests` rows of a card database.
//! Scenario format and execution live in `stackfish_scenario`; see `docs/DESIGN.md`.

// TODO: interactive REPL mode; runs are file-driven only

use std::path::PathBuf;
use std::process::ExitCode;

use stackfish_scenario::{AssertionOutcome, DbTestResult, EntryOutcome, StepOutcome};

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let first = match args.next() {
        Some(a) => a,
        None => {
            eprintln!("usage: stackfish-cli <scenario.toml>");
            eprintln!("       stackfish-cli db-tests <cards.sqlite>");
            return ExitCode::FAILURE;
        }
    };

    if first == "db-tests" {
        let db_path = match args.next() {
            Some(p) => PathBuf::from(p),
            None => {
                eprintln!("usage: stackfish-cli db-tests <cards.sqlite>");
                return ExitCode::FAILURE;
            }
        };
        return run_db_tests(&db_path);
    }

    run_scenario(&PathBuf::from(first))
}

fn run_scenario(path: &std::path::Path) -> ExitCode {
    println!("Running scenario: {}", path.display());
    let report = stackfish_scenario::run_path(path);

    println!("\n-- steps --");
    let hard_failures = print_report(&report);

    if hard_failures > 0 {
        println!("\n{hard_failures} hard failure(s) (failed assertion or rejected action).");
        ExitCode::FAILURE
    } else {
        println!("\nNo hard failures (unimplemented actions are expected pre-Phase 7).");
        ExitCode::SUCCESS
    }
}

fn run_db_tests(db_path: &std::path::Path) -> ExitCode {
    println!("Running card_tests from: {}", db_path.display());
    let outcomes = stackfish_scenario::run_db_tests(db_path);

    if outcomes.is_empty() {
        println!("\nNo active card_tests rows found.");
        return ExitCode::SUCCESS;
    }

    let mut failed_tests = 0;
    for outcome in &outcomes {
        if !outcome.has_hard_failures() {
            continue;
        }
        failed_tests += 1;
        println!(
            "\n== {} :: {} (v{}, oracle_id={}) ==",
            outcome.card_name, outcome.test_name, outcome.version, outcome.oracle_id
        );
        match &outcome.result {
            DbTestResult::Errored(msg) => println!("  ERRORED - {msg}"),
            DbTestResult::Report(report) => {
                print_report(report);
            }
        }
    }

    let total = outcomes.len();
    println!(
        "\n{}/{total} test(s) had no hard failures.",
        total - failed_tests
    );
    if failed_tests > 0 {
        println!("{failed_tests} test(s) failed.");
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn print_report(report: &stackfish_scenario::ScenarioReport) -> usize {
    let mut hard_failures = 0;
    for (i, entry) in report.entries.iter().enumerate() {
        match &entry.outcome {
            EntryOutcome::Action(StepOutcome::Ran) if entry.expect_error => {
                println!(
                    "  [{i}] action | FAIL  - {} (expected rejection, but it ran)",
                    entry.description
                );
                hard_failures += 1;
            }
            EntryOutcome::Action(StepOutcome::Ran) => {
                println!("  [{i}] action | ran   - {}", entry.description);
            }
            EntryOutcome::Action(StepOutcome::NotImplemented(msg)) => {
                println!(
                    "  [{i}] action | not yet implemented - {} ({msg})",
                    entry.description
                );
            }
            EntryOutcome::Action(StepOutcome::Errored(msg)) if entry.expect_error => {
                println!(
                    "  [{i}] action | rejected as expected - {} ({msg})",
                    entry.description
                );
            }
            EntryOutcome::Action(StepOutcome::Errored(msg)) => {
                println!("  [{i}] action | ERRORED - {} ({msg})", entry.description);
                hard_failures += 1;
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
    hard_failures
}
