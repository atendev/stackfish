//! `CardSpec.db_script_id` and `ScenarioFile.card_db` pull a real `CardDefinition` out of a
//! card database by its `script_id` slug.

use rusqlite::Connection;
use stackfish_engine::{CardTypeSet, KeywordSet, PTValue, Subtype};
use stackfish_scenario::{build_initial_state, run, ScenarioFile};
use std::path::{Path, PathBuf};

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

    INSERT INTO keywords (name) VALUES ('Trample');
    INSERT INTO card_keywords (oracle_id, keyword_id) VALUES ('bear-1', (SELECT id FROM keywords WHERE name = 'Trample'));

    -- `phase5/db-script-operation-fix` regression fixture: a Sorcery whose
    -- only ability is a plain `s.operation()` (CR 608.2m), the same
    -- convention Balance's real `card_scripts` row uses. Free cost, no
    -- `[[cards]]` TOML `script` field anywhere - `db_script_id` is the only
    -- way this card's `CardDefinition` gets built, which is exactly the path
    -- that used to leave `effect: None` unregistered.
    INSERT INTO cards (oracle_id, script_id, name, mana_cost, type_line, oracle_text, has_multiple_faces)
    VALUES ('bolt-1', 'test_gain_life_sorcery', 'Test Gain Life', '', 'Sorcery', '', 0);
    INSERT INTO card_scripts (oracle_id, version, script_source, is_active) VALUES (
        'bolt-1', 1,
        'local s = {}
         function s.operation()
             return Game.Ops.GainLife(3)
         end
         return s',
        1
    );
    ";

fn write_fixture_db(path: &Path) {
    let conn = Connection::open(path).unwrap();
    conn.execute_batch(FIXTURE_SCHEMA_AND_DATA).unwrap();
}

fn unique_temp_path(name: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU32, Ordering};
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("stackfish-scenario-carddb-test-{}-{n}-{name}", std::process::id()))
}

fn fixture_db_path() -> PathBuf {
    let path = unique_temp_path("cards.sqlite");
    write_fixture_db(&path);
    path
}

#[test]
fn db_script_id_pulls_a_real_card_definition_into_a_scenario() {
    let db_path = fixture_db_path();
    let toml = format!(
        r#"
        seed = 1
        card_db = {db_path:?}

        [[players]]
        id = 0
        life = 20

        [[players]]
        id = 1
        life = 20

        [[cards]]
        key = "bear"
        db_script_id = "grizzly_bears"

        [[setup]]
        player = 0
        zone = "battlefield"
        card = "bear"
        "#
    );
    let file: ScenarioFile = toml::from_str(&toml).unwrap();
    let (state, card_ids, ..) = build_initial_state(&file);

    let object_id = card_ids["bear"];
    let definition = &state.objects[&object_id].definition;

    assert_eq!(definition.name, "Grizzly Bears");
    assert_eq!(definition.card_types, CardTypeSet::CREATURE);
    assert_eq!(definition.subtypes, vec![Subtype("Bear".to_string())]);
    assert!(matches!(definition.power, Some(PTValue::Fixed(2))));
    assert!(matches!(definition.toughness, Some(PTValue::Fixed(2))));
    assert_eq!(definition.keywords, KeywordSet::TRAMPLE);

    std::fs::remove_file(&db_path).ok();
}

// CR 608.2m
// A DB card has no TOML `script`, so its `operation()` must still reach `CardDefinition.effect`.
#[test]
fn db_script_id_cards_own_operation_function_is_registered_and_runs() {
    let db_path = fixture_db_path();
    let toml = format!(
        r#"
        seed = 1
        card_db = {db_path:?}

        [[players]]
        id = 0
        life = 20

        [[players]]
        id = 1
        life = 20

        [[cards]]
        key = "bolt"
        db_script_id = "test_gain_life_sorcery"

        [[setup]]
        player = 0
        zone = "hand"
        card = "bolt"

        [[steps]]
        type = "pass_priority"
        player = 0
        [[steps]]
        type = "pass_priority"
        player = 1
        [[steps]]
        type = "pass_priority"
        player = 0
        [[steps]]
        type = "pass_priority"
        player = 1

        [[steps]]
        type = "cast_spell"
        player = 0
        card = "bolt"
        targets = []

        [[steps]]
        type = "submit_payment"
        player = 0

        [[steps]]
        type = "pass_priority"
        player = 0
        [[steps]]
        type = "pass_priority"
        player = 1

        [[steps]]
        type = "assert_life"
        player = 0
        equals = 23
        "#
    );
    let file: ScenarioFile = toml::from_str(&toml).unwrap();
    let report = run(&file);
    assert!(!report.has_hard_failures());

    std::fs::remove_file(&db_path).ok();
}

#[test]
#[should_panic(expected = "no top-level card_db path set")]
fn db_script_id_without_card_db_panics_clearly() {
    let toml = r#"
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
        "#;
    let file: ScenarioFile = toml::from_str(toml).unwrap();
    let _ = build_initial_state(&file);
}

// Uses `load()` and not `toml::from_str`, which never sets `base_dir`.
#[test]
fn relative_card_db_path_resolves_against_scenario_file_directory_not_cwd() {
    let dir = unique_temp_path("relative-dir");
    std::fs::create_dir_all(&dir).unwrap();
    write_fixture_db(&dir.join("cards.sqlite"));

    let scenario_path = dir.join("scenario.toml");
    std::fs::write(
        &scenario_path,
        r#"
        seed = 1
        card_db = "cards.sqlite"

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
        "#,
    )
    .unwrap();

    let file = stackfish_scenario::load(&scenario_path);
    let (state, card_ids, ..) = build_initial_state(&file);
    let definition = &state.objects[&card_ids["bear"]].definition;
    assert_eq!(definition.name, "Grizzly Bears");

    std::fs::remove_dir_all(&dir).ok();
}
