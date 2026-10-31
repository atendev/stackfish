//! Commander color identity and deck-legality queries, as pure functions.
//! Nothing here runs at setup; a deck builder or harness calls it. See `DESIGN.md`.

use crate::types::card::CardDefinition;
use crate::types::mana::{ColorSet, ManaCost};

/// CR 903.4: colors from the mana cost, color indicator and rules text, with
/// reminder text excluded first.
///
// TODO: colors from characteristic-defining abilities, a double-faced back face and Adventure characteristics (CR 903.4d, 903.4e)
pub fn color_identity(card: &CardDefinition) -> ColorSet {
    let mut identity = card.colors();
    for colors in mana_symbol_groups(&strip_reminder_text(&card.oracle_text)) {
        identity |= colors;
    }
    identity
}

/// CR 702.124c: the union of every commander's color identity.
pub fn commander_color_identity(commanders: &[CardDefinition]) -> ColorSet {
    commanders.iter().fold(ColorSet::empty(), |acc, c| acc | color_identity(c))
}

/// CR 903.5c: every card whose color identity is not a subset of `commander_identity`.
/// Basic lands need no separate check (CR 903.5d): their `{T}: Add` text carries the symbol.
pub fn illegal_cards_for_color_identity(
    commander_identity: ColorSet,
    cards: &[CardDefinition],
) -> Vec<&CardDefinition> {
    cards.iter().filter(|c| !commander_identity.contains(color_identity(c))).collect()
}

