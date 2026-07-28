use crate::types::cost::FilterController;
use crate::types::counter::CounterSet;
use crate::types::game::GameState;
use crate::types::ids::{ObjectId, PlayerId, SequenceNo};
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
    /// CR 704.5b/120.3: set by `commands::propose_draw` when a draw is
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

/// CR 402.2's two real shapes: "you have no maximum hand size" and "your
/// maximum hand size is N" — deliberately not a third generic "delta" shape
/// (no real card says "your maximum hand size is two more"), matching this
/// engine's "model the shapes that exist" stance elsewhere.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandSizeEffect {
    NoMaximum,
    SetTo(u32),
}

/// CR 402.2/303.4m: which player(s) a `HandSizeModifier` affects — either
/// the same `FilterController` (you/opponent/any) perspective `CostModifier`
/// already uses, or `EnchantedPlayer`, resolved fresh against `source`'s own
/// live `attached_to_player` every time (CR 303.4m: "an ability of a
/// permanent that refers to the 'enchanted player' refers to whatever player
/// that permanent is attached to"). A sibling variant, not a widened
/// `FilterController` itself — `FilterController` is CR 108.4's own "you"/
/// "opponent"/"any" vocabulary, unrelated to attachment, and this engine's
/// established precedent (`AffectedFilter::EnchantedOnly`, `phase5/enchanted-
/// reference`) already treats "enchanted" as a separate concept from a
/// controller-relative filter rather than folding it into one. Mirrors
/// `AffectedFilter::EnchantedOnly`'s own `source.attached_to == Some(object.
/// object_id)` check exactly, just the player-attach sibling field
/// (`attached_to_player`) and resolved via `GameState` lookup rather than an
/// already-borrowed `&GameObject`, since `effective_max_hand_size` only ever
/// had a bare `PlayerId` to check against before this variant existed.
#[derive(Clone, Copy, Debug)]
pub enum HandSizeAffected {
    Filter(FilterController),
    EnchantedPlayer,
}

/// CR 402.2: a static "you/each player/your opponents have no maximum hand
/// size"-style effect — deliberately **not** a `ContinuousEffect`/CR 613
/// layer, same reasoning `cost.rs`'s `CostModifier` already established for
/// cost modification: hand size isn't a characteristic query, and `Layer` is
/// meant to be exactly the CR 613.1 layer list, nothing else. `controller`/
/// `affected` mirror `CostModifier`'s own `controller`/`filter.controller`
/// split exactly — whose "you"/"opponent" perspective this modifier's text
/// is written from, and which player(s) it actually affects.
#[derive(Clone, Debug)]
pub struct HandSizeModifier {
    pub source: ObjectId,
    pub controller: PlayerId,
    pub affected: HandSizeAffected,
    pub effect: HandSizeEffect,
    /// CR 613.7-style "latest wins" tiebreak between two conflicting
    /// modifiers that both apply to the same player — hand size isn't a CR
    /// 613 layer at all, so there's no dependency (613.8) analysis to do,
    /// only this same timestamp-order convention every other flat-override
    /// continuous effect in this engine already uses.
    pub timestamp: SequenceNo,
}

/// CR 402.2: `player`'s current maximum hand size, given whatever
/// `HandSizeModifier`s currently apply — `None` means "no maximum hand
/// size." Multiple applicable modifiers don't stack; the one with the
/// latest `timestamp` wins outright (see `HandSizeModifier::timestamp`'s own
/// doc comment). `base` is normally 7 (CR 402.2) — the caller's problem, not
/// this function's, same "base value supplied by the caller" split
/// `cost::apply_cost_modifiers` already uses for `ManaCost`. `state` is only
/// needed to resolve `HandSizeAffected::EnchantedPlayer` (`phase5/enchanted-
/// player-hand-size`) — every other `affected` shape ignores it.
pub fn effective_max_hand_size(state: &GameState, modifiers: &[HandSizeModifier], player: PlayerId, base: u32) -> Option<u32> {
    let winner = modifiers
        .iter()
        .filter(|m| match m.affected {
            HandSizeAffected::Filter(FilterController::You) => player == m.controller,
            HandSizeAffected::Filter(FilterController::Opponent) => player != m.controller,
            HandSizeAffected::Filter(FilterController::Any) => true,
            HandSizeAffected::EnchantedPlayer => {
                state.object(m.source).is_some_and(|o| o.attached_to_player == Some(player))
            }
        })
        .max_by_key(|m| m.timestamp.0);
    match winner {
        Some(m) => match m.effect {
            HandSizeEffect::NoMaximum => None,
            HandSizeEffect::SetTo(n) => Some(n),
        },
        None => Some(base),
    }
}
