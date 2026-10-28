//! How a Session turns a card key (an `oracle_id` UUID) into a `CardDefinition`. The host
//! passes the source in at creation. The engine never sees keys or a database.

use std::collections::HashMap;

use stackfish_engine::CardDefinition;

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CardSourceError(pub String);

impl std::fmt::Display for CardSourceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for CardSourceError {}

pub trait CardSource {
    /// The definition for `key`. The Session calls it once per distinct key.
    fn load(&self, key: &str) -> Result<CardDefinition, CardSourceError>;
}

/// Serves cards from a map built in code, so CI never reads `../stackfish-db`.
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
