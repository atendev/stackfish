//! Expands `template = "name"` on a `[[cards]]` entry from `card_templates.toml` before
//! deserialization. The entry's own fields win over the template's. See `docs/scenario_format.md`.

use std::sync::OnceLock;
use toml::{Table, Value};

const TEMPLATES_TOML: &str = include_str!("../card_templates.toml");

fn templates() -> &'static Table {
    static TEMPLATES: OnceLock<Table> = OnceLock::new();
    TEMPLATES.get_or_init(|| {
        toml::from_str(TEMPLATES_TOML)
            .unwrap_or_else(|e| panic!("failed to parse crates/scenario/card_templates.toml: {e}"))
    })
}

pub(crate) fn expand(root: &mut Table) {
    let Some(Value::Array(cards)) = root.remove("cards") else {
        return;
    };
    let expanded = cards.into_iter().map(expand_one).collect();
    root.insert("cards".to_string(), Value::Array(expanded));
}

fn expand_one(card: Value) -> Value {
    let Value::Table(mut entry) = card else {
        return card;
    };
    let Some(Value::String(name)) = entry.remove("template") else {
        return Value::Table(entry);
    };
    let template = templates().get(&name).unwrap_or_else(|| {
        panic!("unknown card template {name:?} - see crates/scenario/card_templates.toml for the defined names")
    });
    let Value::Table(template) = template else {
        panic!("card template {name:?} in card_templates.toml must be a table");
    };
    let mut merged = template.clone();
    for (key, value) in entry {
        merged.insert(key, value);
    }
    Value::Table(merged)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_templates_parse_and_are_tables() {
        for name in [
            "test_land_white",
            "test_land_blue",
            "test_land_black",
            "test_land_red",
            "test_land_green",
            "test_land_colorless",
            "test_creature_vanilla",
        ] {
            assert!(templates().get(name).is_some(), "missing template {name:?}");
        }
    }

    #[test]
    fn expand_one_merges_template_under_entry_fields() {
        let mut entry = Table::new();
        entry.insert("key".to_string(), Value::String("forest".to_string()));
        entry.insert(
            "template".to_string(),
            Value::String("test_land_green".to_string()),
        );
        entry.insert(
            "name".to_string(),
            Value::String("Overridden Name".to_string()),
        );

        let Value::Table(result) = expand_one(Value::Table(entry)) else {
            panic!("expected a table back");
        };

        assert_eq!(result.get("key").unwrap().as_str(), Some("forest"));
        assert_eq!(
            result.get("name").unwrap().as_str(),
            Some("Overridden Name")
        );
        assert_eq!(
            result.get("types").unwrap().as_array().unwrap()[0].as_str(),
            Some("Land")
        );
        assert_eq!(
            result.get("mana_ability").unwrap().as_array().unwrap()[0].as_str(),
            Some("G")
        );
        assert!(!result.contains_key("template"));
    }

    #[test]
    fn expand_one_leaves_a_template_free_entry_untouched() {
        let mut entry = Table::new();
        entry.insert("key".to_string(), Value::String("bear".to_string()));
        entry.insert("name".to_string(), Value::String("Bear".to_string()));
        let original = Value::Table(entry.clone());

        assert_eq!(expand_one(Value::Table(entry)), original);
    }

    #[test]
    #[should_panic(expected = "unknown card template")]
    fn expand_one_panics_on_an_unknown_template_name() {
        let mut entry = Table::new();
        entry.insert("key".to_string(), Value::String("x".to_string()));
        entry.insert(
            "template".to_string(),
            Value::String("does_not_exist".to_string()),
        );
        expand_one(Value::Table(entry));
    }
}
