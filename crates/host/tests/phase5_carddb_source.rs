//! End-to-end host test using a real card database as the `CardSource`.
//!
//! This test builds a temp SQLite DB with a minimal real schema, constructs a
//! two-player setup, and confirms the Session resolves cards from it and runs
//! the first Decision.

use rusqlite::Connection;
use stackfish_host::{CardDbCardSource, Session};
use std::path::PathBuf;

fn fixture_db() -> PathBuf {
    use std::sync::atomic::{AtomicU32, Ordering};
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir()
        .join(format!("stackfish-host-carddb-test-{}-{n}.sqlite", std::process::id()));
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(
        "
        CREATE TABLE cards (
            oracle_id          TEXT PRIMARY KEY,
            script_id          TEXT UNIQUE,
            name               TEXT NOT NULL,
            mana_cost          TEXT,
            type_line          TEXT,
            oracle_text        TEXT,
            power              TEXT,
            toughness          TEXT,
            loyalty            TEXT,
            defense            TEXT,
            has_multiple_faces INTEGER NOT NULL DEFAULT 0
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

        -- Two vanilla creatures with no script, enough for a valid deck.
        INSERT INTO cards (oracle_id, script_id, name, mana_cost, type_line, oracle_text, power, toughness, has_multiple_faces)
        VALUES ('alpha-1', 'alpha', 'Alpha Bear', '{1}{G}', 'Creature — Bear', '', '2', '2', 0);

        INSERT INTO cards (oracle_id, script_id, name, mana_cost, type_line, oracle_text, power, toughness, has_multiple_faces)
        VALUES ('beta-1', 'beta', 'Beta Bear', '{2}{G}', 'Creature — Bear', '', '3', '3', 0);
        "
    )
    .unwrap();
    drop(conn);
    path
}

fn setup_request(seed: u64) -> Vec<u8> {
    let body = serde_json::json!({
        "players": [
            { "deck": [{ "card": "alpha-1", "count": 20 }, { "card": "beta-1", "count": 20 }] },
            { "deck": [{ "card": "beta-1", "count": 20 }, { "card": "alpha-1", "count": 20 }] },
        ],
        "format": "standard",
        "seed": seed.to_string(),
        "overrides": { "shuffle_libraries": false, "skip_mulligan": true },
    });
    serde_json::to_vec(&serde_json::json!({ "v": stackfish_host::PROTOCOL_VERSION, "kind": "setup", "body": body })
    )
    .unwrap()
}

#[test]
fn session_create_resolves_real_cards_from_db() {
    let db_path = fixture_db();
    let source = CardDbCardSource::open(&db_path).unwrap();
    let request = setup_request(1);
    let (mut session, response) = Session::create(&request, &source).expect("setup should succeed");

    let value: serde_json::Value = serde_json::from_slice(&response).unwrap();
    assert_eq!(value["body"]["status"], "created");
    assert_eq!(value["body"]["seats"], serde_json::json!([0, 1]));

    let advance = serde_json::to_vec(
        &serde_json::json!({ "v": stackfish_host::PROTOCOL_VERSION, "kind": "advance", "seat": 0, "body": {} })
    )
    .unwrap();
    let response = session.handle(&advance);
    let value: serde_json::Value = serde_json::from_slice(&response).unwrap();
    assert_eq!(value["body"]["status"], "decision_open");
    assert!(value["body"]["decision"].is_object());
}

#[test]
fn missing_db_card_returns_setup_error() {
    let db_path = fixture_db();
    let source = CardDbCardSource::open(&db_path).unwrap();
    let body = serde_json::json!({
        "players": [
            { "deck": [{ "card": "not-in-db", "count": 40 }] },
            { "deck": [{ "card": "alpha-1", "count": 40 }] },
        ],
        "format": "standard",
        "seed": "1",
    });
    let request = serde_json::to_vec(
        &serde_json::json!({ "v": stackfish_host::PROTOCOL_VERSION, "kind": "setup", "body": body })
    )
    .unwrap();

    let response = match Session::create(&request, &source) {
        Ok(_) => panic!("setup should be refused"),
        Err(response) => response,
    };
    let value: serde_json::Value = serde_json::from_slice(&response).unwrap();
    assert_eq!(value["body"]["status"], "error");
    assert!(value["body"]["error"]["message"].as_str().unwrap().contains("not-in-db"));
}
