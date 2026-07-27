//! `resolve_permanent_entry` — the "a resolving `Spell` leaves the stack"
//! block of CR 608.2m resolution, split out of the original fully-inlined
//! `ProcessState::ResolveStackTop` arm. Covers both directions a `Spell` can
//! leave the stack: a permanent type goes to the battlefield (with the CR
//! 614 `ReplaceZoneDestination` redirect check, CR 303.4b Aura ETB-attach,
//! and the CR 707.6/702.82a/702.38a mid-ETB Copy/Devour/Amplify pause
//! points, in that CR-priority order), while an instant or sorcery just
//! goes to its owner's graveyard/exile. A permanent has no `AbilityEffect`
//! of its own to resolve here — that's why this is a structurally separate
//! path from `resolve_ability_effect`'s dispatch, called directly from
//! `resolve_stack_top`'s shell whenever the popped `StackObject` is a
//! `Spell`.

use std::ops::ControlFlow;

use crate::commands;
use crate::replacement::{self, ReplacementOutcome};
use crate::types::card::CardTypeSet;
use crate::types::cost::colors_spent;
use crate::types::event::GameEventKind;
use crate::types::ids::Target;
use crate::types::stack::StackObject;
use crate::types::zone::ZoneKind;

use super::super::{Engine, ProcessState, StepStatus};

