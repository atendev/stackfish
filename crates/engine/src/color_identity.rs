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
pub fn illegal_cards_for_color_identity<'a>(
    commander_identity: ColorSet,
    cards: &'a [CardDefinition],
) -> Vec<&'a CardDefinition> {
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
        if closed {
            if let Ok(cost) = ManaCost::parse(&format!("{{{inner}}}")) {
                groups.push(cost.colors());
            }
        }
    }
    groups
}
