use bitflags::bitflags;

use crate::types::counter::{CounterKind, CounterSet};
use crate::types::mana::{ColorSet, ManaCost};

/// Printed/base value for power, toughness, loyalty, or defense. `*`-style
/// values (and `* + 1` etc — the modifier is folded into whatever number the
/// characteristic-defining ability script returns, there's no separate
/// "base star plus offset" case to represent) are characteristic-defining
/// abilities: the number isn't known until the script callback runs against
/// current game state, so it can't be stored as a plain `i32`.
#[derive(Clone, Debug)]
pub enum PTValue {
    Fixed(i32),
    CharacteristicDefining(String),
}

impl PTValue {
    /// `Some` only for `Fixed` — a CDA's number isn't known without running
    /// its script callback against current game state, which this can't do.
    pub fn as_fixed(&self) -> Option<i32> {
        match self {
            PTValue::Fixed(n) => Some(*n),
            PTValue::CharacteristicDefining(_) => None,
        }
    }

    /// `as_fixed`, netted against `+1/+1`/`-1/-1` counters (CR 121.3's
    /// counters-affect-characteristics rule) — `None` for the same reason
    /// `as_fixed` is `None` (a CDA's base number isn't known without the
    /// script bridge). Doesn't merge/annihilate opposing counters out of
    /// `counters` itself (CR 704.5q's state-based action does that as a
    /// real mutation) — this only nets them arithmetically for a read,
    /// which gives the same effective number without needing that SBA
    /// implemented yet.
    pub fn net_with_counters(&self, counters: &CounterSet) -> Option<i32> {
        let base = self.as_fixed()?;
        Some(
            base + counters.count(&CounterKind::PlusOnePlusOne)
                - counters.count(&CounterKind::MinusOneMinusOne),
        )
    }
}

bitflags! {
    /// Bitmask of `CardType`. A card can be multiple types at once (e.g.
    /// artifact creature).
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub struct CardTypeSet: u16 {
        const ARTIFACT     = 1 << 0;
        const CREATURE     = 1 << 1;
        const ENCHANTMENT  = 1 << 2;
        const INSTANT      = 1 << 3;
        const LAND         = 1 << 4;
        const SORCERY      = 1 << 5;
        const KINDRED      = 1 << 6;
        /// Stub: not implemented v1, slot reserved per DESIGN.md non-goals.
        const PLANESWALKER = 1 << 7;
        /// Stub: not implemented v1, slot reserved per DESIGN.md non-goals.
        const BATTLE       = 1 << 8;
    }
}

/// Named card type, per CR 300. Use this for matching/comparison; use
/// `CardTypeSet` for storage on a card.
pub enum CardType {
    Artifact,
    Creature,
    Enchantment,
    Instant,
    Land,
    Sorcery,
    Kindred,
    Planeswalker,
    Battle,
}

bitflags! {
    /// Bitmask of `Supertype`, per CR 205.4a.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub struct SupertypeSet: u8 {
        const BASIC     = 1 << 0;
        const LEGENDARY = 1 << 1;
        const SNOW      = 1 << 2;
        const WORLD     = 1 << 3;
    }
}

pub enum Supertype {
    Basic,
    Legendary,
    Snow,
    World,
}

bitflags! {
    /// Bitmask of `Keyword`, per CR 702. Only the subset that changes how an
    /// existing engine checkpoint behaves (blocking legality, summoning
    /// sickness, timing, SBA lethal-checks, damage application) — this is
    /// deliberately NOT meant to grow to cover all ~140 CR 702 keywords.
    /// Keywords that are really just sugar for an activated/triggered
    /// ability (Cycling, Equip, Persist, Undying, ...) don't belong here at
    /// all; they're registered as an ordinary `RegisteredAbility` instead.
    /// See the TODO in `script_api.rs` for the still-unbuilt keyword-name ->
    /// primitive-or-ability mapping layer that would let a card's script
    /// just say `Game.GrantKeyword(card, "Flying")` instead of every card's
    /// script constructing a `KeywordSet` by hand.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub struct KeywordSet: u16 {
        /// CR 702.9 - evasion: blockable only by Flying/Reach.
        const FLYING        = 1 << 0;
        /// CR 702.17 - can block Flying (no evasion of its own).
        const REACH         = 1 << 1;
        /// CR 702.111 - can't be blocked except by two or more creatures.
        const MENACE        = 1 << 2;
        /// CR 702.20 - doesn't tap to attack.
        const VIGILANCE     = 1 << 3;
        /// CR 702.10 - ignores summoning sickness (attacking and tap-cost
        /// activated abilities).
        const HASTE         = 1 << 4;
        /// CR 702.3 - can't attack.
        const DEFENDER      = 1 << 5;
        /// CR 702.8 - castable any time its controller could cast an
        /// instant, regardless of the card's own type.
        const FLASH         = 1 << 6;
        /// CR 702.2 - any nonzero damage dealt by this source is lethal,
        /// regardless of amount.
        const DEATHTOUCH    = 1 << 7;
        /// CR 702.12 - ignores the lethal-damage SBA (704.5g); does NOT
        /// ignore the 0-toughness SBA (704.5f).
        const INDESTRUCTIBLE = 1 << 8;
        /// CR 702.15 - damage this source deals also gains its controller
        /// that much life.
        const LIFELINK      = 1 << 9;
        /// CR 702.80 - damage this source deals to a creature becomes
        /// `-1/-1` counters instead of marked damage.
        const WITHER        = 1 << 10;
        /// CR 702.90 - implies Wither's creature-damage behavior, and also
        /// turns damage to a player into poison counters instead of life
        /// loss.
        const INFECT        = 1 << 11;
    }
}

