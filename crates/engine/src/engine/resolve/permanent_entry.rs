//! The "resolving `Spell` leaves the stack" block of CR 608.2m: permanents enter the battlefield,
//! instants and sorceries go to the graveyard or exile. `top` is owned so the CR 616.1 redirect
//! pause can carry the whole unplaced object. See `DESIGN.md`.
use std::ops::ControlFlow;

use crate::commands;
use crate::replacement::{self, ReplacementOutcome};
use crate::types::card::CardTypeSet;
use crate::types::cost::colors_spent;
use crate::types::event::GameEventKind;
use crate::types::ids::Target;
use crate::types::stack::StackObject;
use crate::types::zone::ZoneKind;

use super::super::{Engine, ProcessState, ReplacementResumeTarget, StepStatus};

impl Engine {
    pub(super) fn resolve_permanent_entry(
        &mut self,
        top: StackObject,
        legal_targets: &[Target],
        events: Vec<GameEventKind>,
    ) -> ControlFlow<StepStatus, (StackObject, Vec<GameEventKind>)> {
        // The destination must be decided before `move_resolving_spell`.
        let destination = self.state.object(top.stack_id).map(|obj| {
            if obj.definition.card_types.intersects(CardTypeSet::INSTANT | CardTypeSet::SORCERY) {
                // CR 702.27a: buyback is checked before exile-on-leaving; it is the more specific choice.
                if top.bought_back {
                    ZoneKind::Hand
                } else if top.exile_on_leaving_stack {
                    ZoneKind::Exile
                } else {
                    ZoneKind::Graveyard
                }
            } else {
                ZoneKind::Battlefield
            }
        });
        // Defensive: the spell object is always still at `top.stack_id`.
        let Some(destination) = destination else {
            return ControlFlow::Continue((top, events));
        };
        // CR 614: a registered `ReplaceZoneDestination` can redirect a battlefield
        // entry, proposed and checked before `move_resolving_spell` places anything.
        // The instant or sorcery redirect above is CR 400.4a and unaffected.
        // TODO: the entering permanent's own redirect effects aren't registered yet, since its script runs inside `move_resolving_spell`
        let destination = if destination == ZoneKind::Battlefield {
            let proposed = GameEventKind::ZoneChange {
                object: top.stack_id,
                from: ZoneKind::Stack,
                to: ZoneKind::Battlefield,
                last_known_info: None,
            };
            match replacement::apply_replacements(&mut self.state, &self.script_runtime, proposed, false) {
                ReplacementOutcome::Resolved(GameEventKind::ZoneChange { to, .. }) => to,
                // Defensive: a would-move candidate only rewrites back into a zone change.
                ReplacementOutcome::Resolved(_) => destination,
                // CR 616.1: two or more redirects get an ordering choice. Nothing is placed
                // yet, so the owned `top` and `legal_targets` ride along in
                // `ReplacementResumeTarget::PermanentEntry`.
                ReplacementOutcome::NeedsOrderingChoice { affected_player, event, candidates, excluded, .. } => {
                    self.process_state = ProcessState::AwaitingReplacementChoice {
                        affected_player,
                        pending_event: event,
                        candidates,
                        excluded,
                        resume: ReplacementResumeTarget::PermanentEntry {
                            top,
                            legal_targets: legal_targets.to_vec(),
                            events,
                        },
                    };
                    return ControlFlow::Break(StepStatus::Progressed);
                }
            }
        } else {
            destination
        };
        match self.continue_permanent_entry(top, legal_targets, events, destination) {
            ControlFlow::Break(process_state) => {
                self.process_state = process_state;
                ControlFlow::Break(StepStatus::Progressed)
            }
            ControlFlow::Continue(result) => ControlFlow::Continue(result),
        }
    }

