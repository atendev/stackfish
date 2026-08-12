//! Reads the external card-database SQLite file (schema owned by that
//! separate project, not this repo — see `DESIGN.md`'s "no card database
//! tooling" non-goal) and produces real `stackfish_engine::CardDefinition`s
//! from it. This is the connecting point `DESIGN.md` describes: the engine
//! consumes `CardDefinition`, it doesn't author one — this crate is what
//! turns a DB row into one.
//!
//! Scope: only the *structural/printed* characteristics a card carries (CR
//! 613 layer 7's starting values — cost, types, P/T, keywords) come from SQL
//! columns. Everything behavioral (target requirements, spell effects,
//! alternative costs, modes) is left at `CardDefinition`'s "unset" default
//! and is expected to come from the card's own Lua script via the
//! self-registering `register_abilities(c)` module convention every other
//! card in this engine already uses (see `docs/lua/card-script-basics.md`) —
//! this loader never attempts to derive game behavior from oracle text.

mod keywords;
mod script;
mod types_line;

use rusqlite::Connection;
use stackfish_engine::{CardDefinition, KeywordSet, ManaCost, ModeChoice, PTValue, ScriptReference};
use std::path::Path;

pub use types_line::TypeLineError;

/// Everything that can go wrong turning a DB row into a `CardDefinition`.
/// Deliberately a hard error, never a best-effort guess — same "unrecognized
/// input is a bug to surface, not paper over" posture `ManaCost::parse`
/// already takes in the engine crate.
#[derive(Debug)]
pub enum CardLoadError {
    Sqlite(rusqlite::Error),
    /// No `cards` row matched the given oracle_id/name.
    NotFound(String),
    /// `cards.has_multiple_faces = 1` — DFC/split/flip/adventure cards are
    /// an explicit `DESIGN.md` non-goal; `card_faces` isn't consumed at all
    /// by this slice. Revisit if that non-goal is ever lifted.
    MultiFace(String),
    /// `cards.mana_cost` didn't parse via `ManaCost::parse` — a malformed or
    /// unexpected string in the DB, not something to silently zero out.
    ManaCost(stackfish_engine::ManaCostParseError),
    /// `cards.type_line` couldn't be decoded into this engine's
    /// `CardTypeSet`/`SupertypeSet` — see `TypeLineError`.
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

/// Read-only handle onto the card database. Cheap to construct; holds one
/// `rusqlite::Connection`.
pub struct CardDb {
    conn: Connection,
}

impl CardDb {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, CardLoadError> {
        let conn = Connection::open(path)?;
        Ok(CardDb { conn })
    }

    /// Scryfall oracle_id (the `cards.oracle_id` primary key) -> a real
    /// `CardDefinition`.
    pub fn load_by_oracle_id(&self, oracle_id: &str) -> Result<CardDefinition, CardLoadError> {
        let row = self.fetch_row("oracle_id", oracle_id)?;
        self.build_definition(row)
    }

    /// Card name (`cards.name`) -> a real `CardDefinition`. Scryfall names
    /// are unique per oracle card in the bulk data this DB is built from, so
    /// this is unambiguous the same way `load_by_oracle_id` is - not true in
    /// general for arbitrary SQL data, but true of this specific schema's
    /// source.
    pub fn load_by_name(&self, name: &str) -> Result<CardDefinition, CardLoadError> {
        let row = self.fetch_row("name", name)?;
        self.build_definition(row)
    }

    /// `cards.script_id` -> a real `CardDefinition`. The stable, human-typed
    /// slug tied to a card's script (e.g. `"lightning_bolt"`) - meant as the
    /// ergonomic lookup key for hand-authored callers (scenario TOML
    /// fixtures) that want a real card without retyping its full printed
    /// name or a UUID `oracle_id`.
    pub fn load_by_script_id(&self, script_id: &str) -> Result<CardDefinition, CardLoadError> {
        let row = self.fetch_row("script_id", script_id)?;
        self.build_definition(row)
    }

    fn fetch_row(&self, column: &str, key: &str) -> Result<CardRow, CardLoadError> {
        // `column` is always one of the three hardcoded literals above,
        // never caller-supplied - safe to interpolate into the query string
        // (rusqlite doesn't support parameterizing identifiers, only values).
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
            // Behavioral fields deliberately left at their "unset" default -
            // see this module's own doc comment for why. A card's own
            // script fills these in at runtime (`Engine::
            // register_script_target_requirement`/`register_script_operation`,
            // same convention every hand-authored Lua card already uses),
            // not this loader.
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

    /// `card_scripts` has no active row for a vanilla card with no ability
    /// text - returns a minimal valid stub module rather than an empty
    /// string, so `ScriptRuntime::run_card_script` doesn't hit its
    /// `LoadError` path for the overwhelming majority of real cards (every
    /// call site today discards that `Result`, so an empty string would
    /// silently do the same thing in practice - this is just not relying on
    /// that).
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

    /// Every active `card_tests` row, one per `(oracle_id, test_name)` -
    /// same "`is_active = 1`, highest `version` wins" convention
    /// `load_script_ref` already uses for `card_scripts`, just per test name
    /// instead of a single row per card. Ordered by card name then test name
    /// so a batch run's output is stable/readable run to run.
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

/// One active row from `card_tests`, joined with the card's own `name` for
/// reporting - a hand-authored scenario TOML (see `docs/scenario_format.md`)
/// stored in the DB and tied to the card it exercises, rather than living as
/// a file under `tests/scenarios/`.
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

/// `cards.power`/`toughness`/`loyalty`/`defense` -> `PTValue`. A plain
/// integer (optionally signed, e.g. `-1` for some vehicles' printed values
/// though not toughness in practice) is `Fixed`; anything else (`"*"`,
/// `"*+1"`, `"1+*"`, null) is a characteristic-defining value.
///
/// TODO: every `CharacteristicDefining` produced here gets `script: None` -
/// the DB schema has no column holding the bare `function(source,
/// controller) -> integer` `ScriptRuntime::invoke_cda` expects, only the
/// full self-registering module in `card_scripts`. `layers::
/// effective_characteristics` already treats `script: None` as "skip rather
/// than guess" (not a panic), so a `*`-power creature loads today with
/// correct display text but contributes no real layer-7a number until the
/// DB schema grows a dedicated CDA-function column. Revisit once
/// `card_scripts` (or a new table) can hand back that shape.
fn parse_pt(raw: Option<&str>) -> Option<PTValue> {
    let raw = raw?;
    if let Ok(n) = raw.parse::<i32>() {
        return Some(PTValue::Fixed(n));
    }
    Some(PTValue::CharacteristicDefining { display: raw.to_string(), script: None })
}

/// `CardDefinition.definition_id` is a `u64`, but this DB's stable card
/// identifier (`cards.oracle_id`) is a UUID string with no numeric id
/// anywhere in the schema. FNV-1a 64-bit over the UTF-8 bytes gives a
/// deterministic id that's stable across runs/machines - deliberately not
/// `std::collections::hash_map::DefaultHasher`, which is explicitly
/// documented as *not* build-stable (its output can change between Rust
/// versions/compilations), which would silently break anything persisting
/// this id across a rebuild.
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
