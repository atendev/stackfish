use crate::types::counter::CounterSet;
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

impl Emblem {
    pub fn new(emblem_id: ObjectId, source: ObjectId, ability_text: impl Into<String>) -> Self {
        // TODO: Emblem { emblem_id, source, ability_text: ability_text.into() }.
        todo!()
    }
}

pub struct PlayerState {
    pub player_id: PlayerId,
    pub life: i32,
    pub mana_pool: ManaPool,
    /// Player-level counters: poison, energy, experience, etc. (CR 121.5).
    pub counters: CounterSet,
    pub emblems: Vec<Emblem>,
    /// Commander-specific: cumulative combat damage taken from each commander
    /// source. 21+ from a single source means this player loses (CR 903.10a).
    pub commander_damage_taken: Vec<(ObjectId, u32)>,
    pub eliminated: bool,
    /// CR 704.5b/120.3: set by `Engine::draw_one_card` when a draw is
    /// attempted against an empty (or missing) library zone. Once `true`,
    /// `sba::apply_player_loss` eliminates this player on the next SBA pass
    /// - sticky, never cleared once set (same permanence 704.5a's
    /// life-based elimination already has).
    pub drew_from_empty_library: bool,
    /// Number of mulligans this player took during pregame (CR 103.5). Used
    /// after the player keeps their hand to determine how many cards they must
    /// put on the bottom of their library. Reset is not needed — this value is
    /// only read once (during the bottoming step) and the pregame phase never
    /// repeats.
    pub mulligans_taken: u32,
    /// Set to `true` when this player submits `KeepHand` during pregame.
    /// `next_mulligan_player` skips players where this is `true`; all players
    /// start `false` so they all get a decision before the first turn.
    pub hand_kept: bool,
    /// CR 903.8 (commander tax): how many times this player has cast each
    /// commander from the command zone so far this game, keyed by
    /// `CardDefinition.name` rather than `ObjectId` — CR 400.7 mints a new
    /// id every cast (and every 903.9a return), so an id can't track "the
    /// same commander" across a cast/death/return/recast cycle the way a
    /// card's name can. Partner commanders (CR 702.124d) get independent
    /// entries automatically, since they have different names.
    pub commander_casts: std::collections::HashMap<String, u32>,
}

impl PlayerState {
    pub fn new(player_id: PlayerId, starting_life: i32) -> Self {
        // TODO: PlayerState { player_id, life: starting_life, mana_pool:
        // ManaPool::empty(), counters: CounterSet::new(), emblems: vec![],
        // commander_damage_taken: vec![], eliminated: false,
        // drew_from_empty_library: false, mulligans_taken: 0, hand_kept: false,
        // commander_casts: HashMap::new() }.
        todo!()
    }

    pub fn lose_life(&mut self, amount: i32) {
        // CR 119: negative life isn't special-cased in the rules text - 0-or-
        // less life is what the CR 704.5a SBA checks for, so this just
        // subtracts; no clamping to 0 here.
        self.life -= amount;
    }

    pub fn gain_life(&mut self, amount: i32) {
        self.life += amount;
    }

    /// Commander damage from a single source reaching 21 means this player
    /// loses (CR 903.10a) — exposed as a query so the SBA loop can check it
    /// without duplicating the threshold elsewhere.
    pub fn commander_damage_from(&self, source: ObjectId) -> u32 {
        self.commander_damage_taken.iter().find(|(s, _)| *s == source).map(|(_, n)| *n).unwrap_or(0)
    }
}
