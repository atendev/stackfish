//! The `Counter` and `UnlessPaid` arms (CR 701.6a, 118.12). They act on a different
//! `StackObject` in `state.stack`, found by `stack_id`. See `DESIGN.md`.

use std::collections::VecDeque;
use std::ops::ControlFlow;

use crate::types::ability::UnlessPaidConsequence;
use crate::types::cost::CostSpec;
use crate::types::event::GameEventKind;
use crate::types::ids::{ObjectId, Target};
use crate::types::stack::{StackEntryKind, StackObject};
use crate::types::zone::ZoneKind;

use super::super::{Engine, ProcessState, StepStatus};

impl Engine {
    /// The `stack_id` named by the first legal target. A spell and an ability
    /// both resolve to one (CR 113.9); downstream code dispatches on the object's kind.
    fn counter_target_id(legal_targets: &[Target]) -> Option<ObjectId> {
        match legal_targets.first().copied() {
            Some(Target::Object(id)) | Some(Target::Ability(id)) => Some(id),
            Some(Target::Player(_)) | None => None,
        }
    }

    /// Removes `target_id` from `state.stack` and pushes the `Countered` and
    /// `ZoneChange` events. Shared by the unconditional counter (CR 701.6a) and the
    /// "unless paid" decline (CR 118.12). A countered ability just stops existing; a
    /// spell's card goes to the graveyard, or exile (CR 702.34a), through
    /// `move_resolving_spell` with no replacement check or ordering pause.
    ///
    /// A target already gone is a silent no-op. That is reachable from the decline
    /// path, since the target can resolve while the engine sits paused.
    // TODO: re-check legality on resume from `AwaitingUnlessPaidChoice`
    pub(in crate::engine) fn counter_stack_object(
        &mut self,
        target_id: ObjectId,
        events: &mut Vec<GameEventKind>,
    ) {
        let Some(pos) = self
            .state
            .stack
            .iter()
            .position(|so| so.stack_id == target_id)
        else {
            return;
        };
        let countered: StackObject = self.state.stack.remove(pos);
        if matches!(countered.kind, StackEntryKind::Spell) {
            let destination = if countered.exile_on_leaving_stack {
                ZoneKind::Exile
            } else {
                ZoneKind::Graveyard
            };
            if let Some(event) = self.move_resolving_spell(target_id, destination, false, false) {
                events.push(event);
            }
        }
        events.push(GameEventKind::Countered {
            stack_id: target_id,
        });
    }

    /// CR 701.6a: unconditionally counters the target.
    pub(super) fn resolve_counter_effect(
        &mut self,
        legal_targets: &[Target],
        mut events: Vec<GameEventKind>,
    ) -> ControlFlow<StepStatus, Vec<GameEventKind>> {
        if let Some(target_id) = Self::counter_target_id(legal_targets) {
            self.counter_stack_object(target_id, &mut events);
        }
        ControlFlow::Continue(events)
    }

    /// CR 118.12: pauses on `AwaitingUnlessPaidChoice` instead of resolving. For
    /// `Counter` the payer is the targeted object's controller; for `SacrificeSelf`
    /// it is this ability's controller. No legal target is a silent no-op.
    pub(super) fn resolve_unless_paid_effect(
        &mut self,
        top: &StackObject,
        legal_targets: &[Target],
        cost: &CostSpec,
        otherwise: &UnlessPaidConsequence,
        events: Vec<GameEventKind>,
    ) -> ControlFlow<StepStatus, Vec<GameEventKind>> {
        let (target_id, payer) = match otherwise {
            UnlessPaidConsequence::Counter => {
                let Some(target_id) = Self::counter_target_id(legal_targets) else {
                    return ControlFlow::Continue(events);
                };
                // Defensive: the target left the stack after `target_is_legal` rechecked it.
                let Some(payer) = self
                    .state
                    .stack
                    .iter()
                    .find(|so| so.stack_id == target_id)
                    .map(|so| so.controller)
                else {
                    return ControlFlow::Continue(events);
                };
                (Some(target_id), payer)
            }
            // CR 702.30a: echo's payer is always this ability's controller.
            UnlessPaidConsequence::SacrificeSelf => (None, top.controller),
        };
        self.process_state = ProcessState::AwaitingUnlessPaidChoice {
            payer_queue: VecDeque::from([payer]),
            cost: cost.clone(),
            otherwise: otherwise.clone(),
            target_id,
            source: top.source,
            stack_id: top.stack_id,
            events_so_far: events,
        };
        ControlFlow::Break(StepStatus::AwaitingUnlessPaidChoice {
            payer,
            cost: cost.clone(),
            otherwise: otherwise.clone(),
        })
    }
}
