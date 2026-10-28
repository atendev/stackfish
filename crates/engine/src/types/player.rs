use crate::types::cost::FilterController;
use crate::types::counter::CounterSet;
use crate::types::event::GameEventKind;
use crate::types::game::GameState;
use crate::types::ids::{ObjectId, PlayerId, SequenceNo};
use crate::types::mana::ManaPool;
use crate::types::zone::ZoneKind;

/// CR 114.1: an emblem lives in the command zone but is not a card, so it has
/// no `CardDefinition`, never moves zones and is never targeted. A plain
/// struct on the player avoids a `GameObject` with no definition.
#[derive(Clone, PartialEq, Debug)]
pub struct Emblem {
    pub emblem_id: ObjectId,
    pub source: ObjectId,
    pub ability_text: String,
}

impl Emblem {
    pub fn new(emblem_id: ObjectId, source: ObjectId, ability_text: impl Into<String>) -> Self {
        Emblem { emblem_id, source, ability_text: ability_text.into() }
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct PlayerState {
    pub player_id: PlayerId,
    pub life: i32,
    pub mana_pool: ManaPool,
    /// Player-level counters: poison, energy, experience, etc. (CR 121.5).
    pub counters: CounterSet,
    pub emblems: Vec<Emblem>,
    /// Cumulative commander combat damage taken per source (CR 903.10a, lose
    /// at 21). Keyed by `CardDefinition.name` because CR 400.7 mints a new id
    /// on every cast and return. Partners have different names, so they get
    /// separate entries.
    pub commander_damage_taken: Vec<(String, u32)>,
    pub eliminated: bool,
    /// CR 704.5b, 120.3: set by `commands::propose_draw` on a draw from an
    /// empty library. `sba::apply_player_loss` eliminates the player on the
    /// next pass. Never cleared.
    pub drew_from_empty_library: bool,
    /// Mulligans taken in pregame (CR 103.5), read once to size the bottoming
    /// step.
    pub mulligans_taken: u32,
    /// Set by `KeepHand`. `next_mulligan_player` skips players with this set.
    pub hand_kept: bool,
    /// CR 903.8: times each commander was cast from the command zone, keyed
    /// by `CardDefinition.name` for the same reason as
    /// `commander_damage_taken` (Partners, CR 702.124d).
    pub commander_casts: std::collections::HashMap<String, u32>,
}

impl PlayerState {
    pub fn new(player_id: PlayerId, starting_life: i32) -> Self {
        PlayerState {
            player_id,
            life: starting_life,
            mana_pool: ManaPool::default(),
            counters: CounterSet::new(),
            emblems: vec![],
            commander_damage_taken: vec![],
            eliminated: false,
            drew_from_empty_library: false,
            mulligans_taken: 0,
            hand_kept: false,
            commander_casts: std::collections::HashMap::new(),
        }
    }

    pub fn lose_life(&mut self, amount: i32) {
        // CR 119, 704.5a: no clamp, since the SBA checks for 0 or less.
        self.life -= amount;
    }

    pub fn gain_life(&mut self, amount: i32) {
        self.life += amount;
    }

    /// CR 903.10a: commander damage from one source, so the SBA loop does not
    /// repeat the 21 threshold. Takes the commander's name
    /// (`object.definition.name`), not an id.
    pub fn commander_damage_from(&self, source_name: &str) -> u32 {
        self.commander_damage_taken.iter().find(|(s, _)| s == source_name).map(|(_, n)| *n).unwrap_or(0)
    }
}

/// CR 402.2: "no maximum hand size" or "maximum hand size is N". There is no
/// relative shape, since no real card says "two more".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandSizeEffect {
    NoMaximum,
    SetTo(u32),
}

/// CR 402.2, 303.4m: which players a `HandSizeModifier` affects.
/// `EnchantedPlayer` resolves against `source`'s live `attached_to_player`.
/// It is a sibling of `FilterController` because that is the CR 108.4
/// you/opponent/any vocabulary, unrelated to attachment (as with
/// `AffectedFilter::EnchantedOnly`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HandSizeAffected {
    Filter(FilterController),
    EnchantedPlayer,
}

