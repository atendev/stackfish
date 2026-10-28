//! Turns rows of the external card SQLite file into `CardDefinition`s. Only printed
//! characteristics (cost, types, P/T, keywords) come from columns; behavior comes from
//! the card's Lua script. See `docs/DESIGN.md`.

mod keywords;
mod script;
mod types_line;

use rusqlite::Connection;
use stackfish_engine::{CardDefinition, KeywordSet, ManaCost, ModeChoice, PTValue, ScriptReference};
use std::path::Path;

pub use types_line::TypeLineError;

/// Everything that can go wrong turning a DB row into a `CardDefinition`.
/// Unrecognized input is always an error, never a guess.
#[derive(Debug)]
pub enum CardLoadError {
    Sqlite(rusqlite::Error),
    /// No `cards` row matched the key.
    NotFound(String),
    /// `cards.has_multiple_faces = 1`; multi-face cards are a `DESIGN.md` non-goal.
    MultiFace(String),
    /// `cards.mana_cost` failed `ManaCost::parse`.
    ManaCost(stackfish_engine::ManaCostParseError),
    /// `cards.type_line` held a type this engine does not model.
    TypeLine(TypeLineError),
}

impl std::fmt::Display for CardLoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CardLoadError::Sqlite(e) => write!(f, "sqlite error: {e}"),
            CardLoadError::NotFound(key) => write!(f, "no card found for {key:?}"),
            CardLoadError::MultiFace(name) => {
                write!(f, "{name:?} has multiple faces (DFC/split/flip/adventure) - not supported, see DESIGN.md non-goals")
            }
            CardLoadError::ManaCost(e) => write!(f, "mana_cost parse error: {}", e.0),
            CardLoadError::TypeLine(e) => write!(f, "type_line parse error: {e}"),
        }
    }
}

impl std::error::Error for CardLoadError {}

impl From<rusqlite::Error> for CardLoadError {
    fn from(e: rusqlite::Error) -> Self {
        CardLoadError::Sqlite(e)
    }
}

/// Read-only handle onto the card database.
pub struct CardDb {
    conn: Connection,
}

impl CardDb {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, CardLoadError> {
        let conn = Connection::open(path)?;
        Ok(CardDb { conn })
    }

    /// Loads the card whose `cards.oracle_id` (the primary key) is `oracle_id`.
    pub fn load_by_oracle_id(&self, oracle_id: &str) -> Result<CardDefinition, CardLoadError> {
        let row = self.fetch_row("oracle_id", oracle_id)?;
        self.build_definition(row)
    }

    /// Loads the card named `cards.name`. Names are unique per oracle card in the source
    /// data, but the schema does not enforce it.
    pub fn load_by_name(&self, name: &str) -> Result<CardDefinition, CardLoadError> {
        let row = self.fetch_row("name", name)?;
        self.build_definition(row)
    }

    /// Loads the card whose `cards.script_id` slug (e.g. `"lightning_bolt"`) is `script_id`.
    /// Hand-written scenarios use it to avoid typing a name or UUID.
    pub fn load_by_script_id(&self, script_id: &str) -> Result<CardDefinition, CardLoadError> {
        let row = self.fetch_row("script_id", script_id)?;
        self.build_definition(row)
    }

