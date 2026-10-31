//! The `MadnessCastOrDiscard` arm (CR 702.35a). Always pauses, since the "yes" branch is
//! a nested cast driven by `submit_madness_cast`. See `DESIGN.md`.

use std::ops::ControlFlow;

use crate::types::event::GameEventKind;
use crate::types::stack::StackObject;
use crate::types::zone::ZoneKind;

use super::super::{Engine, ProcessState, StepStatus};

impl Engine {
    pub(super) fn resolve_madness_effect(
        &mut self,
        top: &StackObject,
        events: Vec<GameEventKind>,
    ) -> ControlFlow<StepStatus, Vec<GameEventKind>> {
        // `top.source` is the pre-exile id; `ObjectLineage::previous` links to the live exile object.
        let Some((exile_id, controller)) = self
            .state
            .objects
            .iter()
            .find(|(_, o)| o.zone == ZoneKind::Exile && o.lineage.previous == Some(top.source))
            .map(|(id, o)| (*id, o.controller))
        else {
            // CR 702.35a: the card left exile before this resolved, so nothing happens.
            return ControlFlow::Continue(events);
        };

        self.process_state = ProcessState::AwaitingMadnessChoice {
            player: controller,
            card: exile_id,
            stack_id: top.stack_id,
            events_so_far: events,
        };
        ControlFlow::Break(StepStatus::AwaitingMadnessChoice {
            player: controller,
            card: exile_id,
        })
    }
}
