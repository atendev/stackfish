use crate::types::counter::CounterKind;
use crate::types::ids::{ObjectId, PlayerId};
use crate::types::mana::ManaPool;

/// Emblems live in a player's command zone (CR 114.1) but aren't cards from
/// the external database — no `CardDefinition`, no mana cost/types, never
/// move zones, never get targeted. Modeling them as a lightweight struct
/// directly on the player rather than forcing them through the
/// `CardDefinition`-backed `GameObject` model, which would need a definition
/// they don't have. Revisit if the zone model ever needs to enumerate
/// "everything in the command zone" uniformly.
pub struct Emblem {
    pub emblem_id: ObjectId,
    pub source: ObjectId,
    pub ability_text: String,
}

pub struct PlayerState {
    pub player_id: PlayerId,
    pub life: i32,
    pub mana_pool: ManaPool,
    /// Player-level counters: poison, energy, experience, etc. (CR 121.5).
    pub counters: Vec<(CounterKind, i32)>,
    pub emblems: Vec<Emblem>,
    /// Commander-specific: cumulative combat damage taken from each commander
    /// source. 21+ from a single source means this player loses (CR 903.10a).
    pub commander_damage_taken: Vec<(ObjectId, u32)>,
    pub eliminated: bool,
}
