//! `resolve_ability_effect`'s `ReturnSelfWithCounter` arm — CR 702.79a
//! Persist / CR 702.93a Undying's own "return it to the battlefield under
//! its owner's control with a counter on it." See `AbilityEffect::
//! ReturnSelfWithCounter`'s own doc comment in `types/ability.rs` for the
//! rules citation and the deliberate narrowing (no CR 616 entry-redirect
//! support, no `apply_enters_battlefield_modifiers` family interaction —
//! no real Persist/Undying card needs either).

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
        // `top.source` is the departed, pre-death ObjectId (this ability
        // only ever comes from a Dies trigger) - `sba::commit_death` mints
        // the graveyard object's own fresh id and records this exact link
        // via `ObjectLineage::previous`, so that's the real, current
        // identity of "this permanent" now that it's sitting in the
        // graveyard.
        let Some((graveyard_id, graveyard_object)) = self
            .state
            .objects
            .iter()
            .find(|(_, o)| o.zone == ZoneKind::Graveyard && o.lineage.previous == Some(top.source))
            .map(|(id, o)| (*id, o.clone()))
        else {
            // CR 702.79a/93a: nothing to return - the card left the
            // graveyard before this ability resolved (exiled in response,
            // etc). Matches real Magic: the ability simply does nothing.
            return ControlFlow::Continue(events);
        };

        let owner = graveyard_object.owner;
        let new_id = self.state.allocate_object_id();
        let mut new_object = graveyard_object;
        new_object.object_id = new_id;
        new_object.lineage = ObjectLineage { previous: Some(graveyard_id) };
        new_object.zone = ZoneKind::Battlefield;
        // CR 400.7: a new object has no memory of its previous incarnation's
        // tapped/damage/counters/attachments; CR 800.4a: it returns under its
        // *owner's* control, not whoever controlled it when it died.
        new_object.reset_for_new_object();
        new_object.timestamp = self.state.allocate_sequence_no();
        // CR 302.6/602.5a: a freshly entering permanent is summoning sick.
        new_object.controlled_since_turn = self.state.turn_number;

        if let Some(zone) = self.state.zone_mut(ZoneKind::Graveyard, Some(owner)) {
            zone.remove(graveyard_id);
        }
        if let Some(zone) = self.state.zone_mut(ZoneKind::Battlefield, None) {
            zone.push_bottom(new_id);
            self.state.objects.insert(new_id, new_object);
        }
        // else: defensive - Battlefield is always a registered zone in a
        // real GameState, same stance every other zone_mut call in this
        // engine takes for a lookup that "shouldn't" fail.

        // Logged straight to the ledger, not routed through
        // `ReplacementCheck` - deliberately narrow (see this module's own
        // doc comment): no CR 616 "would enter, redirect elsewhere"
        // producer is wired up for this specific mover. Ordinary "when
        // this creature enters" triggers (this card's own or anyone
        // else's) still fire off this event for free once `CollectTriggers`
        // scans the ledger tail, same as any other real ZoneChange.
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

        // Self-registers this incarnation's own abilities/keywords fresh -
        // same convention every other real battlefield-entry producer
        // (PlayLand/CreateToken/Search/ResolveStackTop) already follows.
        self.run_card_script_for(new_id);

        // CR 702.79a/93a's own counter - a plain CounterAdded event through
        // the normal ReplacementCheck->CommitEvents pipeline (which already
        // applies it generically), not a direct `counters.add()` mutation
        // here - avoids the exact double-apply mistake this project's own
        // "Mistakes caught" notes warn about for a payload that both
        // mutates state directly and separately proposes an event
        // describing the same mutation.
        let mut events = events;
        events.push(GameEventKind::CounterAdded { object: new_id, kind, amount: 1 });
        ControlFlow::Continue(events)
    }
}