impl Engine {
    pub(super) fn resolve_permanent_entry(
        &mut self,
        top: &StackObject,
        legal_targets: &[Target],
        mut events: Vec<GameEventKind>,
    ) -> ControlFlow<StepStatus, Vec<GameEventKind>> {
        // Peek the card's types before moving it -
        // move_resolving_spell needs the destination decided
        // up front.
        let destination = self.state.object(top.stack_id).map(|obj| {
            if obj.definition.card_types.intersects(CardTypeSet::INSTANT | CardTypeSet::SORCERY) {
                if top.exile_on_leaving_stack {
                    ZoneKind::Exile
                } else {
                    ZoneKind::Graveyard
                }
            } else {
                ZoneKind::Battlefield
            }
        });
        if let Some(destination) = destination {
            // CR 614's general "instead" framing: a
            // registered ReplaceZoneDestination effect can
            // redirect a would-be battlefield entry to a
            // different zone entirely (e.g. "if a creature
            // would enter the battlefield under your
            // control, exile it instead") - proposed and
            // checked *before* move_resolving_spell ever
            // places anything, mirroring
            // sba::resolve_deaths's own propose-then-commit
            // shape for the graveyard side. Only meaningful
            // for a would-be Battlefield destination - the
            // instant/sorcery "remains in its previous
            // zone" redirect above is CR 400.4a's own
            // unconditional rule, untouched by this.
            //
            // Narrow, named gap: only an *externally*
            // granted redirect effect can ever fire here -
            // the entering permanent's own self-registered
            // effects (if any) haven't been registered yet
            // at this point (run_card_script only runs
            // inside move_resolving_spell, below, once the
            // destination is already decided), so a card's
            // own "as this would enter, redirect it
            // instead" text isn't reachable via this path.
            let destination = if destination == ZoneKind::Battlefield {
                let proposed = GameEventKind::ZoneChange {
                    object: top.stack_id,
                    from: ZoneKind::Stack,
                    to: ZoneKind::Battlefield,
                    last_known_info: None,
                };
                match replacement::apply_replacements(&mut self.state, &self.script_runtime, proposed, false) {
                    ReplacementOutcome::Resolved(GameEventKind::ZoneChange { to, .. }) => to,
                    // Defensive - a WouldMoveZone candidate's
                    // own matching payloads only ever
                    // rewrite it back into a ZoneChange;
                    // falls back to the original destination
                    // rather than guessing at an
                    // unreachable shape.
                    ReplacementOutcome::Resolved(_) => destination,
                    // CR 616.1: two or more simultaneous
                    // "instead" redirects for the same
                    // entering permanent is a genuinely rare
                    // combination (two different sources
                    // each independently hating on the same
                    // entry at once) - narrowed to "no
                    // redirect happens" rather than a real
                    // player-choice pause, same posture this
                    // codebase already takes for comparably
                    // rare nested-pause interactions (e.g.
                    // pending_devour_choice's own
                    // sacrifice-triggers-another-pause gap).
                    // TODO: a real AwaitingReplacementChoice
                    // pause here, if a real card ever needs
                    // it - would need its own
                    // ReplacementResumeTarget variant plus
                    // extracting this whole Spell-resolution
                    // tail into a shared continuation, since
                    // nothing has been placed yet at this
                    // point for submit_replacement_choice to
                    // resume into.
                    ReplacementOutcome::NeedsOrderingChoice { .. } => destination,
                }
            } else {
                destination
            };
            let run_script = destination == ZoneKind::Battlefield;
            if let Some(event) =
                self.move_resolving_spell(top.stack_id, destination, run_script)
            {
                // CR 303.4b: an Aura entering the battlefield
                // this way attaches to its (already
                // legality-checked, CR 608.2b, above)
                // sole target instead of entering bare -
                // the only permanent-entry path this engine
                // builds attach for; CR 303.4f/h's other
                // entry shapes need an effect that puts a
                // permanent onto the battlefield already
                // attached, which doesn't exist here yet.
                let new_battlefield_aura = match &event {
                    GameEventKind::ZoneChange { object, to: ZoneKind::Battlefield, .. }
                        if self
                            .state
                            .object(*object)
                            .is_some_and(|o| commands::is_aura(&o.definition)) =>
                    {
                        Some(*object)
                    }
                    _ => None,
                };
                // CR 603.6d/614.1c: `move_resolving_spell`
                // already ran this object's own self-
                // registration script (run_script above),
                // so any self-registered EntersTapped/
                // EntersWithCounters effect is already in
                // state.replacement_effects by now - apply
                // it (and any externally granted one)
                // directly, same as the PlayLand and
                // search-to-battlefield call sites below.
                if let GameEventKind::ZoneChange { object, to: ZoneKind::Battlefield, .. } = &event {
                    let object_id = *object;
                    // CR 707.6/614.12a: checked before even
                    // Devour/Amplify below - becoming a copy
                    // is the most identity-fundamental of
                    // the three mid-ETB choices, and no real
                    // Clone-family card carries Devour/
                    // Amplify anyway (see EntersAsCopy's own
                    // doc comment for why the post-choice
                    // resume doesn't chain back into those
                    // two). Only this call site (a resolving
                    // spell) can raise this pause, same
                    // "PlayLand/Search never check this"
                    // posture pending_devour_choice already
                    // established - see pending_copy_choice's
                    // own doc comment.
                    if let Some((exceptions, candidates)) = replacement::pending_copy_choice(
                        &self.state,
                        &self.script_runtime,
                        object_id,
                        ZoneKind::Stack,
                    ) {
                        events.push(event);
                        self.process_state = ProcessState::AwaitingCopyChoice {
                            object: object_id,
                            controller: top.controller,
                            exceptions,
                            candidates,
                            stack_id: top.stack_id,
                            events_so_far: events,
                            colors_spent: colors_spent(&top.costs_paid),
                        };
                        return ControlFlow::Break(StepStatus::Progressed);
                    }
                    // CR 702.82a: checked *before*
                    // apply_enters_battlefield_modifiers -
                    // only this call site (a resolving
                    // spell) can raise this pause; see
                    // pending_devour_choice's own doc
                    // comment for why PlayLand/Search never
                    // check this at all.
                    if let Some((per_creature, candidates)) = replacement::pending_devour_choice(
                        &self.state,
                        &self.script_runtime,
                        object_id,
                        ZoneKind::Stack,
                        top.controller,
                    ) {
                        events.push(event);
                        self.process_state = ProcessState::AwaitingDevourChoice {
                            object: object_id,
                            controller: top.controller,
                            per_creature,
                            candidates,
                            stack_id: top.stack_id,
                            events_so_far: events,
                        };
                        return ControlFlow::Break(StepStatus::Progressed);
                    }
                    // CR 702.38a: same "checked before
                    // apply_enters_battlefield_modifiers,
                    // only this call site can pause" posture
                    // as Devour above - see
                    // pending_amplify_choice's own doc
                    // comment.
                    if let Some((per_card, candidates)) = replacement::pending_amplify_choice(
                        &self.state,
                        &self.script_runtime,
                        object_id,
                        ZoneKind::Stack,
                        top.controller,
                    ) {
                        events.push(event);
                        self.process_state = ProcessState::AwaitingAmplifyChoice {
                            object: object_id,
                            controller: top.controller,
                            per_card,
                            candidates,
                            stack_id: top.stack_id,
                            events_so_far: events,
                        };
                        return ControlFlow::Break(StepStatus::Progressed);
                    }
                    // CR 702.44b: only a resolving spell can
                    // ever have a nonempty `costs_paid` to
                    // read colors from - `top` is this same
                    // spell's own `StackObject`, captured
                    // above before its zone move.
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
                            events.push(GameEventKind::Attached {
                                object: aura_id,
                                attached_to: target_id,
                            });
                        }
                        // CR 303.4b: an Aura can enchant a player instead of
                        // an object - phase5/aura-enchants-a-player's own
                        // resolution half.
                        Some(Target::Player(player)) => {
                            commands::attach_permanent_to_player(&mut self.state, aura_id, player);
                            events.push(GameEventKind::AttachedToPlayer { object: aura_id, player });
                        }
                        None => {}
                    }
                }
            }
        }
        // else: defensive - the spell's GameObject should
        // always still be at top.stack_id (nothing else
        // removes it while on the stack); None here would
        // mean some other path already mutated it
        // unexpectedly. Skip the zone move rather than
        // panic, same defensive stance as the empty-stack
        // branch above.
        ControlFlow::Continue(events)
    }
}
