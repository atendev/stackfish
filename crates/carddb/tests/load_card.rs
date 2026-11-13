//! End-to-end tests against a fixture SQLite DB holding only the tables the loader reads.

use rusqlite::Connection;
use stackfish_carddb::{CardDb, CardLoadError, NonGoalKind};
use stackfish_engine::{CardTypeSet, KeywordSet, PTValue, ScriptReference, Subtype, SupertypeSet};
use std::path::PathBuf;

// `CardDb::open` takes a path, so the fixture is a temp file. The process id and a counter
// keep parallel tests off each other's file.
fn fixture_db_path() -> PathBuf {
    use std::sync::atomic::{AtomicU32, Ordering};
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "stackfish-carddb-test-{}-{n}.sqlite",
        std::process::id()
    ));
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

        INSERT INTO cards (oracle_id, script_id, name, mana_cost, type_line, oracle_text, power, toughness, has_multiple_faces)
        VALUES ('bear-1', 'grizzly_bears', 'Grizzly Bears', '{1}{G}', 'Creature — Bear', '', '2', '2', 0);

        INSERT INTO cards (oracle_id, script_id, name, mana_cost, type_line, oracle_text, power, toughness, has_multiple_faces)
        VALUES ('bolt-1', 'lightning_bolt', 'Lightning Bolt', '{R}', 'Instant', 'Lightning Bolt deals 3 damage to any target.', NULL, NULL, 0);

        INSERT INTO keywords (name) VALUES ('Flying'), ('Trample'), ('Cumulative upkeep');
        INSERT INTO card_keywords (oracle_id, keyword_id) VALUES ('bolt-1', (SELECT id FROM keywords WHERE name = 'Flying'));
        INSERT INTO card_keywords (oracle_id, keyword_id) VALUES ('bolt-1', (SELECT id FROM keywords WHERE name = 'Cumulative upkeep'));

        INSERT INTO card_scripts (oracle_id, version, script_source, is_active)
        VALUES ('bolt-1', 1, 'return { register_abilities = function(c) end }', 1);

        INSERT INTO cards (oracle_id, name, mana_cost, type_line, oracle_text, has_multiple_faces)
        VALUES ('dfc-1', 'Delver of Secrets // Insectile Aberration', NULL, 'Creature — Human Wizard', '', 1);

        INSERT INTO cards (oracle_id, name, mana_cost, type_line, oracle_text, has_multiple_faces)
        VALUES ('plane-1', 'Rabiah, City of Genies', NULL, 'Plane — Arabian Nights', '', 0);

        INSERT INTO cards (oracle_id, name, mana_cost, type_line, oracle_text, power, toughness, has_multiple_faces)
        VALUES ('tarmogoyf-1', 'Tarmogoyf', '{1}{G}', 'Creature — Lhurgoyf', '*/1+*', '*', '1+*', 0);

        INSERT INTO cards (oracle_id, name, mana_cost, type_line, oracle_text, loyalty, has_multiple_faces)
        VALUES ('pw-1', 'Jace, the Mind Sculptor', '{2}{U}{U}', 'Legendary Planeswalker — Jace', '', '3', 0);

        INSERT INTO cards (oracle_id, name, mana_cost, type_line, oracle_text, loyalty, has_multiple_faces)
        VALUES ('pw-art-1', 'The Aetherspark', '{4}', 'Legendary Artifact Planeswalker — Equipment', '', '4', 0);

        INSERT INTO cards (oracle_id, name, mana_cost, type_line, oracle_text, loyalty, has_multiple_faces)
        VALUES ('battle-1', 'Occupation of Kulrath', '{3}{R}', 'Battle — Control Point', '', NULL, 0);

        INSERT INTO cards (oracle_id, name, mana_cost, type_line, oracle_text, loyalty, has_multiple_faces)
        VALUES ('saga-1', 'The Eldest Reborn', '{4}{B}', 'Enchantment — Saga', '', NULL, 0);

        INSERT INTO cards (oracle_id, name, mana_cost, type_line, oracle_text, loyalty, has_multiple_faces)
        VALUES ('class-1', 'Artificer Class', '{1}{U}', 'Enchantment — Class', '', NULL, 0);

        INSERT INTO cards (oracle_id, name, mana_cost, type_line, oracle_text, loyalty, has_multiple_faces)
        VALUES ('morph-1', 'Venomspout Brackus', '{3}{G}{G}', 'Creature — Beast', '', NULL, 0);

        INSERT INTO cards (oracle_id, name, mana_cost, type_line, oracle_text, loyalty, has_multiple_faces)
        VALUES ('mega-1', 'Ire Shaman', '{1}{R}', 'Creature — Orc Shaman', '', NULL, 0);

        INSERT INTO cards (oracle_id, name, mana_cost, type_line, oracle_text, loyalty, has_multiple_faces)
        VALUES ('disguise-1', 'Mistway Spy', '{2}{U}', 'Creature — Merfolk Detective', '', NULL, 0);

        INSERT INTO cards (oracle_id, name, mana_cost, type_line, oracle_text, loyalty, has_multiple_faces)
        VALUES ('cloak-1', 'Veiled Ascension', '{2}{U}', 'Enchantment', '', NULL, 0);

        INSERT INTO cards (oracle_id, name, mana_cost, type_line, oracle_text, loyalty, has_multiple_faces)
        VALUES ('dread-1', 'Manifest Dread', '{1}{G}', 'Sorcery', '', NULL, 0);

        INSERT INTO cards (oracle_id, name, mana_cost, type_line, oracle_text, loyalty, has_multiple_faces)
        VALUES ('both-1', 'Roalesk, Prime Specimen', '{2}{G}{U}', 'Legendary Creature — Human Mutant', '', NULL, 0);

        INSERT INTO cards (oracle_id, name, mana_cost, type_line, oracle_text, loyalty, has_multiple_faces)
        VALUES ('meld-1', 'Bruna, the Fading Light', '{5}{W}{W}', 'Legendary Creature — Angel Horror', '', NULL, 0);

        INSERT INTO cards (oracle_id, name, mana_cost, type_line, oracle_text, loyalty, has_multiple_faces)
        VALUES ('ixidron-1', 'Ixidron', '{3}{U}{U}', 'Creature — Illusion', '', NULL, 0);

        INSERT INTO keywords (name) VALUES ('Morph'), ('Megamorph'), ('Disguise'), ('Cloak'), ('Manifest dread');
        INSERT INTO card_keywords (oracle_id, keyword_id) VALUES ('morph-1', (SELECT id FROM keywords WHERE name = 'Morph'));
        INSERT INTO card_keywords (oracle_id, keyword_id) VALUES ('mega-1', (SELECT id FROM keywords WHERE name = 'Megamorph'));
        INSERT INTO card_keywords (oracle_id, keyword_id) VALUES ('disguise-1', (SELECT id FROM keywords WHERE name = 'Disguise'));
        INSERT INTO card_keywords (oracle_id, keyword_id) VALUES ('both-1', (SELECT id FROM keywords WHERE name = 'Disguise'));
        INSERT INTO card_keywords (oracle_id, keyword_id) VALUES ('cloak-1', (SELECT id FROM keywords WHERE name = 'Cloak'));
        INSERT INTO card_keywords (oracle_id, keyword_id) VALUES ('both-1', (SELECT id FROM keywords WHERE name = 'Cloak'));
        INSERT INTO card_keywords (oracle_id, keyword_id) VALUES ('dread-1', (SELECT id FROM keywords WHERE name = 'Manifest dread'));
        ",
    )
    .unwrap();
    drop(conn);
    path
}

