//! `resolve_ability_effect`'s always-pausing zone-move arms — `Sacrifice`,
//! `Destroy`, `DestroyAll`, `Exile`. Every one of these hands its target
//! queue to `Engine::continue_scripted_zone_move` (already `pub(in
//! crate::engine)` in `replacement_resume.rs`) and always returns
//! `ControlFlow::Break` — `continue_scripted_zone_move` itself sets
//! `self.process_state` (either an `AwaitingReplacementChoice` CR 616.1
//! ordering pause or straight through to `ReplacementCheck`), so there's
//! nothing left in the shared `events` accumulator for these arms to
//! contribute — same as the original inlined code, which called these with
//! a literal `Vec::new()` trailing rather than threading `events` through.

use std::collections::VecDeque;
use std::ops::ControlFlow;

use crate::types::effect::{matches_effect_filter, EffectFilter};
use crate::types::event::{GameEvent, GameEventKind};
use crate::types::ids::{ObjectId, PlayerId, Target};
use crate::types::stack::StackObject;
use crate::types::zone::ZoneKind;

use super::super::{Engine, ScriptedZoneMoveKind, ScriptedZoneMoveThen, StepStatus};

impl Engine {
    pub(super) fn resolve_sacrifice_effect(
        &mut self,
        top: &StackObject,
        legal_targets: &[Target],
    ) -> ControlFlow<StepStatus, Vec<GameEventKind>> {
        let targets: VecDeque<ObjectId> = legal_targets
            .iter()
            .filter_map(|t| match t {
                Target::Object(id) => Some(*id),
                _ => None,
            })
            .collect();
        self.process_state =
            self.continue_scripted_zone_move(ScriptedZoneMoveKind::Sacrifice, targets, ScriptedZoneMoveThen::StackResolution { stack_id: top.stack_id }, Vec::new());
        ControlFlow::Break(StepStatus::Progressed)
    }

    pub(super) fn resolve_destroy_effect(
        &mut self,
        top: &StackObject,
        legal_targets: &[Target],
    ) -> ControlFlow<StepStatus, Vec<GameEventKind>> {
        let targets: VecDeque<ObjectId> = legal_targets
            .iter()
            .filter_map(|t| match t {
                Target::Object(id) => Some(*id),
                _ => None,
            })
            .collect();
        self.process_state =
            self.continue_scripted_zone_move(ScriptedZoneMoveKind::Destroy, targets, ScriptedZoneMoveThen::StackResolution { stack_id: top.stack_id }, Vec::new());
        ControlFlow::Break(StepStatus::Progressed)
    }

    pub(super) fn resolve_destroy_all_effect(
        &mut self,
        top: &StackObject,
        filter: &EffectFilter,
    ) -> ControlFlow<StepStatus, Vec<GameEventKind>> {
        let targets: VecDeque<ObjectId> = self
            .state
            .zone(ZoneKind::Battlefield, None)
            .map(|zone| {
                zone.contents
                    .iter()
                    .copied()
                    .filter(|id| {
                        self.state
                            .object(*id)
                            .is_some_and(|object| matches_effect_filter(filter, object, top.source, top.controller))
                    })
                    .collect()
            })
            .unwrap_or_default();
        self.process_state =
            self.continue_scripted_zone_move(ScriptedZoneMoveKind::Destroy, targets, ScriptedZoneMoveThen::StackResolution { stack_id: top.stack_id }, Vec::new());
        ControlFlow::Break(StepStatus::Progressed)
    }

    pub(super) fn resolve_exile_effect(
        &mut self,
        top: &StackObject,
        legal_targets: &[Target],
    ) -> ControlFlow<StepStatus, Vec<GameEventKind>> {
        let targets: VecDeque<ObjectId> = legal_targets
            .iter()
            .filter_map(|t| match t {
                Target::Object(id) => Some(*id),
                _ => None,
            })
            .collect();
        self.process_state =
            self.continue_scripted_zone_move(ScriptedZoneMoveKind::Exile, targets, ScriptedZoneMoveThen::StackResolution { stack_id: top.stack_id }, Vec::new());
        ControlFlow::Break(StepStatus::Progressed)
    }

