use bitflags::bitflags;

use crate::types::mana::{ColorSet, ManaCost};

/// Printed/base value for power, toughness, loyalty, or defense. `*`-style
/// values (and `* + 1` etc — the modifier is folded into whatever number the
/// characteristic-defining ability script returns, there's no separate
/// "base star plus offset" case to represent) are characteristic-defining
/// abilities: the number isn't known until the script callback runs against
/// current game state, so it can't be stored as a plain `i32`.
pub enum PTValue {
    Fixed(i32),
    CharacteristicDefining(String),
}

bitflags! {
    /// Bitmask of `CardType`. A card can be multiple types at once (e.g.
    /// artifact creature).
    #[derive(Clone, Copy, PartialEq, Eq)]
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
    #[derive(Clone, Copy, PartialEq, Eq)]
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

/// Newtype over the subtype string (creature types, land types, spell
/// subtypes, etc.). The full subtype space is large and open-ended (new
/// creature types are added over time), so this stays string-backed rather
/// than a fixed enum — the wrapper exists for type-fill/compile-time
/// distinction from arbitrary `String`s, not to enumerate every value.
pub struct Subtype(pub String);

pub enum ScriptReference {
    /// Lookup key into the external card-database project's script registry.
    ByKey(String),
    /// Literal Lua source, as supplied directly by the external DB project.
    Inline(String),
}

/// Immutable, shared. Sourced from the external card database project — this
/// engine consumes it, does not author or update it.
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
}
