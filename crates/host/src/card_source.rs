//! How a Session turns a card key (an `oracle_id` UUID) into a `CardDefinition`. The host
//! passes the source in at creation. The engine never sees keys or a database.

use std::collections::HashMap;

use stackfish_engine::CardDefinition;

use crate::fingerprint;

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

    /// A short text that changes whenever the card `key` resolves to changes, so a replay
    /// can refuse card data that differs from the game it recorded.
    fn fingerprint(&self, key: &str) -> Result<String, CardSourceError>;
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

impl FixtureCardSource {
    fn get(&self, key: &str) -> Result<&CardDefinition, CardSourceError> {
        self.cards
            .get(key)
            .ok_or_else(|| CardSourceError("no such card".to_string()))
    }
}

impl CardSource for FixtureCardSource {
    fn load(&self, key: &str) -> Result<CardDefinition, CardSourceError> {
        self.get(key).cloned()
    }

    fn fingerprint(&self, key: &str) -> Result<String, CardSourceError> {
        self.get(key).map(fingerprint::of_definition)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stackfish_engine::{CardTypeSet, KeywordSet, ManaCost, ScriptReference};
    use stackfish_testutil::minimal_card_definition;

    fn bear() -> CardDefinition {
        minimal_card_definition(
            7,
            "Bear",
            "return {}",
            ManaCost::parse("{1}{G}").unwrap(),
            CardTypeSet::CREATURE,
        )
    }

    fn print(card: CardDefinition) -> String {
        FixtureCardSource::new()
            .with("k", card)
            .fingerprint("k")
            .unwrap()
    }

    #[test]
    fn a_fixture_fingerprint_is_pinned_so_a_change_to_the_encoding_is_noticed() {
        assert_eq!(print(bear()), "3601aaa9d58f8624");
    }

    #[test]
    fn a_fixture_fingerprint_changes_with_the_name_text_keywords_script_and_mode_choice() {
        let base = print(bear());
        let changes: [fn(&mut CardDefinition); 6] = [
            |c| c.name = "Bare".to_string(),
            |c| c.oracle_text = "Trample".to_string(),
            |c| c.keywords = KeywordSet::TRAMPLE,
            |c| c.script_ref = ScriptReference::Inline("return { }".to_string()),
            |c| c.mode_choice = stackfish_engine::ModeChoice::UpTo(1),
            |c| c.color_indicator = Some(stackfish_engine::ColorSet::GREEN),
        ];
        for change in changes {
            let mut card = bear();
            change(&mut card);
            assert_ne!(print(card), base);
        }
    }

    #[test]
    fn a_missing_key_has_no_fingerprint() {
        assert!(FixtureCardSource::new().fingerprint("nope").is_err());
    }
}