fn open_fixture() -> CardDb {
    CardDb::open(fixture_db_path()).unwrap()
}

#[test]
fn loads_vanilla_creature_by_name() {
    let db = open_fixture();
    let def = db.load_by_name("Grizzly Bears").unwrap();
    assert_eq!(def.name, "Grizzly Bears");
    assert_eq!(def.card_types, CardTypeSet::CREATURE);
    assert_eq!(def.subtypes, vec![Subtype("Bear".to_string())]);
    assert!(matches!(def.power, Some(PTValue::Fixed(2))));
    assert!(matches!(def.toughness, Some(PTValue::Fixed(2))));
    // No active script row, so the loader supplies the stub.
    match def.script_ref {
        ScriptReference::Inline(src) => assert!(src.contains("register_abilities")),
        ScriptReference::ByKey(_) => panic!("expected Inline"),
    }
    assert!(def.target_requirement.is_none());
    assert!(def.effect.is_none());
}

#[test]
fn loads_scripted_card_with_keywords_by_oracle_id() {
    let db = open_fixture();
    let def = db.load_by_oracle_id("bolt-1").unwrap();
    assert_eq!(def.name, "Lightning Bolt");
    assert_eq!(def.card_types, CardTypeSet::INSTANT);
    // "Cumulative upkeep" is not in KeywordSet, so only Flying survives.
    assert_eq!(def.keywords, KeywordSet::FLYING);
    match def.script_ref {
        ScriptReference::Inline(src) => assert!(src.contains("register_abilities")),
        ScriptReference::ByKey(_) => panic!("expected Inline"),
    }
}

#[test]
fn loads_card_by_script_id() {
    let db = open_fixture();
    let def = db.load_by_script_id("lightning_bolt").unwrap();
    assert_eq!(def.name, "Lightning Bolt");
    let by_oracle_id = db.load_by_oracle_id("bolt-1").unwrap();
    assert_eq!(
        def.definition_id, by_oracle_id.definition_id,
        "same card, same id regardless of lookup key"
    );
}

