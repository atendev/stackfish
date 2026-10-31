//! The `ReturnSelfWithCounter` arm (CR 702.79a, 702.93a). General graveyard-to-
//! battlefield effects go through `ScriptedZoneMoveKind::Reanimate` instead. See `DESIGN.md`.
// TODO: entry redirects (CR 616) and the enters-the-battlefield modifiers for this mover

use std::ops::ControlFlow;

use crate::types::counter::CounterKind;
use crate::types::event::GameEventKind;
use crate::types::ids::ObjectLineage;
use crate::types::stack::StackObject;
use crate::types::zone::ZoneKind;

use super::super::{Engine, StepStatus};

impl Engine {
    pub(super) fn resolve_return_self_with_counter_effect(
        &mut self,
        top: &StackObject,
        kind: CounterKind,
        events: Vec<GameEventKind>,
    ) -> ControlFlow<StepStatus, Vec<GameEventKind>> {
        // `top.source` is the pre-death id; `ObjectLineage::previous` links to the graveyard object.
        let Some((graveyard_id, graveyard_object)) = self
            .state
            .objects
            .iter()
            .find(|(_, o)| o.zone == ZoneKind::Graveyard && o.lineage.previous == Some(top.source))
            .map(|(id, o)| (*id, o.clone()))
        else {
            // CR 702.79a: the card left the graveyard before this resolved, so nothing happens.
            return ControlFlow::Continue(events);
        };

        let owner = graveyard_object.owner;
        let new_id = self.state.allocate_object_id();
        let mut new_object = graveyard_object;
        new_object.object_id = new_id;
        new_object.lineage = ObjectLineage {
            previous: Some(graveyard_id),
        };
        new_object.zone = ZoneKind::Battlefield;
        // CR 400.7: no memory of the old object; CR 800.4a: returns under its owner's control.
        new_object.reset_for_new_object();
        new_object.timestamp = self.state.allocate_sequence_no();
        // CR 302.6
        new_object.controlled_since_turn = self.state.turn_number;

        if let Some(zone) = self.state.zone_mut(ZoneKind::Graveyard, Some(owner)) {
            zone.remove(graveyard_id);
        }
        if let Some(zone) = self.state.zone_mut(ZoneKind::Battlefield, None) {
            zone.push_bottom(new_id);
            self.state.objects.insert(new_id, new_object);
        }

        // Logged straight to the ledger without a replacement check; enters triggers still fire from it.
        let event_id = self.state.allocate_event_id();
        let sequence_no = self.state.allocate_sequence_no();
        self.state.ledger.push(crate::types::event::GameEvent {
            event_id,
            sequence_no,
            kind: GameEventKind::ZoneChange {
                object: new_id,
                from: ZoneKind::Graveyard,
                to: ZoneKind::Battlefield,
                last_known_info: None,
            },
            caused_by_command: None,
            parent_event: None,
        });

        // Self-registers this incarnation's abilities, as every battlefield-entry producer does.
        self.run_card_script_for(new_id);

        // CR 702.79a: the counter is an event through the pipeline, not a direct mutation, to avoid applying it twice.
        let mut events = events;
        events.push(GameEventKind::CounterAdded {
            object: new_id,
            kind,
            amount: 1,
        });
        ControlFlow::Continue(events)
    }
}
