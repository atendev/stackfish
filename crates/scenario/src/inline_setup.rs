//! Moves inline `CardSpec` fields on a `[[setup]]` entry into a synthesized `[[cards]]` entry
//! before deserialization. It runs before `card_templates::expand`, so a moved `template`
//! still expands. Format in `docs/scenario_format.md`.

use toml::{Table, Value};

// Anything else on a `[[setup]]` table is an inline `CardSpec` field and gets moved out.
const SETUP_FIELDS: &[&str] =
    &["player", "zone", "card", "controlled_since_turn", "damage_marked", "is_commander"];

/// Expands inline card fields into synthesized `[[cards]]` entries in place. An entry with
/// only `SETUP_FIELDS` is untouched.
pub(crate) fn expand(root: &mut Table) {
    let Some(Value::Array(setup_entries)) = root.get("setup") else { return };

    let mut synthesized_cards: Vec<Value> = Vec::new();
    let mut existing_keys: std::collections::HashSet<String> = match root.get("cards") {
        Some(Value::Array(cards)) => cards
            .iter()
            .filter_map(|c| c.as_table()?.get("key")?.as_str().map(str::to_string))
            .collect(),
        _ => std::collections::HashSet::new(),
    };

    let expanded_setup: Vec<Value> = setup_entries
        .iter()
        .cloned()
        .map(|entry| expand_one(entry, &mut synthesized_cards, &mut existing_keys))
        .collect();

    root.insert("setup".to_string(), Value::Array(expanded_setup));

    if !synthesized_cards.is_empty() {
        let cards = match root.entry("cards".to_string()).or_insert_with(|| Value::Array(Vec::new())) {
            Value::Array(cards) => cards,
            other => panic!("scenario file's top-level `cards` must be an array, found {other:?}"),
        };
        cards.extend(synthesized_cards);
    }
}

fn expand_one(
    entry: Value,
    synthesized_cards: &mut Vec<Value>,
    existing_keys: &mut std::collections::HashSet<String>,
) -> Value {
    let Value::Table(mut entry) = entry else { return entry };

    let inline_keys: Vec<String> =
        entry.keys().filter(|k| !SETUP_FIELDS.contains(&k.as_str())).cloned().collect();
    if inline_keys.is_empty() {
        return Value::Table(entry);
    }

    let card_key = entry
        .get("card")
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("a [[setup]] entry with inline card fields ({inline_keys:?}) must set `card` as that card's own key"))
        .to_string();

    if !existing_keys.insert(card_key.clone()) {
        panic!(
            "[[setup]] entry for card key {card_key:?} sets inline card fields, but a \
             [[cards]]/[[setup]] entry with that key already exists - use a different key, or \
             remove the inline fields and reference the existing [[cards]] entry instead"
        );
    }

    let mut card_table = Table::new();
    card_table.insert("key".to_string(), Value::String(card_key));
    for key in inline_keys {
        let value = entry.remove(&key).expect("key came from entry.keys() above");
        card_table.insert(key, value);
    }
    synthesized_cards.push(Value::Table(card_table));

    Value::Table(entry)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Table {
        toml::from_str(text).unwrap()
    }

    #[test]
    fn leaves_a_plain_setup_entry_untouched() {
        let mut root = parse(
            r#"
            [[setup]]
            player = 0
            zone = "battlefield"
            card = "bear"
            "#,
        );
        let original = root.clone();
        expand(&mut root);
        assert_eq!(root, original);
    }

    #[test]
    fn synthesizes_a_cards_entry_from_inline_fields() {
        let mut root = parse(
            r#"
            [[setup]]
            player = 0
            zone = "battlefield"
            card = "forest"
            template = "test_land_green"
            "#,
        );
        expand(&mut root);

        let cards = root["cards"].as_array().unwrap();
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0]["key"].as_str(), Some("forest"));
        assert_eq!(cards[0]["template"].as_str(), Some("test_land_green"));

        let setup = root["setup"].as_array().unwrap();
        assert_eq!(setup.len(), 1);
        assert!(setup[0].as_table().unwrap().get("template").is_none());
        assert_eq!(setup[0]["card"].as_str(), Some("forest"));
    }

    #[test]
    #[should_panic(expected = "already exists")]
    fn panics_on_a_key_collision_with_an_existing_cards_entry() {
        let mut root = parse(
            r#"
            [[cards]]
            key = "bear"
            name = "Test Bear"
            types = ["Creature"]

            [[setup]]
            player = 0
            zone = "battlefield"
            card = "bear"
            power = 9
            "#,
        );
        expand(&mut root);
    }
}