    /// CR 701.21 (Sacrifice) + "at random" — `Game.Ops.SacrificeRandom(filter,
    /// count)`. Draws via `pick_sacrifice_random_ids` below, then hands the
    /// result straight to `continue_scripted_zone_move` for the single-player
    /// case (a bare `Game.Ops.ForEachPlayer(Game.Ops.SacrificeRandom(...))`,
    /// `phase5/mass-choice-effects`, calls the picking half directly instead
    /// — see `each_player::resolve_for_each_player_effect`'s own doc comment
    /// for why merging every player's picks before the one real zone move is
    /// both simpler and already CR 101.4-"simultaneous" for free).
    pub(super) fn resolve_sacrifice_random_effect(
        &mut self,
        top: &StackObject,
        filter: &EffectFilter,
        count: u32,
    ) -> ControlFlow<StepStatus, Vec<GameEventKind>> {
        let chosen = self.pick_sacrifice_random_ids(top.source, top.controller, filter, count);
        self.process_state = self.continue_scripted_zone_move(
            ScriptedZoneMoveKind::Sacrifice,
            chosen,
            ScriptedZoneMoveThen::StackResolution { stack_id: top.stack_id },
            Vec::new(),
        );
        ControlFlow::Break(StepStatus::Progressed)
    }

    /// The picking half of `resolve_sacrifice_random_effect` above, split out
    /// (`phase5/mass-choice-effects`) so a bare-tag `Game.Ops.ForEachPlayer`
    /// can draw one player's own random picks without also performing the
    /// real zone move on the spot (that only happens once, after every
    /// player's own picks are merged — see `AbilityEffect::ForEachPlayer`'s
    /// own doc comment). Candidates matching `filter` (relative to `source`/
    /// `controller`, same as `resolve_destroy_all_effect`'s own convention)
    /// are collected into a plain `Vec` (not the `VecDeque` the caller
    /// ultimately needs) so `next_rng`'s modulus can index into it directly
    /// and `Vec::remove` can pop the chosen one out without disturbing the
    /// rest; `count` of them (or fewer, if `count` exceeds however many
    /// matched — same "as many as possible" stance CR 701.17b's mill takes)
    /// are drawn without replacement, each draw logged as a `RngDrawn` event
    /// exactly like `Engine::shuffle_library` already does. Pure synchronous
    /// Rust, no Lua involved — can't pause mid-computation the way a
    /// `Game.ChooseN`-driven pick can, so a caller merging several players'
    /// worth of these never needs to consider an intermediate pause.
    pub(in crate::engine) fn pick_sacrifice_random_ids(
        &mut self,
        source: ObjectId,
        controller: PlayerId,
        filter: &EffectFilter,
        count: u32,
    ) -> VecDeque<ObjectId> {
        let mut candidates: Vec<ObjectId> = self
            .state
            .zone(ZoneKind::Battlefield, None)
            .map(|zone| {
                zone.contents
                    .iter()
                    .copied()
                    .filter(|id| {
                        self.state.object(*id).is_some_and(|object| matches_effect_filter(filter, object, source, controller))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let mut chosen: VecDeque<ObjectId> = VecDeque::new();
        for _ in 0..count {
            if candidates.is_empty() {
                break;
            }
            let rng_val = self.next_rng();
            let idx = (rng_val % candidates.len() as u64) as usize;
            chosen.push_back(candidates.remove(idx));
            let event_id = self.state.allocate_event_id();
            let seq = self.state.allocate_sequence_no();
            self.state.ledger.push(GameEvent {
                event_id,
                sequence_no: seq,
                kind: GameEventKind::RngDrawn { purpose: "sacrifice_random".to_string(), value: rng_val },
                caused_by_command: None,
                parent_event: None,
            });
        }
        chosen
    }
}
