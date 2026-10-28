//! A `CardSource` over `stackfish_carddb::CardDb`, so a `setup` request can resolve cards
//! from a database such as `../stackfish-db/stackfish.db`.

use std::path::Path;

use stackfish_carddb::{CardDb, CardLoadError};
use stackfish_engine::CardDefinition;

use crate::card_source::{CardSource, CardSourceError};

/// Loads definitions by `oracle_id`, the key decklists carry. A failure becomes a
/// `CardSourceError` so the Session can name the key in its setup refusal.
pub struct CardDbCardSource {
    db: CardDb,
}

impl CardDbCardSource {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, CardSourceError> {
        let db = CardDb::open(path).map_err(|e| CardSourceError(format!("{e}")))?;
        Ok(CardDbCardSource { db })
    }
}

impl CardSource for CardDbCardSource {
    fn load(&self, key: &str) -> Result<CardDefinition, CardSourceError> {
        self.db
            .load_by_oracle_id(key)
            .map_err(|e| match e {
                CardLoadError::NotFound(_) => CardSourceError(format!("no card found for key {key:?}")),
                other => CardSourceError(format!("card {key:?} failed to load: {other}")),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use std::path::PathBuf;

    fn fixture_db() -> PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir()
            .join(format!("stackfish-carddb-source-test-{}-{n}.sqlite", std::process::id()));
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
            VALUES ('alpha-1', 'alpha', 'Alpha Bear', '{1}{G}', 'Creature — Bear', '', '2', '2', 0);

            INSERT INTO card_scripts (oracle_id, version, script_source, is_active)
            VALUES ('alpha-1', 1, 'return { register_abilities = function(c) end }', 1);

            INSERT INTO cards (oracle_id, script_id, name, mana_cost, type_line, oracle_text, power, toughness, has_multiple_faces)
            VALUES ('dfc-1', 'dfc', 'Double-Faced Card', NULL, 'Creature — Human', '', '1', '1', 1);

            INSERT INTO cards (oracle_id, name, mana_cost, type_line, oracle_text, power, toughness, has_multiple_faces)
            VALUES ('plane-1', 'Plane Card', NULL, 'Plane — Arabian Nights', '', NULL, NULL, 0);

            INSERT INTO cards (oracle_id, script_id, name, mana_cost, type_line, oracle_text, power, toughness, has_multiple_faces)
            VALUES ('beta-1', 'beta', 'Beta Bear', '{2}{G}', 'Creature — Bear', '', '3', '3', 0);

            INSERT INTO card_scripts (oracle_id, version, script_source, is_active)
            VALUES ('beta-1', 1, 'return { register_abilities = function(c) end }', 1);

            INSERT INTO keywords (name) VALUES ('Trample');
            INSERT INTO card_keywords (oracle_id, keyword_id)
            VALUES ('beta-1', (SELECT id FROM keywords WHERE name = 'Trample'));

            INSERT INTO cards (oracle_id, script_id, name, mana_cost, type_line, oracle_text, power, toughness, has_multiple_faces)
            VALUES ('land-1', 'basic_forest', 'Forest', NULL, 'Basic Land — Forest', '{T}: Add {G}.', NULL, NULL, 0);

            INSERT INTO card_scripts (oracle_id, version, script_source, is_active)
            VALUES ('land-1', 1, 'local s = {} function s.register_abilities(c) local e = Game.CreateManaAbility(c) e:AddCost(Game.Costs.TapSelf) e:SetOperation(Game.Ops.AddMana(\"G\")) e:Register() end return s', 1);

            INSERT INTO cards (oracle_id, script_id, name, mana_cost, type_line, oracle_text, power, toughness, has_multiple_faces)
            VALUES ('spell-1', 'giant_growth', 'Giant Growth', '{G}', 'Instant', 'Target creature gets +3/+3 until end of turn.', NULL, NULL, 0);

            INSERT INTO card_scripts (oracle_id, version, script_source, is_active)
            VALUES ('spell-1', 1, 'local s = {} function s.target_requirement() return { type = \"object\", card_types = { \"Creature\" }, count = 1 } end function s.operation() return Game.Ops.Pump(3, 3) end return s', 1);
            "
        )
        .unwrap();
        drop(conn);
        path
    }

    #[test]
    fn loads_a_real_card_by_oracle_id() {
        let source = CardDbCardSource::open(fixture_db()).unwrap();
        let card = source.load("alpha-1").unwrap();
        assert_eq!(card.name, "Alpha Bear");
        assert!(
            matches!(card.script_ref, stackfish_engine::ScriptReference::Inline(_)),
            "script_ref should be inline"
        );
    }

    #[test]
    fn missing_key_returns_card_source_error_with_key() {
        let source = CardDbCardSource::open(fixture_db()).unwrap();
        let err = source.load("no-such-card").unwrap_err();
        assert!(err.0.contains("no-such-card"), "error names the key: {err}");
    }

    #[test]
    fn unsupported_card_returns_card_source_error_with_key() {
        let source = CardDbCardSource::open(fixture_db()).unwrap();
        let err = source.load("plane-1").unwrap_err();
        assert!(err.0.contains("plane-1"), "error names the key: {err}");
    }

    #[test]
    fn multi_face_card_returns_card_source_error_with_key() {
        let source = CardDbCardSource::open(fixture_db()).unwrap();
        let err = source.load("dfc-1").unwrap_err();
        assert!(err.0.contains("dfc-1"), "error names the key: {err}");
    }
}