    fn fetch_row(&self, column: &str, key: &str) -> Result<CardRow, CardLoadError> {
        // `column` is always a literal from the loaders above, and SQLite cannot bind identifiers.
        let sql = format!(
            "SELECT oracle_id, name, mana_cost, type_line, oracle_text, power, toughness, \
             loyalty, defense, has_multiple_faces \
             FROM cards WHERE {column} = ?1"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let row = stmt
            .query_row([key], |row| {
                Ok(CardRow {
                    oracle_id: row.get(0)?,
                    name: row.get(1)?,
                    mana_cost: row.get(2)?,
                    type_line: row.get(3)?,
                    oracle_text: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
                    power: row.get(5)?,
                    toughness: row.get(6)?,
                    loyalty: row.get(7)?,
                    defense: row.get(8)?,
                    has_multiple_faces: row.get(9)?,
                })
            })
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => CardLoadError::NotFound(key.to_string()),
                other => CardLoadError::Sqlite(other),
            })?;
        Ok(row)
    }

    fn build_definition(&self, row: CardRow) -> Result<CardDefinition, CardLoadError> {
        if row.has_multiple_faces {
            return Err(CardLoadError::MultiFace(row.name));
        }

        let mana_cost = match row.mana_cost.as_deref() {
            Some(s) if !s.is_empty() => ManaCost::parse(s).map_err(CardLoadError::ManaCost)?,
            _ => ManaCost { symbols: Vec::new() },
        };

        let types_line::ParsedTypeLine { supertypes, card_types, subtypes } =
            types_line::parse(&row.type_line).map_err(CardLoadError::TypeLine)?;

        let keywords = self.load_keywords(&row.oracle_id)?;
        let script_ref = self.load_script_ref(&row.oracle_id)?;

        Ok(CardDefinition {
            definition_id: hash_oracle_id(&row.oracle_id),
            name: row.name,
            mana_cost,
            color_indicator: None,
            supertypes,
            card_types,
            subtypes,
            oracle_text: row.oracle_text,
            power: parse_pt(row.power.as_deref()),
            toughness: parse_pt(row.toughness.as_deref()),
            loyalty: parse_pt(row.loyalty.as_deref()),
            defense: parse_pt(row.defense.as_deref()),
            script_ref,
            keywords,
            // The card's script registers these at runtime.
            target_requirement: None,
            alternative_costs: Vec::new(),
            buyback_cost: None,
            effect: None,
            modes: Vec::new(),
            mode_choice: ModeChoice::Exactly(1),
        })
    }

    fn load_keywords(&self, oracle_id: &str) -> Result<KeywordSet, CardLoadError> {
        let mut stmt = self.conn.prepare(
            "SELECT k.name FROM keywords k \
             JOIN card_keywords ck ON ck.keyword_id = k.id \
             WHERE ck.oracle_id = ?1",
        )?;
        let names = stmt
            .query_map([oracle_id], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<String>, _>>()?;
        Ok(keywords::parse(&names))
    }

    // A card with no active `card_scripts` row gets a stub module, so `run_card_script`
    // does not fail on an empty string.
    fn load_script_ref(&self, oracle_id: &str) -> Result<ScriptReference, CardLoadError> {
        use rusqlite::OptionalExtension;
        let source: Option<String> = self
            .conn
            .query_row(
                "SELECT script_source FROM card_scripts \
                 WHERE oracle_id = ?1 AND is_active = 1 \
                 ORDER BY version DESC LIMIT 1",
                [oracle_id],
                |row| row.get(0),
            )
            .optional()?;
        Ok(ScriptReference::Inline(source.unwrap_or_else(script::vanilla_stub)))
    }

    /// The highest active `version` of each `(oracle_id, test_name)` in `card_tests`,
    /// ordered by card name then test name so batch output is stable.
    pub fn active_tests(&self) -> Result<Vec<CardTestRow>, CardLoadError> {
        let mut stmt = self.conn.prepare(
            "SELECT ct.oracle_id, c.name, ct.test_name, ct.version, ct.test_source \
             FROM card_tests ct \
             JOIN cards c ON c.oracle_id = ct.oracle_id \
             WHERE ct.is_active = 1 \
             AND ct.version = ( \
                 SELECT MAX(ct2.version) FROM card_tests ct2 \
                 WHERE ct2.oracle_id = ct.oracle_id \
                   AND ct2.test_name = ct.test_name \
                   AND ct2.is_active = 1 \
             ) \
             ORDER BY c.name, ct.test_name",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok(CardTestRow {
                    oracle_id: row.get(0)?,
                    card_name: row.get(1)?,
                    test_name: row.get(2)?,
                    version: row.get(3)?,
                    test_source: row.get(4)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }
}

/// One active `card_tests` row joined with its card's name. `test_source` is a scenario
/// TOML (`docs/scenario_format.md`).
pub struct CardTestRow {
    pub oracle_id: String,
    pub card_name: String,
    pub test_name: String,
    pub version: i64,
    pub test_source: String,
}

struct CardRow {
    oracle_id: String,
    name: String,
    mana_cost: Option<String>,
    type_line: String,
    oracle_text: String,
    power: Option<String>,
    toughness: Option<String>,
    loyalty: Option<String>,
    defense: Option<String>,
    has_multiple_faces: bool,
}

/// An integer is `Fixed`; anything else (`"*"`, `"*+1"`) is characteristic-defining.
///
/// TODO: `CharacteristicDefining` gets `script: None` because the DB has no column for a
/// bare CDA function, so a `*` creature contributes no layer 7a value. CR 613.4a
fn parse_pt(raw: Option<&str>) -> Option<PTValue> {
    let raw = raw?;
    if let Ok(n) = raw.parse::<i32>() {
        return Some(PTValue::Fixed(n));
    }
    Some(PTValue::CharacteristicDefining { display: raw.to_string(), script: None })
}

// FNV-1a over the oracle_id UUID, because the schema has no numeric id. `DefaultHasher` is
// not stable across Rust versions, which would break any persisted id.
fn hash_oracle_id(oracle_id: &str) -> u64 {
    const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;
    let mut hash = FNV_OFFSET_BASIS;
    for byte in oracle_id.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_oracle_id_is_deterministic() {
        let id = "56115bd1-1234-4c1e-9a3a-0e1f2a3b4c5d";
        assert_eq!(hash_oracle_id(id), hash_oracle_id(id));
    }

    #[test]
    fn hash_oracle_id_differs_for_different_ids() {
        assert_ne!(hash_oracle_id("a"), hash_oracle_id("b"));
    }

    #[test]
    fn parse_pt_numeric_is_fixed() {
        assert!(matches!(parse_pt(Some("3")), Some(PTValue::Fixed(3))));
        assert!(matches!(parse_pt(Some("-1")), Some(PTValue::Fixed(-1))));
    }

    #[test]
    fn parse_pt_star_is_characteristic_defining_with_no_script() {
        match parse_pt(Some("*")) {
            Some(PTValue::CharacteristicDefining { display, script }) => {
                assert_eq!(display, "*");
                assert!(script.is_none());
            }
            other => panic!("expected CharacteristicDefining, got {other:?}"),
        }
    }

    #[test]
    fn parse_pt_none_is_none() {
        assert!(parse_pt(None).is_none());
    }
}
