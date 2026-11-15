//! CR 603.7: making a delayed triggered ability from a `Chain` entry, and what
//! it still affects when it resolves. See `DESIGN.md`.

use crate::types::ability::{AbilityEffect, DelayedRefers};
use crate::types::event::GameEventKind;
use crate::types::game::{DelayedTrigger, DelayedWhen};
use crate::types::ids::{EventId, ObjectId, Target};
use crate::types::object::GameObject;
use crate::types::stack::StackObject;
use crate::types::zone::ZoneKind;

use super::super::Engine;

impl Engine {
    /// CR 603.7c, 400.7: each referred object still in its recorded zone. A
    /// `Sacrifice` also skips what its controller does not control (CR 701.21a).
    pub(super) fn delayed_targets_still_there(&self, top: &StackObject) -> Vec<Target> {
        let sacrifice = matches!(top.effect, Some(AbilityEffect::Sacrifice));
        top.delayed_refers
            .iter()
            .filter(|&&(id, zone)| {
                self.state.object(id).is_some_and(|o| o.zone == zone)
                    && (!sacrifice
                        || crate::layers::effective_controller(
                            &self.state,
                            &self.script_runtime,
                            id,
                        ) == top.controller)
            })
            .map(|&(id, _)| Target::Object(id))
            .collect()
    }

    /// CR 603.7a, 603.7d, 603.7e: adds the row for an `AtNextEndStep` entry.
    /// `since_ledger` is where the resolution began, so `Entered` and a moved
    /// target read only what this resolution did.
    pub(super) fn create_delayed_trigger(
        &mut self,
        top: &StackObject,
        legal: &[Target],
        effect: &AbilityEffect,
        refers: DelayedRefers,
        since_ledger: usize,
    ) {
        let since = since_ledger.min(self.state.ledger.len());
        let ids: Vec<ObjectId> = match refers {
            DelayedRefers::Targets => legal
                .iter()
                .filter_map(|target| match target {
                    Target::Object(id) => Some(self.moved_to(*id, since)),
                    _ => None,
                })
                .collect(),
            DelayedRefers::SelfObject => vec![top.source],
            DelayedRefers::Entered => self.state.ledger[since..]
                .iter()
                .filter_map(|event| match &event.kind {
                    GameEventKind::ZoneChange {
                        object,
                        to: ZoneKind::Battlefield,
                        ..
                    } => Some(*object),
                    _ => None,
                })
                .collect(),
        };
        let mut recorded: Vec<(ObjectId, ZoneKind)> = Vec::new();
        for id in ids {
            if let Some(object) = self.state.object(id)
                && !recorded.iter().any(|&(seen, _)| seen == id)
            {
                recorded.push((id, object.zone));
            }
        }
        // A spell whose earlier entry paused has already gone to its graveyard,
        // so its snapshot comes from that move.
        let source_lki = self
            .state
            .object(top.source)
            .cloned()
            .or_else(|| top.source_lki.clone())
            .or_else(|| self.left_since(top.source, since));
        let id = self.state.allocate_ability_id();
        let created_after = EventId(self.state.next_event_id);
        self.state.delayed_triggers.push(DelayedTrigger {
            id,
            source: top.source,
            source_lki,
            controller: top.controller,
            when: DelayedWhen::NextEndStep,
            effect: effect.clone(),
            refers: recorded,
            created_after,
            triggered: false,
        });
    }

    /// The last known information of `id` if it changed zones since `since`.
    fn left_since(&self, id: ObjectId, since: usize) -> Option<GameObject> {
        self.state.ledger[since..]
            .iter()
            .find_map(|event| match &event.kind {
                GameEventKind::ZoneChange {
                    last_known_info: Some(lki),
                    ..
                } if lki.object_id == id => Some(lki.clone()),
                _ => None,
            })
    }

    /// CR 400.7j: the object `id` became when this resolution moved it to a
    /// public zone, or `id` itself.
    fn moved_to(&self, id: ObjectId, since: usize) -> ObjectId {
        let mut current = id;
        for event in &self.state.ledger[since..] {
            if let GameEventKind::ZoneChange {
                object,
                to,
                last_known_info: Some(lki),
                ..
            } = &event.kind
                && lki.object_id == current
                && !matches!(to, ZoneKind::Library | ZoneKind::Hand)
            {
                current = *object;
            }
        }
        current
    }
}

/// The stack text of a delayed ability, which has no card text of its own.
pub(in crate::engine) fn delayed_ability_text(effect: &AbilityEffect) -> String {
    let action = match effect {
        AbilityEffect::Exile => "exile",
        AbilityEffect::Sacrifice => "sacrifice",
        AbilityEffect::Destroy => "destroy",
        AbilityEffect::ReturnToHand => "return to hand",
        AbilityEffect::ReturnToBattlefield { .. } => "return to the battlefield",
        _ => "act",
    };
    format!("At the beginning of the next end step, {action}.")
}