/// CR 402.2: a static maximum-hand-size effect. Not a CR 613 layer, because
/// hand size is not a characteristic (as with `CostModifier`). `controller` is
/// whose "you" the text uses, and `affected` is who it hits.
#[derive(Clone, Debug, PartialEq)]
pub struct HandSizeModifier {
    pub source: ObjectId,
    pub controller: PlayerId,
    pub affected: HandSizeAffected,
    pub effect: HandSizeEffect,
    /// "Latest wins" tiebreak between conflicting modifiers (CR 613.7 style).
    /// No CR 613.8 dependency analysis applies outside the layers.
    pub timestamp: SequenceNo,
}

/// CR 402.2: `player`'s maximum hand size, `None` for no maximum. Modifiers
/// do not stack, and the latest `timestamp` wins. The caller supplies `base`
/// (normally 7). `state` is used only for `HandSizeAffected::EnchantedPlayer`.
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

/// CR 701.20, 701.18c, 303.4m: which players a `HandRevealModifier` affects.
/// A separate type from `HandSizeAffected` despite the same shape, because
/// they serve unrelated rules. Extract a shared type if a third mechanism
/// needs it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HandRevealAffected {
    Filter(FilterController),
    EnchantedPlayer,
}

/// CR 701.20a, 701.18c: a static "plays with hand revealed" effect, such as
/// Telepathy (CR 701.20c). Not a CR 613 layer. It has no `effect` field,
/// because any applicable modifier means revealed.
///
/// Nothing enforces it yet: `VisibilityPolicy::is_visible_to` has no callers,
/// and `script::encode::state_to_lua` encodes only `hand_count`, never hand
/// contents. Only `is_hand_revealed`'s unit tests exercise it.
#[derive(Clone, Debug, PartialEq)]
pub struct HandRevealModifier {
    pub source: ObjectId,
    pub controller: PlayerId,
    pub affected: HandRevealAffected,
    /// Kept for consistency with `HandSizeModifier::timestamp`.
    /// `is_hand_revealed` ORs every modifier, so nothing breaks ties.
    pub timestamp: SequenceNo,
}

/// CR 701.20a: whether `player`'s hand is revealed to all players. An OR
/// across applicable modifiers, since reveal has no magnitude to conflict
/// (CR 701.20c). There is no `viewer` parameter: reveal shows to all, unlike
/// a "look at" effect (CR 701.20e).
pub fn is_hand_revealed(state: &GameState, modifiers: &[HandRevealModifier], player: PlayerId) -> bool {
    modifiers.iter().any(|m| match m.affected {
        HandRevealAffected::Filter(FilterController::You) => player == m.controller,
        HandRevealAffected::Filter(FilterController::Opponent) => player != m.controller,
        HandRevealAffected::Filter(FilterController::Any) => true,
        HandRevealAffected::EnchantedPlayer => {
            state.object(m.source).is_some_and(|o| o.attached_to_player == Some(player))
        }
    })
}

/// CR 121.2b: which players a `DrawLimitModifier` affects. Only `Filter`,
/// because no real card needs an `EnchantedPlayer` form. Add one like
/// `HandRevealAffected` if one appears.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DrawLimitAffected {
    Filter(FilterController),
}

/// CR 121.2b: a static "can't draw more than one card each turn" effect. Not
/// a CR 613 layer. `commands::propose_draw` enforces it, the one choke point
/// for `Engine::continue_draw_cards` and `Step::Draw`.
#[derive(Clone, Debug, PartialEq)]
pub struct DrawLimitModifier {
    pub source: ObjectId,
    pub controller: PlayerId,
    pub affected: DrawLimitAffected,
    /// CR 121.2b: the cap per turn, a magnitude and not a hardcoded 1.
    pub limit: u32,
    /// "Latest wins" tiebreak, as in `HandSizeModifier::timestamp`.
    pub timestamp: SequenceNo,
}

