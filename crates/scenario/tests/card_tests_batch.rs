//! `run_db_tests` runs every active `card_tests` row as its own scenario against the cards in
//! the same SQLite file.

use rusqlite::Connection;
use stackfish_scenario::DbTestResult;
use std::path::{Path, PathBuf};

const SCHEMA: &str = "
    CREATE TABLE cards (
        oracle_id          TEXT PRIMARY KEY,
        script_id          TEXT UNIQUE,
        name               TEXT NOT NULL,
        mana_cost          TEXT,
        type_line          TEXT,
        oracle_text        TEXT,
        power              TEXT,
        toughness           TEXT,
        loyalty             TEXT,
        defense             TEXT,
        has_multiple_faces  INTEGER NOT NULL DEFAULT 0
    );
    CREATE TABLE keywords (
        id   INTEGER PRIMARY KEY AUTOINCREMENT,
        name TEXT NOT NULL UNIQUE
    );
    CREATE TABLE card_keywords (
        oracle_id  TEXT NOT NULL,
        keyword_id INTEGER NOT NULL,
        PRIMARY KEY (oracle_id, keyword_id)
    );
    CREATE TABLE card_scripts (
        id            INTEGER PRIMARY KEY AUTOINCREMENT,
        oracle_id     TEXT NOT NULL,
        version       INTEGER NOT NULL,
        script_source TEXT NOT NULL,
        is_active     INTEGER NOT NULL DEFAULT 0
    );
    CREATE TABLE card_tests (
        id          INTEGER PRIMARY KEY AUTOINCREMENT,
        oracle_id   TEXT NOT NULL,
        test_name   TEXT NOT NULL DEFAULT 'default',
        version     INTEGER NOT NULL,
        test_source TEXT NOT NULL,
        is_active   INTEGER NOT NULL DEFAULT 1
    );

    INSERT INTO cards (oracle_id, script_id, name, mana_cost, type_line, oracle_text, power, toughness, has_multiple_faces)
    VALUES ('bear-1', 'grizzly_bears', 'Grizzly Bears', '{1}{G}', 'Creature — Bear', '', '2', '2', 0);
    ";

const PASSING_TEST_SOURCE: &str = r#"
    seed = 1

    [[players]]
    id = 0
    life = 20

    [[cards]]
    key = "bear"
    db_script_id = "grizzly_bears"

    [[setup]]
    player = 0
    zone = "battlefield"
    card = "bear"

    [[assertions]]
    type = "life"
    player = 0
    equals = 20
    "#;

const FAILING_TEST_SOURCE: &str = r#"
    seed = 1

    [[players]]
    id = 0
    life = 20

    [[cards]]
    key = "bear"
    db_script_id = "grizzly_bears"

    [[setup]]
    player = 0
    zone = "battlefield"
    card = "bear"

    [[assertions]]
    type = "life"
    player = 0
    equals = 99
    "#;

fn unique_temp_path(name: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU32, Ordering};
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("stackfish-scenario-card-tests-{}-{n}-{name}", std::process::id()))
}

fn open_fixture_db(path: &Path) -> Connection {
    let conn = Connection::open(path).unwrap();
    conn.execute_batch(SCHEMA).unwrap();
    conn
}

#[test]
fn run_db_tests_runs_every_active_row_against_real_card_data() {
    let db_path = unique_temp_path("cards.sqlite");
    let conn = open_fixture_db(&db_path);
    conn.execute(
        "INSERT INTO card_tests (oracle_id, test_name, version, test_source, is_active) \
         VALUES ('bear-1', 'default', 1, ?1, 1)",
        [PASSING_TEST_SOURCE],
    )
    .unwrap();
    drop(conn);

    let outcomes = stackfish_scenario::run_db_tests(&db_path);
    assert_eq!(outcomes.len(), 1);
    let outcome = &outcomes[0];
    assert_eq!(outcome.card_name, "Grizzly Bears");
    assert_eq!(outcome.test_name, "default");
    assert!(!outcome.has_hard_failures());
    match &outcome.result {
        DbTestResult::Report(report) => assert!(!report.has_hard_failures()),
        DbTestResult::Errored(msg) => panic!("expected a report, got an error: {msg}"),
    }

    std::fs::remove_file(&db_path).ok();
}

#[test]
fn run_db_tests_reports_a_failing_assertion_as_a_hard_failure() {
    let db_path = unique_temp_path("cards.sqlite");
    let conn = open_fixture_db(&db_path);
    conn.execute(
        "INSERT INTO card_tests (oracle_id, test_name, version, test_source, is_active) \
         VALUES ('bear-1', 'default', 1, ?1, 1)",
        [FAILING_TEST_SOURCE],
    )
    .unwrap();
    drop(conn);

    let outcomes = stackfish_scenario::run_db_tests(&db_path);
    assert_eq!(outcomes.len(), 1);
    assert!(outcomes[0].has_hard_failures());

    std::fs::remove_file(&db_path).ok();
}

// Inactive rows are skipped, and only the highest active `version` of a test runs.
#[test]
fn run_db_tests_skips_inactive_rows_and_picks_the_highest_active_version() {
    let db_path = unique_temp_path("cards.sqlite");
    let conn = open_fixture_db(&db_path);
    conn.execute(
        "INSERT INTO card_tests (oracle_id, test_name, version, test_source, is_active) \
         VALUES ('bear-1', 'default', 1, ?1, 1)",
        [FAILING_TEST_SOURCE],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO card_tests (oracle_id, test_name, version, test_source, is_active) \
         VALUES ('bear-1', 'default', 2, ?1, 1)",
        [PASSING_TEST_SOURCE],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO card_tests (oracle_id, test_name, version, test_source, is_active) \
         VALUES ('bear-1', 'default', 3, ?1, 0)",
        [FAILING_TEST_SOURCE],
    )
    .unwrap();
    drop(conn);

    let outcomes = stackfish_scenario::run_db_tests(&db_path);
    assert_eq!(outcomes.len(), 1, "only one (oracle_id, test_name) group - the winning version");
    assert_eq!(outcomes[0].version, 2);
    assert!(!outcomes[0].has_hard_failures());

    std::fs::remove_file(&db_path).ok();
}

#[test]
fn run_db_tests_reports_a_malformed_test_source_as_errored_without_aborting_the_batch() {
    let db_path = unique_temp_path("cards.sqlite");
    let conn = open_fixture_db(&db_path);
    conn.execute(
        "INSERT INTO card_tests (oracle_id, test_name, version, test_source, is_active) \
         VALUES ('bear-1', 'broken', 1, 'this is not valid toml [[[', 1)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO card_tests (oracle_id, test_name, version, test_source, is_active) \
         VALUES ('bear-1', 'default', 1, ?1, 1)",
        [PASSING_TEST_SOURCE],
    )
    .unwrap();
    drop(conn);

    let outcomes = stackfish_scenario::run_db_tests(&db_path);
    assert_eq!(outcomes.len(), 2);

    let broken = outcomes.iter().find(|o| o.test_name == "broken").unwrap();
    assert!(broken.has_hard_failures());
    assert!(matches!(broken.result, DbTestResult::Errored(_)));

    let default = outcomes.iter().find(|o| o.test_name == "default").unwrap();
    assert!(!default.has_hard_failures());

    std::fs::remove_file(&db_path).ok();
}
