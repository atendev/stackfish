//! The interface a Session uses to turn a card key into a `CardDefinition`.
//! Whoever creates the Session passes the source in, and it never travels in a
//! message. A key is the card's `oracle_id` UUID. The engine never sees keys,
//! decklists or a database.

use std::collections::HashMap;

use stackfish_engine::CardDefinition;

/// Why a source could not give a card. The Session adds the key to the message.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CardSourceError(pub String);

impl std::fmt::Display for CardSourceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for CardSourceError {}

pub trait CardSource {
    /// The definition for `key`. The Session calls this once per distinct key
    /// and clones the result for each copy in a deck.
    fn load(&self, key: &str) -> Result<CardDefinition, CardSourceError>;
}

/// Serves cards from a map built in code. Tests and the proof game use it, so
/// CI never reads `../stackfish-db`.
///
/// TODO(carddb source): a source backed by `stackfish_carddb::CardDb::load_by_oracle_id`
/// comes after B3. It wraps one call, so it needs no new decision.
#[derive(Default)]
pub struct FixtureCardSource {
    cards: HashMap<String, CardDefinition>,
}

impl FixtureCardSource {
    pub fn new() -> Self {
        FixtureCardSource::default()
    }

    /// Adds a card under `key`, replacing any card already there.
    pub fn with(mut self, key: impl Into<String>, card: CardDefinition) -> Self {
        self.cards.insert(key.into(), card);
        self
    }

    pub fn insert(&mut self, key: impl Into<String>, card: CardDefinition) {
        self.cards.insert(key.into(), card);
    }
}

impl CardSource for FixtureCardSource {
    fn load(&self, key: &str) -> Result<CardDefinition, CardSourceError> {
        self.cards.get(key).cloned().ok_or_else(|| CardSourceError("no such card".to_string()))
    }
}