    /// The rest of permanent entry once `destination` is settled, synchronously or
    /// after a redirect ordering choice. A `Break` carries the `ProcessState` to
    /// install for a further copy, devour or amplify pause.
    pub(in crate::engine) fn continue_permanent_entry(
        &mut self,
        top: StackObject,
        legal_targets: &[Target],
        mut events: Vec<GameEventKind>,
        destination: ZoneKind,
    ) -> ControlFlow<ProcessState, (StackObject, Vec<GameEventKind>)> {
        let run_script = destination == ZoneKind::Battlefield;
        if let Some(event) = self.move_resolving_spell(top.stack_id, destination, run_script, top.escaped) {
            // CR 303.4b: an Aura attaches to its legality-checked sole target.
            // TODO: other attached-entry shapes (CR 303.4f, 303.4h) have no producer here
            let new_battlefield_aura = match &event {
                GameEventKind::ZoneChange { object, to: ZoneKind::Battlefield, .. }
                    if self.state.object(*object).is_some_and(|o| commands::is_aura(&o.definition)) =>
                {
                    Some(*object)
                }
                _ => None,
            };
            // CR 603.6d: the object's script already ran in `move_resolving_spell`, so its
            // entry effects are registered; apply them and any external ones.
            if let GameEventKind::ZoneChange { object, to: ZoneKind::Battlefield, .. } = &event {
                let object_id = *object;
                // CR 707.9: checked before devour and amplify as the most identity-
                // fundamental choice. Only a resolving spell raises this pause.
                if let Some((exceptions, candidates)) = replacement::pending_copy_choice(
                    &self.state,
                    &self.script_runtime,
                    object_id,
                    ZoneKind::Stack,
                ) {
                    events.push(event);
                    return ControlFlow::Break(ProcessState::AwaitingCopyChoice {
                        object: object_id,
                        controller: top.controller,
                        exceptions,
                        candidates,
                        stack_id: top.stack_id,
                        events_so_far: events,
                        colors_spent: colors_spent(&top.costs_paid),
                        // CR 707.9e: a first pause has no outer round to fall back to.
                        carry_forward_if_declined: None,
                        // CR 707.9e: `object`'s paid X, captured once.
                        x_value: top.x_value,
                    });
                }
                // CR 702.82a: checked before the modifiers; only a resolving spell pauses here.
                if let Some((per_creature, candidates)) = replacement::pending_devour_choice(
                    &self.state,
                    &self.script_runtime,
                    object_id,
                    ZoneKind::Stack,
                    top.controller,
                ) {
                    events.push(event);
                    return ControlFlow::Break(ProcessState::AwaitingDevourChoice {
                        object: object_id,
                        controller: top.controller,
                        per_creature,
                        candidates,
                        stack_id: top.stack_id,
                        events_so_far: events,
                    });
                }
                // CR 702.38a: as devour.
                if let Some((per_card, candidates)) = replacement::pending_amplify_choice(
                    &self.state,
                    &self.script_runtime,
                    object_id,
                    ZoneKind::Stack,
                    top.controller,
                ) {
                    events.push(event);
                    return ControlFlow::Break(ProcessState::AwaitingAmplifyChoice {
                        object: object_id,
                        controller: top.controller,
                        per_card,
                        candidates,
                        stack_id: top.stack_id,
                        events_so_far: events,
                    });
                }
                // CR 702.44b: only a resolving spell has `costs_paid` to read colors from.
                let counter_events = replacement::apply_enters_battlefield_modifiers(
                    &mut self.state,
                    &self.script_runtime,
                    object_id,
                    ZoneKind::Stack,
                    colors_spent(&top.costs_paid),
                );
                events.push(event);
                events.extend(counter_events);
            } else {
                events.push(event);
            }
            if let Some(aura_id) = new_battlefield_aura {
                match legal_targets.first().copied() {
                    Some(Target::Object(target_id)) => {
                        commands::attach_permanent(&mut self.state, aura_id, target_id);
                        events.push(GameEventKind::Attached { object: aura_id, attached_to: target_id });
                    }
                    // CR 303.4b: an Aura can enchant a player.
                    Some(Target::Player(player)) => {
                        commands::attach_permanent_to_player(&mut self.state, aura_id, player);
                        events.push(GameEventKind::AttachedToPlayer { object: aura_id, player });
                    }
                    // CR 303.4: an Aura's target is an object or a player.
                    Some(Target::Ability(_)) | None => {}
                }
            }
        }
        ControlFlow::Continue((top, events))
    }
}
