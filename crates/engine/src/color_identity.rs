//! CR 903.4/903.5c/d: Commander color identity and deck-legality queries.
//! Pure functions only - nothing here is wired into `Engine::new_game` or
//! any command. Matches the "engine trusts the setup" stance
//! `docs/implementation.md`'s commander-marking slice already took for CR
//! 903.3a ("can be your commander" card text isn't engine-checked either):
//! a deck-builder or test harness calls these itself if it wants CR 903.5c/d
//! enforced.

use crate::types::card::CardDefinition;
use crate::types::mana::{ColorSet, ManaCost};

/// CR 903.4: the color or colors of any mana symbols in a card's mana cost
/// or rules text, plus its color indicator (CR 204). `CardDefinition::colors`
/// already covers the mana-cost/indicator half (CR 105.2a) - this adds the
/// rules-text half, with CR 903.4c's reminder-text exclusion applied first.
///
/// TODO: colors defined by characteristic-defining abilities (903.4's own
/// clause), a double-faced card's back face (903.4d), and Adventure
/// alternative characteristics (903.4e) aren't included - no color-CDA,
/// DFC, or Adventure mechanic exists in this engine yet (DFCs/Adventure are
/// explicit non-goals elsewhere in docs/implementation.md's backlog).
pub fn color_identity(card: &CardDefinition) -> ColorSet {
    let mut identity = card.colors();
    for colors in mana_symbol_groups(&strip_reminder_text(&card.oracle_text)) {
        identity |= colors;
    }
    identity
}

/// CR 702.124c: a player's commander color identity is the union of all
/// their commanders' individual color identities (Partner: two, tracked
/// independently everywhere else - see `PlayerState::commander_casts`/
/// `commander_damage_taken` - but unioned here specifically, per the rule).
pub fn commander_color_identity(commanders: &[CardDefinition]) -> ColorSet {
    commanders.iter().fold(ColorSet::empty(), |acc, c| acc | color_identity(c))
}

/// CR 903.5c/d: every card in `cards` whose own color identity isn't a
/// subset of `commander_identity`. CR 903.5d (basic lands) needs no
/// separate check - a basic land's own `{T}: Add {G}`-shaped oracle text
/// already carries the `{G}` symbol through `color_identity` above, so an
/// off-color basic land is caught by the same general rule.
pub fn illegal_cards_for_color_identity<'a>(
    commander_identity: ColorSet,
    cards: &'a [CardDefinition],
) -> Vec<&'a CardDefinition> {
    cards.iter().filter(|c| !commander_identity.contains(color_identity(c))).collect()
}

/// CR 903.4c: reminder text (parenthetical) is ignored when determining
/// color identity - strips every top-level `(...)` span. MTG reminder text
/// never nests parentheses, so a flat depth counter is enough.
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

/// Every `{...}` group's colors, found by scanning free-form prose - each
/// group is parsed independently via `ManaCost::parse` (rewrapped in its
/// own braces) rather than duplicating `mana.rs`'s own symbol-group
/// grammar. A malformed/unrecognized group (rare in prose - e.g. a stray
/// unmatched brace) is skipped rather than erroring, since oracle text
/// isn't a cost being paid, just scanned for color.
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
