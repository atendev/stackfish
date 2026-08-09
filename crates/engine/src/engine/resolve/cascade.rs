//! `resolve_ability_effect`'s `Cascade` arm — CR 702.85a's own "exile cards
//! from the top of your library until you exile a nonland card whose mana
//! value is less than this spell's mana value" walk. See `AbilityEffect::
//! Cascade`'s own doc comment in `types/ability.rs` for the rules citation.
//!
//! Unlike Madness's own resolver, the library walk itself is entirely
//! deterministic (no player choice about *which* cards get exiled) so it
//! runs synchronously in one shot here — no per-card pause needed, unlike
//! `Search`'s own candidate-queue shape (which pauses because the player
//! chooses which matching cards to take). Only the *end* of the walk
//! ("cast the candidate, or not") is a real decision, raised as `ProcessState
//! ::AwaitingCascadeChoice` and answered by `Engine::decline_cascade_cast`/
//! `submit_cascade_cast` (`engine::choice_answers`).
//!
//! Narrowed, named rather than hidden: CR 702.85a's own second clause ("you
//! may cast that card ... if the resulting spell's mana value is less than
//! this spell's mana value") is a re-check against the *candidate's own*
//! mana value at cast time, in case some effect changed it between being
//! exiled and being cast — this slice doesn't re-derive it, using the same
//! value the walk itself already computed. No real card in this engine's
//! test corpus has a cost that can change between those two points. CR
//! 702.85b ("as you cascade" actions on the exiled cards) and CR 702.85c
//! (multiple Cascade instances trigger separately - this falls out for free
//! from ordinary trigger collection, not handled specially here) are also
//! out of this slice's own scope - 702.85b because no real card needs it,
//! 702.85c because it needs no code at all.

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
        // CR 202.3e: the cascade spell's own mana value, X already resolved
        // (top.x_value) since this spell is very much on the stack.
        let cascade_mv = self
            .state
            .object(top.source)
            .map(|o| o.definition.mana_cost.resolve_x(top.x_value.unwrap_or(0)).mana_value())
            .unwrap_or(0);

        let mut exiled = Vec::new();
        let mut candidate = None;
        loop {
            let Some(card) =
                self.state.zone(ZoneKind::Library, Some(controller)).and_then(|z| z.contents.front().copied())
            else {
                // CR 702.85a: the library ran out before a qualifying card
                // was found - the walk simply stops.
                break;
            };
            let Some(proposed) = commands::propose_zone_move(&self.state, card, ZoneKind::Library, ZoneKind::Exile)
            else {
                break;
            };
            // No replacement check - library moves aren't replaceable
            // anywhere in this engine today, same posture Search's own
            // found-card mover already takes.
            let (committed, departed) = commands::commit_zone_move(&mut self.state, proposed);
            self.departed_triggered_abilities.extend(departed);
            let GameEventKind::ZoneChange { object: new_id, .. } = committed.kind else {
                unreachable!("propose_zone_move only ever proposes a ZoneChange");
            };
            self.state.ledger.push(committed);
            exiled.push(new_id);

            let card_object = self.state.object(new_id).expect("just inserted by commit_zone_move");
            let is_land = card_object.definition.card_types.contains(CardTypeSet::LAND);
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
        ControlFlow::Break(StepStatus::AwaitingCascadeChoice { player: controller, candidate })
    }
}
