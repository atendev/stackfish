//! `resolve_ability_effect`'s `MadnessCastOrDiscard` arm — CR 702.35a
//! Madness's own "you may cast it by paying [cost] rather than paying its
//! mana cost. If you don't, put it into your graveyard." See
//! `AbilityEffect::MadnessCastOrDiscard`'s own doc comment in
//! `types/ability.rs` for the rules citation. Unlike every sibling in this
//! subdirectory, resolving this always pauses (`ProcessState::
//! AwaitingMadnessChoice`) rather than finishing synchronously or draining a
//! zone-move queue — the "yes" branch is a full nested cast (its own
//! targets/modes/payment), which only `Engine::submit_madness_cast`
//! (`engine::choice_answers`) can drive.

use std::ops::ControlFlow;

use crate::types::stack::StackObject;
use crate::types::event::GameEventKind;
use crate::types::zone::ZoneKind;

use super::super::{Engine, ProcessState, StepStatus};

impl Engine {
    pub(super) fn resolve_madness_effect(
        &mut self,
        top: &StackObject,
        events: Vec<GameEventKind>,
    ) -> ControlFlow<StepStatus, Vec<GameEventKind>> {
        // `top.source` is the departed, pre-exile ObjectId (this ability
        // only ever comes from a MadnessExiled trigger) - `commands::
        // commit_zone_move` mints the exile object's own fresh id and
        // records this exact link via `ObjectLineage::previous`, same
        // "look up the live descendant" pattern `ReturnSelfWithCounter`'s
        // own Persist/Undying resolver already established for Graveyard.
        let Some((exile_id, controller)) = self
            .state
            .objects
            .iter()
            .find(|(_, o)| o.zone == ZoneKind::Exile && o.lineage.previous == Some(top.source))
            .map(|(id, o)| (*id, o.controller))
        else {
            // CR 702.35a: nothing to decide - the card left exile before
            // this ability resolved (cast or moved away in response, etc).
            // Matches real Magic: the ability simply does nothing.
            return ControlFlow::Continue(events);
        };

        self.process_state = ProcessState::AwaitingMadnessChoice {
            player: controller,
            card: exile_id,
            stack_id: top.stack_id,
            events_so_far: events,
        };
        ControlFlow::Break(StepStatus::AwaitingMadnessChoice { player: controller, card: exile_id })
    }
}