/// CR 121.2b: `player`'s per-turn draw limit, `None` for none. The latest
/// timestamp wins, as in `effective_max_hand_size`.
pub fn effective_draw_limit(modifiers: &[DrawLimitModifier], player: PlayerId) -> Option<u32> {
    modifiers
        .iter()
        .filter(|m| match m.affected {
            DrawLimitAffected::Filter(FilterController::You) => player == m.controller,
            DrawLimitAffected::Filter(FilterController::Opponent) => player != m.controller,
            DrawLimitAffected::Filter(FilterController::Any) => true,
        })
        .max_by_key(|m| m.timestamp.0)
        .map(|m| m.limit)
}

/// CR 305.2: which players a `LandPlayModifier` affects. Only `Filter`, as in
/// `DrawLimitAffected`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LandPlayAffected {
    Filter(FilterController),
}

/// CR 305.2: a static "play an additional land" effect (Exploration, Azusa).
/// Not a CR 613 layer. Unlike `DrawLimitModifier`, increases stack (two
/// Explorations give 3 land plays), so `effective_land_play_limit` sums them
/// and there is no `timestamp`. `commands::retire_object` purges this list
/// and its sibling modifier lists by `source` once the permanent leaves the
/// battlefield (CR 611.3b).
#[derive(Clone, Debug, PartialEq)]
pub struct LandPlayModifier {
    pub source: ObjectId,
    pub controller: PlayerId,
    pub affected: LandPlayAffected,
    /// Additional land plays granted, a magnitude and not a hardcoded 1.
    pub additional: u32,
}

/// CR 305.2: `player`'s land-play limit, `1` plus the sum of applicable
/// `additional`. Stacking, unlike `effective_draw_limit`.
pub fn effective_land_play_limit(modifiers: &[LandPlayModifier], player: PlayerId) -> u32 {
    1 + modifiers
        .iter()
        .filter(|m| match m.affected {
            LandPlayAffected::Filter(FilterController::You) => player == m.controller,
            LandPlayAffected::Filter(FilterController::Opponent) => player != m.controller,
            LandPlayAffected::Filter(FilterController::Any) => true,
        })
        .map(|m| m.additional)
        .sum::<u32>()
}