/// Named keyword, per CR 702. Use this for matching/comparison; use
/// `KeywordSet` for storage on a card.
pub enum Keyword {
    Flying,
    Reach,
    Menace,
    Vigilance,
    Haste,
    Defender,
    Flash,
    Deathtouch,
    Indestructible,
    Lifelink,
    Wither,
    Infect,
}

/// Newtype over the subtype string (creature types, land types, spell
/// subtypes, etc.). The full subtype space is large and open-ended (new
/// creature types are added over time), so this stays string-backed rather
/// than a fixed enum — the wrapper exists for type-fill/compile-time
/// distinction from arbitrary `String`s, not to enumerate every value.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Subtype(pub String);

impl Subtype {
    pub fn new(name: impl Into<String>) -> Self {
        // TODO: wrap name.into() in Subtype(_).
        todo!()
    }

    pub fn as_str(&self) -> &str {
        // TODO: return self.0.as_str().
        todo!()
    }
}

#[derive(Clone, Debug)]
pub enum ScriptReference {
    /// Lookup key into the external card-database project's script registry.
    ByKey(String),
    /// Literal Lua source, as supplied directly by the external DB project.
    Inline(String),
}

/// Immutable, shared. Sourced from the external card database project — this
/// engine consumes it, does not author or update it.
#[derive(Clone, Debug)]
pub struct CardDefinition {
    /// External DB's stable card identifier.
    pub definition_id: u64,
    pub name: String,
    pub mana_cost: ManaCost,
    pub color_indicator: Option<ColorSet>,
    pub supertypes: SupertypeSet,
    pub card_types: CardTypeSet,
    pub subtypes: Vec<Subtype>,
    pub oracle_text: String,
    /// Oracle/printed base power. `None` for non-creatures. This is the base
    /// value layer 7 starts from, not the live in-game value — see
    /// `EffectiveCharacteristics` in `effect.rs` for the latter (which is
    /// always a resolved `i32` — by query time any CDA has already run).
    pub power: Option<PTValue>,
    /// Oracle/printed base toughness. See `power` doc above.
    pub toughness: Option<PTValue>,
    /// Stub: planeswalker loyalty, unused v1.
    pub loyalty: Option<PTValue>,
    /// Stub: battle defense, unused v1.
    pub defense: Option<PTValue>,
    pub script_ref: ScriptReference,
    /// Printed CR 702 keywords this card has natively - see `KeywordSet`'s
    /// own doc comment for what is and isn't represented here.
    pub keywords: KeywordSet,
}

impl CardDefinition {
    /// Per CR 105.2a: color comes from mana cost symbols and/or an explicit
    /// `color_indicator`, not stored as its own redundant field.
    pub fn colors(&self) -> ColorSet {
        // TODO: CR 105.2a: if color_indicator is Some, use it as-is.
        // Otherwise union mana_cost.colors() across all symbols (delegate to
        // ManaCost::colors, mana.rs). A card with no colored symbols and no
        // indicator is colorless (empty ColorSet), not an error.
        todo!()
    }

    pub fn has_type(&self, card_type: CardType) -> bool {
        // TODO: map card_type to its CardTypeSet bit and check
        // self.card_types.contains(that bit).
        todo!()
    }

    pub fn has_supertype(&self, supertype: Supertype) -> bool {
        // TODO: same pattern as has_type, against self.supertypes.
        todo!()
    }

    pub fn has_subtype(&self, subtype: &str) -> bool {
        // TODO: self.subtypes.iter().any(|s| s.as_str() == subtype).
        todo!()
    }
}
