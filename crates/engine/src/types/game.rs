use std::collections::HashMap;

use crate::types::ability::RegisteredAbility;
use crate::types::combat::CombatState;
use crate::types::cost::CostModifier;
use crate::types::effect::{ContinuousEffect, ReplacementEffect};
use crate::types::event::GameEvent;
use crate::types::ids::{AbilityId, EventId, ObjectId, PlayerId, SequenceNo};
use crate::types::object::GameObject;
use crate::types::player::{
    DrawLimitModifier, HandRevealModifier, HandSizeModifier, LandPlayModifier, PlayerState,
};
use crate::types::stack::StackObject;
use crate::types::turn::{Phase, Step};
use crate::types::zone::{Zone, ZoneKind};

/// Seeded once at game start. Every draw is a logged `RngDrawn` event, so
/// replay is exact.
#[derive(Clone, PartialEq, Debug)]
pub struct RngState {
    pub seed: u64,
    pub draws_consumed: u64,
}

/// One "exile until it leaves the battlefield" link (CR 610.3). `exiled` is
/// the card's id in exile (a new object, CR 400.7); `source` is the
/// permanent whose departure returns it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LinkedExile {
    pub source: ObjectId,
    pub exiled: ObjectId,
}

/// Owns the full mutable state for one game. Player count is a runtime
/// parameter — not hardcoded to 2 or 4 — to support N-player Commander.
#[derive(Clone, PartialEq, Debug)]
pub struct GameState {
    pub players: Vec<PlayerState>,
    /// Turn order as a list of player indices into `players`; APNAP order is
    /// this list rotated to start at the active player.
    pub turn_order: Vec<PlayerId>,
    pub active_player: PlayerId,
    pub turn_number: u32,
    pub phase: Phase,
    pub step: Step,
    /// Priority holder and consecutive passes (CR 117.3a, 117.4), which
    /// `turn_order` and `active_player` cannot show.
    pub priority_player: PlayerId,
    pub consecutive_passes: u32,
    /// CR 305.2, 305.2a: lands each player has played this turn, against the
    /// limit from `land_play_modifiers`. Reset when a new turn begins. The
    /// modifiers persist for as long as their source does.
    pub lands_played_this_turn: HashMap<PlayerId, u32>,
    /// CR 506-509: `None` outside combat, `Some` from `DeclareAttackers`
    /// onward, cleared at `EndOfCombat` (`engine::Engine::advance_turn_step`)
    /// so it never leaks into the next turn's combat.
    pub combat: Option<CombatState>,
    /// CR 302.6, 602.5a: `turn_number` when each player most recently became
    /// the active player, written by `advance_turn_step` on each rotation and
    /// once for the first turn. `commands::has_summoning_sickness` compares it
    /// with `GameObject.controlled_since_turn`.
    pub last_active_turn_number: HashMap<PlayerId, u32>,
    /// Single source of truth for object data. `zones` holds only ids, so it
    /// alone owns order.
    pub objects: HashMap<ObjectId, GameObject>,
    /// Every per-player zone plus the shared battlefield and exile, flat,
    /// because `Zone.owner` says whose it is. The stack is separate in `stack`
    /// because it needs `StackObject` metadata.
    pub zones: Vec<Zone>,
    pub stack: Vec<StackObject>,
    pub continuous_effects: Vec<ContinuousEffect>,
    pub replacement_effects: Vec<ReplacementEffect>,
    /// CR 601.2f, 118.9: cost modifiers (Goblin Warchief, Thalia). A separate
    /// mechanism from the CR 613 layers that the two lists above feed. See
    /// `CostModifier`.
    pub cost_modifiers: Vec<CostModifier>,
    /// CR 402.2: max-hand-size effects. See `HandSizeModifier`.
    pub hand_size_modifiers: Vec<HandSizeModifier>,
    /// CR 701.20a, 701.18c: "plays with hand revealed" effects. See
    /// `HandRevealModifier`. Nothing enforces them yet.
    pub hand_reveal_modifiers: Vec<HandRevealModifier>,
    /// CR 121.2b: draw-limit effects. See `DrawLimitModifier`.
    pub draw_limit_modifiers: Vec<DrawLimitModifier>,
    /// CR 305.2: "play an additional land" effects, summed instead of taking
    /// the latest timestamp (see `LandPlayModifier`). Read by
    /// `commands::execute_play_land`.
    pub land_play_modifiers: Vec<LandPlayModifier>,
    /// Registered triggered, activated and static abilities, flat.
    pub abilities: Vec<RegisteredAbility>,
    pub ledger: Vec<GameEvent>,
    pub rng: RngState,
    /// Monotonic counter for `EventId`s. Not `ledger.len()`, because several
    /// events can be in flight before they land. See `allocate_event_id`.
    pub next_event_id: u64,
    /// Monotonic counter for `SequenceNo`. `ledger.len()` varies with how
    /// `CommitEvents` batches, which would break replay. See
    /// `allocate_sequence_no`.
    pub next_sequence_no: u64,
    /// Monotonic counter for `ObjectId`s, since each CR 400.7 new object needs
    /// a fresh id. See `allocate_object_id`.
    pub next_object_id: u64,
    /// CR 500.7, 500.8: queued (player, step) skips, consumed by
    /// `consume_step_skip` at the top of `AdvanceTurnStep`. Card effects push
    /// here, and the CR 103.8a two-player turn-1 draw skip is seeded at game
    /// start.
    pub pending_step_skips: Vec<(PlayerId, Step)>,
    /// CR 603.2h: abilities that already triggered this turn, for
    /// `ConditionSpec::PrimitiveOncePerTurn`. Filled by
    /// `triggers::collect_triggers` and reset with `lands_played_this_turn`.
    pub once_per_turn_triggered: std::collections::HashSet<AbilityId>,
    /// CR 903.9a: commander ids in a graveyard or exile already offered the
    /// return to the command zone, so a declined offer is not repeated on each
    /// SBA pass. An accepted return mints a new id (CR 400.7). Never pruned.
    pub commander_return_offered: std::collections::HashSet<ObjectId>,
    /// CR 707.9e, 707.9g: tags a `RegisteredAbility` with the
    /// `copy_generation` its source had when a copy effect granted it. A side
    /// map because `RegisteredAbility` has no `Default` and 100+ literal
    /// sites. `triggers::collect_triggers` matches a tagged ability only while
    /// the source's current generation equals the tag. Never pruned, since a
    /// stale tag just stops matching.
    ///
    /// Nothing writes it yet, because no `CopyExceptions` shape grants an
    /// ability. CR 707.9e's "additional effect" exception has no consumer
    /// either.
    pub linked_ability_generation: std::collections::HashMap<AbilityId, u32>,
    /// Abilities that `ScriptRuntime::run_card_script` registered, as opposed
    /// to hand-built ones (a token's Clue, a copy's granted trigger, test
    /// fixtures). `commands::remap_abilities` drops them on a zone change
    /// because the script reruns for the new zone and carrying them would
    /// register everything twice. A side set because `RegisteredAbility` has
    /// ~160 literal sites.
    pub script_ability_ids: std::collections::HashSet<AbilityId>,
    /// CR 113.6: zone a script-registered ability declared it functions from
    /// (`:SetFunctionsFrom("Graveyard")`). Absent means the battlefield, or
    /// the zone its cost implies, such as Cycling's hand. A side map for the
    /// same reason as `script_ability_ids`, and pruned with it.
    pub ability_function_zones: std::collections::HashMap<AbilityId, ZoneKind>,
    /// Objects in the graveyard or exile whose card script already ran for
    /// that zone (`Engine::sync_off_battlefield_scripts`). One sweep per
    /// trigger-collection pass covers every producer. Pruned by
    /// `commands::retire_object`.
    pub off_battlefield_scripts_run: std::collections::HashSet<ObjectId>,
    /// CR 610.3: cards exiled "until [source] leaves the battlefield". Pushed
    /// when the exile commits, drained by `Engine::return_linked_exiles` once
    /// the source is off the battlefield. `commands::retire_object` drops an
    /// entry whose `exiled` card left exile some other way.
    pub linked_exiles: Vec<LinkedExile>,
    /// CR 702.35b, 601.3: one-shot timing waiver, set before
    /// `Engine::submit_madness_cast` queues its `CastSpell` and consumed by the
    /// sorcery-speed gate in `commands::cast::execute_cast_spell`. A side
    /// channel because `CastSpell` has ~87 construction sites. Taken
    /// unconditionally so it cannot leak into a later cast of the same card.
    pub pending_timing_bypass: Option<ObjectId>,
    /// CR 702.85a: one-shot grant to cast this object for no mana. Set with
    /// `pending_timing_bypass` by `Engine::submit_cascade_cast` and consumed by
    /// `commands::cast::execute_cast_spell`, which builds a transient `{0}`
    /// `AlternativeCost::Replacement` because a library card has no Cascade
    /// cost of its own. Madness waives only timing, since its cost is real.
    pub pending_free_cast: Option<ObjectId>,
}

