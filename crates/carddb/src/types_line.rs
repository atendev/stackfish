//! Parses `cards.type_line` (`"Legendary Creature — Human Wizard"`) into supertypes, card
//! types and subtypes. It duplicates the scenario crate's parsers because those panic on an
//! unknown name and this one must return an error.

use stackfish_engine::{CardTypeSet, Subtype, SupertypeSet};

#[derive(Debug)]
pub struct TypeLineError(pub String);

impl std::fmt::Display for TypeLineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

pub struct ParsedTypeLine {
    pub supertypes: SupertypeSet,
    pub card_types: CardTypeSet,
    pub subtypes: Vec<Subtype>,
}

/// Splits on the em dash (U+2014) between the type portion and the subtypes. A line with
/// no dash has no subtypes.
pub fn parse(type_line: &str) -> Result<ParsedTypeLine, TypeLineError> {
    let (left, right) = match type_line.split_once('—') {
        Some((l, r)) => (l, Some(r)),
        None => (type_line, None),
    };

    let mut supertypes = SupertypeSet::empty();
    let mut card_types = CardTypeSet::empty();
    for word in left.split_whitespace() {
        if let Some(supertype) = parse_supertype(word) {
            supertypes |= supertype;
        } else if let Some(card_type) = parse_card_type(word) {
            card_types |= card_type;
        } else {
            return Err(TypeLineError(format!(
                "unrecognized/unsupported type {word:?} in type_line {type_line:?} - not in this \
                 engine's CardTypeSet/SupertypeSet (casual-format types like Plane/Phenomenon/\
                 Scheme/Vanguard/Conspiracy/Dungeon/Class are an explicit DESIGN.md non-goal)"
            )));
        }
    }
    if card_types.is_empty() {
        return Err(TypeLineError(format!(
            "type_line {type_line:?} has no recognized card type"
        )));
    }

    let subtypes = right
        .unwrap_or("")
        .split_whitespace()
        .map(|s| Subtype(s.to_string()))
        .collect();

    Ok(ParsedTypeLine {
        supertypes,
        card_types,
        subtypes,
    })
}

fn parse_supertype(word: &str) -> Option<SupertypeSet> {
    Some(match word {
        "Basic" => SupertypeSet::BASIC,
        "Legendary" => SupertypeSet::LEGENDARY,
        "Snow" => SupertypeSet::SNOW,
        "World" => SupertypeSet::WORLD,
        _ => return None,
    })
}

fn parse_card_type(word: &str) -> Option<CardTypeSet> {
    Some(match word {
        "Artifact" => CardTypeSet::ARTIFACT,
        "Creature" => CardTypeSet::CREATURE,
        "Enchantment" => CardTypeSet::ENCHANTMENT,
        "Instant" => CardTypeSet::INSTANT,
        "Land" => CardTypeSet::LAND,
        "Sorcery" => CardTypeSet::SORCERY,
        // CR 205.3m
        // Scryfall data can still print the old name.
        "Tribal" | "Kindred" => CardTypeSet::KINDRED,
        "Planeswalker" => CardTypeSet::PLANESWALKER,
        "Battle" => CardTypeSet::BATTLE,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_creature_with_subtypes() {
        let parsed = parse("Legendary Creature — Human Wizard").unwrap();
        assert_eq!(parsed.supertypes, SupertypeSet::LEGENDARY);
        assert_eq!(parsed.card_types, CardTypeSet::CREATURE);
        assert_eq!(
            parsed.subtypes,
            vec![Subtype("Human".into()), Subtype("Wizard".into())]
        );
    }

    #[test]
    fn parses_type_with_no_dash_or_subtypes() {
        let parsed = parse("Land").unwrap();
        assert_eq!(parsed.card_types, CardTypeSet::LAND);
        assert!(parsed.subtypes.is_empty());
    }

    #[test]
    fn parses_multi_type_permanent() {
        let parsed = parse("Artifact Creature — Construct").unwrap();
        assert_eq!(
            parsed.card_types,
            CardTypeSet::ARTIFACT | CardTypeSet::CREATURE
        );
        assert_eq!(parsed.subtypes, vec![Subtype("Construct".into())]);
    }

    #[test]
    fn tribal_and_kindred_both_map_to_kindred_bit() {
        assert_eq!(
            parse("Tribal Sorcery — Goblin").unwrap().card_types,
            CardTypeSet::SORCERY | CardTypeSet::KINDRED
        );
        assert_eq!(
            parse("Kindred Sorcery — Goblin").unwrap().card_types,
            CardTypeSet::SORCERY | CardTypeSet::KINDRED
        );
    }

    #[test]
    fn unsupported_casual_format_type_is_an_error() {
        assert!(parse("Plane — Rabiah").is_err());
        assert!(parse("Scheme").is_err());
    }
}
