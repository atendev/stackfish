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
use crate::types::event::GameEventKind;
use crate::types::ids::{ObjectId, Target};
use crate::types::stack::StackObject;
use crate::types::zone::ZoneKind;

use super::super::{Engine, ScriptedZoneMoveKind, StepStatus};

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
            self.continue_scripted_zone_move(ScriptedZoneMoveKind::Sacrifice, targets, top.stack_id, Vec::new());
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
            self.continue_scripted_zone_move(ScriptedZoneMoveKind::Destroy, targets, top.stack_id, Vec::new());
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
            self.continue_scripted_zone_move(ScriptedZoneMoveKind::Destroy, targets, top.stack_id, Vec::new());
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
            self.continue_scripted_zone_move(ScriptedZoneMoveKind::Exile, targets, top.stack_id, Vec::new());
        ControlFlow::Break(StepStatus::Progressed)
    }
}