impl GameState {
    pub fn allocate_event_id(&mut self) -> EventId {
        let id = EventId(self.next_event_id);
        self.next_event_id += 1;
        id
    }

    pub fn allocate_sequence_no(&mut self) -> SequenceNo {
        let seq = SequenceNo(self.next_sequence_no);
        self.next_sequence_no += 1;
        seq
    }

    pub fn allocate_object_id(&mut self) -> ObjectId {
        let id = ObjectId(self.next_object_id);
        self.next_object_id += 1;
        id
    }

    /// Mints a fresh `AbilityId` from the sequence counter. See `AbilityId`.
    pub fn allocate_ability_id(&mut self) -> AbilityId {
        AbilityId(self.allocate_sequence_no().0)
    }

    pub fn object(&self, id: ObjectId) -> Option<&GameObject> {
        self.objects.get(&id)
    }

    pub fn object_mut(&mut self, id: ObjectId) -> Option<&mut GameObject> {
        self.objects.get_mut(&id)
    }

    pub fn player(&self, id: PlayerId) -> Option<&PlayerState> {
        self.players.iter().find(|p| p.player_id == id)
    }

    pub fn player_mut(&mut self, id: PlayerId) -> Option<&mut PlayerState> {
        self.players.iter_mut().find(|p| p.player_id == id)
    }

