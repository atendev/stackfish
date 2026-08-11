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
use crate::types::ids::{ObjectId, Target};
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
    /// count)`. Same candidate collection as `resolve_destroy_all_effect`
    /// above, then `count` of them (or fewer, if `count` exceeds however many
    /// matched — same "as many as possible" stance CR 701.17b's mill takes)
    /// are drawn without replacement via `next_rng`, each draw logged as a
    /// `RngDrawn` event exactly like `Engine::shuffle_library` already does.
    /// Candidates are collected into a plain `Vec` (not the `VecDeque` the
    /// final queue needs) so `next_rng`'s modulus can index into it directly
    /// and `Vec::remove` can pop the chosen one out without disturbing the
    /// rest.
    pub(super) fn resolve_sacrifice_random_effect(
        &mut self,
        top: &StackObject,
        filter: &EffectFilter,
        count: u32,
    ) -> ControlFlow<StepStatus, Vec<GameEventKind>> {
        let mut candidates: Vec<ObjectId> = self
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
        self.process_state = self.continue_scripted_zone_move(
            ScriptedZoneMoveKind::Sacrifice,
            chosen,
            ScriptedZoneMoveThen::StackResolution { stack_id: top.stack_id },
            Vec::new(),
        );
        ControlFlow::Break(StepStatus::Progressed)
    }
}
