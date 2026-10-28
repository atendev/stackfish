use std::panic::{self, AssertUnwindSafe};

use stackfish_carddb::{CardDb, CardTestRow};

use crate::load::parse;
use crate::report::{ScenarioReport, run};
use crate::run::panic_message;

/// One `card_tests` row after being run - identifies which card/test it was
/// (`CardTestRow`'s own fields) alongside what happened.
pub struct DbTestOutcome {
    pub oracle_id: String,
    pub card_name: String,
    pub test_name: String,
    pub version: i64,
    pub result: DbTestResult,
}

pub enum DbTestResult {
    Report(ScenarioReport),
    /// `test_source` didn't parse as a scenario TOML, or building/running it
    /// panicked (e.g. a bad `db_script_id`) - caught per-test via
    /// `catch_unwind` rather than aborting the whole batch, since these rows
    /// are independent and one malformed test shouldn't hide the results of
    /// every other one.
    Errored(String),
}

impl DbTestOutcome {
    /// A hard failure is either an `Errored` build/run, or a `Report` that
    /// itself has a hard assertion failure - same two-tier distinction
    /// `ScenarioReport::has_hard_failures` already draws between "ran and
    /// got the wrong answer" and "not yet implemented".
    pub fn has_hard_failures(&self) -> bool {
        match &self.result {
            DbTestResult::Report(report) => report.has_hard_failures(),
            DbTestResult::Errored(_) => true,
        }
    }
}

/// Opens `db_path`, reads every active `card_tests` row
/// (`CardDb::active_tests`), and runs each `test_source` as its own scenario
/// - the same `run` this module uses for a file-based scenario, just sourced
/// from the DB instead of `tests/scenarios/`. Every test's `card_db` is
/// forced to `db_path` regardless of what (if anything) `test_source` itself
/// sets, so a DB-stored test always resolves real cards against the exact
/// database it was read from - see `docs/scenario_format.md`'s `card_db`
/// field for what that path feeds.
pub fn run_db_tests(db_path: &std::path::Path) -> Vec<DbTestOutcome> {
    let db = CardDb::open(db_path)
        .unwrap_or_else(|e| panic!("failed to open card_db {}: {e}", db_path.display()));
    let tests = db
        .active_tests()
        .unwrap_or_else(|e| panic!("failed to read card_tests from {}: {e}", db_path.display()));

    // Absolute up front so `resolve_card_db_path` never needs `base_dir` -
    // there's no scenario *file* here to resolve a relative path against.
    let db_path = std::fs::canonicalize(db_path)
        .unwrap_or_else(|e| panic!("failed to canonicalize card_db {}: {e}", db_path.display()));
    let db_path_str = db_path.to_string_lossy().into_owned();

    tests.into_iter().map(|row| run_one_db_test(&db_path_str, row)).collect()
}

fn run_one_db_test(db_path_str: &str, row: CardTestRow) -> DbTestOutcome {
    let CardTestRow { oracle_id, card_name, test_name, version, test_source } = row;
    let outcome = panic::catch_unwind(AssertUnwindSafe(|| {
        let mut file = parse(&test_source);
        file.card_db = Some(db_path_str.to_string());
        run(&file)
    }));
    let result = match outcome {
        Ok(report) => DbTestResult::Report(report),
        Err(payload) => DbTestResult::Errored(panic_message(payload)),
    };
    DbTestOutcome { oracle_id, card_name, test_name, version, result }
}