/// CR 903.4c: strips every `(...)` span. Reminder text never nests, so a flat depth counter works.
fn strip_reminder_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut depth = 0u32;
    for c in text.chars() {
        match c {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out
}

/// Colors of every `{...}` group in prose, parsed through `ManaCost` to share its symbol grammar.
/// A malformed group is skipped, since oracle text is scanned rather than paid.
fn mana_symbol_groups(text: &str) -> Vec<ColorSet> {
    let mut groups = Vec::new();
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '{' {
            continue;
        }
        let mut inner = String::new();
        let mut closed = false;
        for c in chars.by_ref() {
            if c == '}' {
                closed = true;
                break;
            }
            inner.push(c);
        }
        if closed
            && let Ok(cost) = ManaCost::parse(&format!("{{{inner}}}")) {
                groups.push(cost.colors());
            }
    }
    groups
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::card::{CardDefinition, ModeChoice, PTValue, ScriptReference, SupertypeSet};
    use crate::types::mana::ManaCost;

    fn card_named(name: &str) -> CardDefinition {
        CardDefinition {
            definition_id: 1,
            name: name.to_string(),
            mana_cost: ManaCost { symbols: Vec::new() },
            color_indicator: None,
            supertypes: SupertypeSet::empty(),
            card_types: crate::types::card::CardTypeSet::CREATURE,
            subtypes: Vec::new(),
            oracle_text: String::new(),
            power: Some(PTValue::Fixed(1)),
            toughness: Some(PTValue::Fixed(1)),
            loyalty: None,
            defense: None,
            script_ref: ScriptReference::ByKey("test".to_string()),
            keywords: crate::types::card::KeywordSet::empty(),
            target_requirement: None,
            alternative_costs: Vec::new(),
            buyback_cost: None,
            effect: None,
            modes: Vec::new(),
            mode_choice: ModeChoice::Exactly(1),
        }
    }

    fn with_mana_cost(mut card: CardDefinition, cost: &str) -> CardDefinition {
        card.mana_cost = ManaCost::parse(cost).unwrap();
        card
    }

    fn with_indicator(mut card: CardDefinition, indicator: ColorSet) -> CardDefinition {
        card.color_indicator = Some(indicator);
        card
    }

    fn with_text(mut card: CardDefinition, text: &str) -> CardDefinition {
        card.oracle_text = text.to_string();
        card
    }

    #[test]
    fn color_identity_from_mana_cost_alone() {
        let card = with_mana_cost(card_named("Red Creature"), "{1}{R}");
        assert_eq!(color_identity(&card), ColorSet::RED);
    }

    #[test]
    fn color_identity_uses_color_indicator_when_present() {
        // CR 105.2a: a color indicator overrides the mana-cost color.
        let card = with_indicator(with_mana_cost(card_named("Blue-ish"), "{1}{R}"), ColorSet::BLUE);
        assert_eq!(color_identity(&card), ColorSet::BLUE);
    }

    #[test]
    fn color_identity_includes_rules_text_symbols() {
        // "Flying" carries no mana symbols; "{T}: Add {G}" does.
        let card = with_text(with_mana_cost(card_named("Birds of Paradise-ish"), "{G}"), "{T}: Add {W}.");
        assert_eq!(color_identity(&card), ColorSet::GREEN | ColorSet::WHITE);
    }

    #[test]
    fn color_identity_skips_reminder_text_before_scanning() {
        // CR 903.4c: reminder text in parentheses is ignored.
        let card = with_text(
            with_mana_cost(card_named("Hybrid Reminder"), "{W/B}"),
            "(This spell costs {U} more to cast.) {T}: Add {R}.",
        );
        // Mana cost gives W|B; rules text gives only {R} after stripping parentheses.
        assert_eq!(color_identity(&card), ColorSet::WHITE | ColorSet::BLACK | ColorSet::RED);
    }

    #[test]
    fn color_identity_unions_multiple_text_symbols() {
        let card = with_text(
            with_mana_cost(card_named("Five-Color Text"), "{1}"),
            "{T}: Add {W}. {T}: Add {U}. {T}: Add {B}. {T}: Add {R}. {T}: Add {G}.",
        );
        assert_eq!(color_identity(&card), ColorSet::all());
    }

    #[test]
    fn color_identity_ignores_malformed_groups() {
        let card = with_text(with_mana_cost(card_named("Malformed"), "{1}"), "{T}: Add {Q}.");
        // {Q} is not a known symbol, so it is skipped.
        assert_eq!(color_identity(&card), ColorSet::empty());
    }

    #[test]
    fn commander_color_identity_unions_commanders() {
        let red = with_mana_cost(card_named("Red"), "{R}");
        let blue = with_mana_cost(card_named("Blue"), "{U}");
        assert_eq!(commander_color_identity(&[red, blue]), ColorSet::RED | ColorSet::BLUE);
    }

    #[test]
    fn illegal_cards_for_color_identity_filters_by_subset() {
        let commander = with_mana_cost(card_named("Commander"), "{R}{G}");
        let identity = color_identity(&commander);

        let legal = with_mana_cost(card_named("Legal"), "{R}");
        let legal_hybrid = with_mana_cost(card_named("Legal Hybrid"), "{R/G}");
        let illegal = with_mana_cost(card_named("Illegal"), "{U}");

        let cards = [legal, legal_hybrid, illegal];
        let illegal_cards = illegal_cards_for_color_identity(identity, &cards);
        assert_eq!(illegal_cards.len(), 1);
        assert_eq!(illegal_cards[0].name, "Illegal");
    }

    #[test]
    fn strip_reminder_text_removes_parenthesized_spans() {
        assert_eq!(strip_reminder_text("(foo) bar"), " bar");
        assert_eq!(strip_reminder_text("a (b (c)) d"), "a  d");
    }

    #[test]
    fn mana_symbol_groups_collects_colors() {
        // {T} has no color; {G} is green; {W/U} is white/blue.
        let groups = mana_symbol_groups("{T}: Add {G} or {W/U}.");
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0], ColorSet::GREEN);
        assert_eq!(groups[1], ColorSet::WHITE | ColorSet::BLUE);
    }

    #[test]
    fn mana_symbol_groups_skips_malformed_and_plain_text() {
        let groups = mana_symbol_groups("{T}: Add {Q} and 1 red mana.");
        // {T}, {Q} both have no colors.
        assert!(groups.iter().all(|g| g.is_empty()));
    }

    #[test]
    fn mana_symbol_groups_hybrid_color_color_yields_union() {
        let groups = mana_symbol_groups("{W/B}");
        assert_eq!(groups, vec![ColorSet::WHITE | ColorSet::BLACK]);
    }
}