    /// The one `Zone` matching `kind` and `owner`. More than one match means
    /// `zones` was built wrong.
    // TODO: enforce that at most one zone matches
    pub fn zone(&self, kind: ZoneKind, owner: Option<PlayerId>) -> Option<&Zone> {
        self.zones
            .iter()
            .find(|z| z.kind == kind && z.owner == owner)
    }

    pub fn zone_mut(&mut self, kind: ZoneKind, owner: Option<PlayerId>) -> Option<&mut Zone> {
        self.zones
            .iter_mut()
            .find(|z| z.kind == kind && z.owner == owner)
    }

    /// CR 500.7, 500.8: returns whether `player`'s current `step` is skipped,
    /// consuming the entry. A skipped step runs no turn-based actions and
    /// opens no priority window.
    pub(crate) fn consume_step_skip(&mut self, player: PlayerId, step: Step) -> bool {
        if let Some(pos) = self
            .pending_step_skips
            .iter()
            .position(|(p, s)| *p == player && *s == step)
        {
            self.pending_step_skips.remove(pos);
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ids::ObjectId;
    use std::collections::VecDeque;

    // Pins the zone helpers independently of the command layer.
    #[test]
    fn zone_helpers_roundtrip() {
        let hand = PlayerId(0);
        let mut state = GameState {
            players: Vec::new(),
            turn_order: vec![hand],
            active_player: hand,
            turn_number: 1,
            phase: crate::types::turn::Phase::PreGame,
            step: crate::types::turn::Step::Untap,
            priority_player: hand,
            consecutive_passes: 0,
            lands_played_this_turn: HashMap::new(),
            combat: None,
            last_active_turn_number: HashMap::new(),
            objects: HashMap::new(),
            zones: vec![
                Zone {
                    kind: ZoneKind::Hand,
                    owner: Some(hand),
                    contents: VecDeque::new(),
                },
                Zone {
                    kind: ZoneKind::Battlefield,
                    owner: None,
                    contents: VecDeque::new(),
                },
            ],
            stack: Vec::new(),
            continuous_effects: Vec::new(),
            replacement_effects: Vec::new(),
            cost_modifiers: Vec::new(),
            hand_size_modifiers: Vec::new(),
            hand_reveal_modifiers: Vec::new(),
            draw_limit_modifiers: Vec::new(),
            land_play_modifiers: Vec::new(),
            abilities: Vec::new(),
            ledger: Vec::new(),
            rng: RngState {
                seed: 1,
                draws_consumed: 0,
            },
            next_event_id: 0,
            next_sequence_no: 0,
            next_object_id: 1,
            pending_step_skips: Vec::new(),
            once_per_turn_triggered: std::collections::HashSet::new(),
            commander_return_offered: std::collections::HashSet::new(),
            linked_ability_generation: std::collections::HashMap::new(),
            script_ability_ids: std::collections::HashSet::new(),
            ability_function_zones: std::collections::HashMap::new(),
            off_battlefield_scripts_run: std::collections::HashSet::new(),
            linked_exiles: Vec::new(),
            pending_timing_bypass: None,
            pending_free_cast: None,
        };

        let card = ObjectId(0);
        state
            .zone_mut(ZoneKind::Hand, Some(hand))
            .unwrap()
            .push_bottom(card);
        assert!(
            state
                .zone(ZoneKind::Hand, Some(hand))
                .unwrap()
                .contents
                .contains(&card)
        );

        let removed = state
            .zone_mut(ZoneKind::Hand, Some(hand))
            .unwrap()
            .remove(card);
        assert_eq!(removed, Some(card));
        assert!(
            !state
                .zone(ZoneKind::Hand, Some(hand))
                .unwrap()
                .contents
                .contains(&card)
        );

        state
            .zone_mut(ZoneKind::Battlefield, None)
            .unwrap()
            .push_bottom(card);
        assert!(
            state
                .zone(ZoneKind::Battlefield, None)
                .unwrap()
                .contents
                .contains(&card)
        );

        // Nonexistent zone (no command zone registered here) -> None, not a panic.
        assert!(state.zone_mut(ZoneKind::Command, Some(hand)).is_none());

        assert!(state.object(card).is_none());
        assert!(state.object_mut(card).is_none());
    }
}