/// CR 121.2b: cards `player` has drawn this turn, counted from the ledger
/// after the latest `TurnStarted` (the same scan as
/// `replacement::opponent_damaged_this_turn`). Counts library-to-hand
/// `ZoneChange` events by `last_known_info.owner`, since the card may no
/// longer be in `state.objects`.
pub fn cards_drawn_this_turn(state: &GameState, player: PlayerId) -> u32 {
    let turn_start = state.ledger.iter().rposition(|e| matches!(e.kind, GameEventKind::TurnStarted { .. }));
    let scan_from = turn_start.map(|i| i + 1).unwrap_or(0);
    state.ledger[scan_from..]
        .iter()
        .filter(|e| {
            matches!(
                &e.kind,
                GameEventKind::ZoneChange { from: ZoneKind::Library, to: ZoneKind::Hand, last_known_info: Some(info), .. }
                    if info.owner == player
            )
        })
        .count() as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ids::ObjectId;

    #[test]
    fn player_state_new_initializes_fields() {
        let p = PlayerState::new(PlayerId(0), 20);
        assert_eq!(p.player_id, PlayerId(0));
        assert_eq!(p.life, 20);
        assert_eq!(p.mana_pool, ManaPool::default());
        assert_eq!(p.counters.count(&crate::types::counter::CounterKind::Poison), 0);
        assert!(p.emblems.is_empty());
        assert!(p.commander_damage_taken.is_empty());
        assert!(!p.eliminated);
        assert_eq!(p.mulligans_taken, 0);
        assert!(!p.hand_kept);
    }

    #[test]
    fn lose_life_and_gain_life_adjust_life() {
        let mut p = PlayerState::new(PlayerId(0), 20);
        p.lose_life(3);
        assert_eq!(p.life, 17);
        p.gain_life(5);
        assert_eq!(p.life, 22);
    }

    #[test]
    fn commander_damage_from_reads_entry_or_zero() {
        let mut p = PlayerState::new(PlayerId(0), 20);
        p.commander_damage_taken.push(("K commanders".to_string(), 7));
        assert_eq!(p.commander_damage_from("K commanders"), 7);
        assert_eq!(p.commander_damage_from("Other"), 0);
    }

    #[test]
    fn emblem_new_stores_fields() {
        let e = Emblem::new(ObjectId(0), ObjectId(1), "Test emblem");
        assert_eq!(e.emblem_id, ObjectId(0));
        assert_eq!(e.source, ObjectId(1));
        assert_eq!(e.ability_text, "Test emblem");
    }

    #[test]
    fn effective_draw_limit_latest_timestamp_wins() {
        let source = ObjectId(0);
        let p0 = PlayerId(0);
        let modifiers = vec![
            DrawLimitModifier { source, controller: p0, affected: DrawLimitAffected::Filter(FilterController::You), limit: 5, timestamp: SequenceNo(1) },
            DrawLimitModifier { source, controller: p0, affected: DrawLimitAffected::Filter(FilterController::You), limit: 2, timestamp: SequenceNo(2) },
        ];
        assert_eq!(effective_draw_limit(&modifiers, p0), Some(2));
    }

    #[test]
    fn effective_draw_limit_filters_by_controller_relation() {
        let source = ObjectId(0);
        let p0 = PlayerId(0);
        let p1 = PlayerId(1);
        let modifiers = vec![
            DrawLimitModifier { source, controller: p0, affected: DrawLimitAffected::Filter(FilterController::You), limit: 1, timestamp: SequenceNo(0) },
            DrawLimitModifier { source, controller: p0, affected: DrawLimitAffected::Filter(FilterController::Opponent), limit: 2, timestamp: SequenceNo(0) },
            DrawLimitModifier { source, controller: p0, affected: DrawLimitAffected::Filter(FilterController::Any), limit: 3, timestamp: SequenceNo(0) },
        ];
        // Latest among You (1) and Any (3) is 3.
        assert_eq!(effective_draw_limit(&modifiers, p0), Some(3));
        // Latest among Opponent (2) and Any (3) is 3.
        assert_eq!(effective_draw_limit(&modifiers, p1), Some(3));
    }

    #[test]
    fn effective_draw_limit_none_when_no_match() {
        assert_eq!(effective_draw_limit(&[], PlayerId(0)), None);
    }

    #[test]
    fn effective_land_play_limit_sums_additional_land_plays() {
        let source = ObjectId(0);
        let p0 = PlayerId(0);
        let p1 = PlayerId(1);
        let modifiers = vec![
            LandPlayModifier { source, controller: p0, affected: LandPlayAffected::Filter(FilterController::You), additional: 1 },
            LandPlayModifier { source, controller: p0, affected: LandPlayAffected::Filter(FilterController::You), additional: 2 },
            LandPlayModifier { source, controller: p0, affected: LandPlayAffected::Filter(FilterController::Opponent), additional: 5 },
        ];
        // CR 305.2: base 1 plus all applicable additional plays for the player.
        assert_eq!(effective_land_play_limit(&modifiers, p0), 4);
        assert_eq!(effective_land_play_limit(&modifiers, p1), 6);
    }

    #[test]
    fn effective_land_play_limit_any_applies_to_every_player() {
        let source = ObjectId(0);
        let modifiers = vec![
            LandPlayModifier { source, controller: PlayerId(0), affected: LandPlayAffected::Filter(FilterController::Any), additional: 3 },
        ];
        assert_eq!(effective_land_play_limit(&modifiers, PlayerId(0)), 4);
        assert_eq!(effective_land_play_limit(&modifiers, PlayerId(1)), 4);
    }

    #[test]
    fn effective_land_play_limit_default_is_one() {
        assert_eq!(effective_land_play_limit(&[], PlayerId(0)), 1);
    }
}
