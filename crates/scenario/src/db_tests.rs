use std::panic::{self, AssertUnwindSafe};

use stackfish_carddb::{CardDb, CardTestRow};

use crate::load::parse;
use crate::report::{ScenarioReport, run};
use crate::run::panic_message;

/// One `card_tests` row and what happened when it ran.
pub struct DbTestOutcome {
    pub oracle_id: String,
    pub card_name: String,
    pub test_name: String,
    pub version: i64,
    pub result: DbTestResult,
}

pub enum DbTestResult {
    Report(ScenarioReport),
    /// `test_source` did not parse, or building or running it panicked. The panic is caught
    /// per test so one malformed row does not hide the others.
    Errored(String),
}

impl DbTestOutcome {
    /// A hard failure is an `Errored` build or run, or a report with a hard failure.
    pub fn has_hard_failures(&self) -> bool {
        match &self.result {
            DbTestResult::Report(report) => report.has_hard_failures(),
            DbTestResult::Errored(_) => true,
        }
    }
}

/// Runs every active `card_tests` row in `db_path` as its own scenario. Each test's
/// `card_db` is forced to `db_path`, so it resolves cards against the database it came from.
pub fn run_db_tests(db_path: &std::path::Path) -> Vec<DbTestOutcome> {
    let db = CardDb::open(db_path)
        .unwrap_or_else(|e| panic!("failed to open card_db {}: {e}", db_path.display()));
    let tests = db
        .active_tests()
        .unwrap_or_else(|e| panic!("failed to read card_tests from {}: {e}", db_path.display()));

    // Absolute, because there is no scenario file for a relative path to resolve against.
    let db_path = std::fs::canonicalize(db_path)
        .unwrap_or_else(|e| panic!("failed to canonicalize card_db {}: {e}", db_path.display()));
    let db_path_str = db_path.to_string_lossy().into_owned();

    tests
        .into_iter()
        .map(|row| run_one_db_test(&db_path_str, row))
        .collect()
}

fn run_one_db_test(db_path_str: &str, row: CardTestRow) -> DbTestOutcome {
    let CardTestRow {
        oracle_id,
        card_name,
        test_name,
        version,
        test_source,
    } = row;
    let outcome = panic::catch_unwind(AssertUnwindSafe(|| {
        let mut file = parse(&test_source);
        file.card_db = Some(db_path_str.to_string());
        run(&file)
    }));
    let result = match outcome {
        Ok(report) => DbTestResult::Report(report),
        Err(payload) => DbTestResult::Errored(panic_message(payload)),
    };
    DbTestOutcome {
        oracle_id,
        card_name,
        test_name,
        version,
        result,
    }
}
