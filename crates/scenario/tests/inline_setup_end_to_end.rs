//! Inline `[[setup]]` card fields work end to end through the engine. These tests use `parse`,
//! which expands them.

use rusqlite::Connection;
use stackfish_scenario::{parse, run, AssertionOutcome, EntryOutcome};
use std::path::PathBuf;

fn assert_no_hard_failures(report: &stackfish_scenario::ScenarioReport) {
    for entry in &report.entries {
        match &entry.outcome {
            EntryOutcome::Assertion(AssertionOutcome::Failed { expected, actual }) => {
                panic!("assertion failed: {} (expected {expected}, got {actual})", entry.description)
            }
            EntryOutcome::Action(stackfish_scenario::StepOutcome::Errored(msg)) => {
                panic!("action errored: {} ({msg})", entry.description)
            }
            _ => {}
        }
    }
}

#[test]
fn inline_template_on_setup_builds_and_places_the_card() {
    let toml = r#"
        seed = 1

        [[players]]
        id = 0
        life = 20

        [[setup]]
        player = 0
        zone = "battlefield"
        card = "forest"
        template = "test_land_green"

        [[steps]]
        type = "tap_for_mana"
        player = 0
        source = "forest"

        [[steps]]
        type = "assert_mana_pool"
        player = 0
        color = "green"
        equals = 1
        "#;

    let report = run(&parse(toml));
    assert_no_hard_failures(&report);
}

#[test]
fn inline_handwritten_fields_on_setup_build_and_place_the_card() {
    let toml = r#"
        seed = 1

        [[players]]
        id = 0
        life = 20

        [[setup]]
        player = 0
        zone = "battlefield"
        card = "bear"
        name = "Test Bear"
        generic_cost = 1
        colors = ["G"]
        types = ["Creature"]
        power = 2
        toughness = 2

        [[steps]]
        type = "assert_object_zone"
        card = "bear"
        equals = "battlefield"
        "#;

    let report = run(&parse(toml));
    assert_no_hard_failures(&report);
}

#[test]
#[should_panic(expected = "already exists")]
fn inline_fields_colliding_with_an_existing_cards_key_panics_clearly() {
    let toml = r#"
        seed = 1

        [[players]]
        id = 0
        life = 20

        [[cards]]
        key = "bear"
        name = "Test Bear"
        types = ["Creature"]

        [[setup]]
        player = 0
        zone = "battlefield"
        card = "bear"
        power = 9
        "#;

    let _ = parse(toml);
}

const FIXTURE_SCHEMA_AND_DATA: &str = "
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
    INSERT INTO cards (oracle_id, script_id, name, mana_cost, type_line, oracle_text, power, toughness, has_multiple_faces)
    VALUES ('bear-1', 'grizzly_bears', 'Grizzly Bears', '{1}{G}', 'Creature — Bear', '', '2', '2', 0);
";

fn unique_temp_path(name: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU32, Ordering};
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("stackfish-scenario-inline-setup-test-{}-{n}-{name}", std::process::id()))
}

fn fixture_db_path() -> PathBuf {
    let path = unique_temp_path("cards.sqlite");
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(FIXTURE_SCHEMA_AND_DATA).unwrap();
    path
}

#[test]
fn inline_db_script_id_on_setup_pulls_the_real_card() {
    let db_path = fixture_db_path();
    let toml = format!(
        r#"
        seed = 1
        card_db = {db_path:?}

        [[players]]
        id = 0
        life = 20

        [[setup]]
        player = 0
        zone = "battlefield"
        card = "bear"
        db_script_id = "grizzly_bears"

        [[steps]]
        type = "assert_object_zone"
        card = "bear"
        equals = "battlefield"
        "#
    );

    let report = run(&parse(&toml));
    assert_no_hard_failures(&report);

    std::fs::remove_file(&db_path).ok();
}
