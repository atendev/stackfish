//! The `Cascade` arm (CR 702.85a): a synchronous library walk, then a cast-or-decline
//! pause answered in `choice_answers`. See `DESIGN.md`.
// TODO: recheck the candidate's mana value at cast time (CR 702.85a)
// TODO: "as you cascade" actions on the exiled cards (CR 702.85b)

use std::ops::ControlFlow;

use crate::commands;
use crate::types::card::CardTypeSet;
use crate::types::event::GameEventKind;
use crate::types::stack::StackObject;
use crate::types::zone::ZoneKind;

use super::super::{Engine, ProcessState, StepStatus};

impl Engine {
    pub(super) fn resolve_cascade_effect(
        &mut self,
        top: &StackObject,
        events: Vec<GameEventKind>,
    ) -> ControlFlow<StepStatus, Vec<GameEventKind>> {
        let controller = top.controller;
        // CR 202.3e: X is already resolved while the spell is on the stack.
        let cascade_mv = self
            .state
            .object(top.source)
            .map(|o| {
                o.definition
                    .mana_cost
                    .resolve_x(top.x_value.unwrap_or(0))
                    .mana_value()
            })
            .unwrap_or(0);

        let mut exiled = Vec::new();
        let mut candidate = None;
        // CR 702.85a: the walk stops when the library runs out.
        while let Some(card) = self
            .state
            .zone(ZoneKind::Library, Some(controller))
            .and_then(|z| z.contents.front().copied())
        {
            let Some(proposed) =
                commands::propose_zone_move(&self.state, card, ZoneKind::Library, ZoneKind::Exile)
            else {
                break;
            };
            // Library moves aren't replaceable here, as for `Search`.
            let (committed, departed) = commands::commit_zone_move(&mut self.state, proposed);
            self.departed_triggered_abilities.extend(departed);
            let GameEventKind::ZoneChange { object: new_id, .. } = committed.kind else {
                unreachable!("propose_zone_move only ever proposes a ZoneChange");
            };
            self.state.ledger.push(committed);
            exiled.push(new_id);

            let card_object = self
                .state
                .object(new_id)
                .expect("just inserted by commit_zone_move");
            let is_land = card_object
                .definition
                .card_types
                .contains(CardTypeSet::LAND);
            let mana_value = card_object.definition.mana_cost.mana_value();
            if !is_land && mana_value < cascade_mv {
                candidate = Some(new_id);
                break;
            }
        }

        self.process_state = ProcessState::AwaitingCascadeChoice {
            player: controller,
            candidate,
            exiled,
            stack_id: top.stack_id,
            events_so_far: events,
        };
        ControlFlow::Break(StepStatus::AwaitingCascadeChoice {
            player: controller,
            candidate,
        })
    }
}