#[test]
fn definition_id_is_stable_across_loads() {
    let db = open_fixture();
    let a = db.load_by_oracle_id("bear-1").unwrap();
    let b = db.load_by_oracle_id("bear-1").unwrap();
    assert_eq!(a.definition_id, b.definition_id);
}

#[test]
fn oracle_id_is_the_row_key_whatever_the_lookup() {
    let db = open_fixture();
    assert_eq!(
        db.load_by_oracle_id("bear-1").unwrap().oracle_id.as_deref(),
        Some("bear-1")
    );
    assert_eq!(
        db.load_by_script_id("lightning_bolt")
            .unwrap()
            .oracle_id
            .as_deref(),
        Some("bolt-1")
    );
}

#[test]
fn multi_face_card_is_rejected() {
    let db = open_fixture();
    let err = db.load_by_oracle_id("dfc-1").unwrap_err();
    assert!(matches!(err, stackfish_carddb::CardLoadError::MultiFace(_)));
}

#[test]
fn unsupported_casual_format_type_is_rejected() {
    let db = open_fixture();
    let err = db.load_by_oracle_id("plane-1").unwrap_err();
    assert!(matches!(err, stackfish_carddb::CardLoadError::TypeLine(_)));
}

#[test]
fn not_found_is_a_typed_error() {
    let db = open_fixture();
    let err = db
        .load_by_name("Some Card That Does Not Exist")
        .unwrap_err();
    assert!(matches!(err, stackfish_carddb::CardLoadError::NotFound(_)));
}

// CR 613.4a
#[test]
fn cda_power_and_toughness_have_no_script_yet() {
    // The DB has no CDA function column, so `script` is always None.
    let db = open_fixture();
    let def = db.load_by_oracle_id("tarmogoyf-1").unwrap();
    match def.power {
        Some(PTValue::CharacteristicDefining { display, script }) => {
            assert_eq!(display, "*");
            assert!(script.is_none());
        }
        other => panic!("expected CharacteristicDefining, got {other:?}"),
    }
    match def.toughness {
        Some(PTValue::CharacteristicDefining { display, .. }) => assert_eq!(display, "1+*"),
        other => panic!("expected CharacteristicDefining, got {other:?}"),
    }
    // The fixture row has a NULL loyalty, which must not read as "*".
    assert!(def.loyalty.is_none());
    assert_eq!(def.supertypes, SupertypeSet::empty());
}

fn non_goal_kind(db: &CardDb, oracle_id: &str) -> NonGoalKind {
    match db.load_by_oracle_id(oracle_id).unwrap_err() {
        CardLoadError::NonGoal { kind, .. } => kind,
        other => panic!("expected NonGoal, got {other:?}"),
    }
}

#[test]
fn non_goal_kinds_are_rejected_with_their_kind() {
    let db = open_fixture();
    for (oracle_id, kind) in [
        ("pw-1", NonGoalKind::Planeswalker),
        ("battle-1", NonGoalKind::Battle),
        ("saga-1", NonGoalKind::Saga),
        ("class-1", NonGoalKind::Class),
        ("cloak-1", NonGoalKind::FaceDownProducer),
        ("dread-1", NonGoalKind::FaceDownProducer),
    ] {
        assert_eq!(non_goal_kind(&db, oracle_id), kind, "{oracle_id}");
    }
}

#[test]
fn artifact_planeswalker_reports_planeswalker() {
    let db = open_fixture();
    assert_eq!(non_goal_kind(&db, "pw-art-1"), NonGoalKind::Planeswalker);
}

#[test]
fn disguise_card_with_a_producer_keyword_reports_face_down_producer() {
    let db = open_fixture();
    assert_eq!(non_goal_kind(&db, "both-1"), NonGoalKind::FaceDownProducer);
}

#[test]
fn non_goal_error_names_the_card_and_every_lookup_refuses_it() {
    let db = open_fixture();
    let err = db.load_by_name("The Eldest Reborn").unwrap_err();
    assert!(err.to_string().contains("The Eldest Reborn"), "{err}");
    assert!(err.to_string().contains("saga"), "{err}");
    assert!(matches!(
        db.record_by_oracle_id("saga-1").unwrap_err(),
        CardLoadError::NonGoal { .. }
    ));
}

#[test]
fn face_down_cast_and_unflagged_cards_still_load() {
    let db = open_fixture();
    for oracle_id in ["morph-1", "mega-1", "disguise-1", "meld-1", "ixidron-1"] {
        db.load_by_oracle_id(oracle_id)
            .unwrap_or_else(|e| panic!("{oracle_id} should load: {e}"));
    }
    // Morph and Megamorph are not in `KeywordSet`, so only the record keeps them.
    let record = db.record_by_oracle_id("morph-1").unwrap();
    assert_eq!(record.keywords, vec!["Morph".to_string()]);
}
