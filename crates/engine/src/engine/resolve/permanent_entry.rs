//! `resolve_permanent_entry` — the "a resolving `Spell` leaves the stack"
//! block of CR 608.2m resolution, split out of the original fully-inlined
//! `ProcessState::ResolveStackTop` arm. Covers both directions a `Spell` can
//! leave the stack: a permanent type goes to the battlefield (with the CR
//! 614/616.1 `ReplaceZoneDestination` redirect check, CR 303.4b Aura
//! ETB-attach, and the CR 707.6/702.82a/702.38a mid-ETB Copy/Devour/Amplify
//! pause points, in that CR-priority order), while an instant or sorcery just
//! goes to its owner's graveyard/exile. A permanent has no `AbilityEffect`
//! of its own to resolve here — that's why this is a structurally separate
//! path from `resolve_ability_effect`'s dispatch, called directly from
//! `resolve_stack_top`'s shell whenever the popped `StackObject` is a
//! `Spell`.
//!
//! `top` is owned (not borrowed) by both functions here, unlike every other
//! `resolve_*`/`continue_*` helper in this module — the CR 616.1 redirect
//! pause below (`continue_permanent_entry`'s own caller) has to carry the
//! *entire* not-yet-placed `StackObject` across a real pause
//! (`ReplacementResumeTarget::PermanentEntry`), since nothing about it has
//! been committed to any zone yet at that point, unlike every other pause
//! this engine raises (which only ever need to remember a handful of already-
//! on-the-battlefield scalar ids).

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
        // Peek the card's types before moving it -
        // move_resolving_spell needs the destination decided
        // up front.
        let destination = self.state.object(top.stack_id).map(|obj| {
            if obj.definition.card_types.intersects(CardTypeSet::INSTANT | CardTypeSet::SORCERY) {
                // CR 702.27a: buyback checked before exile_on_leaving_stack -
                // no real card combines the two, but if one somehow did,
                // "returns to hand" is the more specific, more recently
                // chosen alternative.
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
        // else: defensive - the spell's GameObject should
        // always still be at top.stack_id (nothing else
        // removes it while on the stack); None here would
        // mean some other path already mutated it
        // unexpectedly. Skip the zone move rather than
        // panic, same defensive stance as the empty-stack
        // branch below.
        let Some(destination) = destination else {
            return ControlFlow::Continue((top, events));
        };
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
                // CR 616.1: two or more simultaneous "instead"
                // redirects for the same entering permanent - a
                // genuinely rare combination (two different sources
                // each independently hating on the same entry at
                // once), but a real one, so it gets a real ordering
                // choice like every other replacement collision -
                // this pause is the one place in the pipeline where
                // nothing has been placed anywhere yet, so the whole
                // owned `top`/`legal_targets` has to ride along in
                // `ReplacementResumeTarget::PermanentEntry` for
                // `Engine::submit_replacement_choice` to hand back to
                // `continue_permanent_entry` once it's answered.
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

    /// The rest of CR 608.2m permanent-entry resolution, once `destination`
    /// is finally settled (either synchronously, from `resolve_permanent_
    /// entry`'s own redirect check above, or after a CR 616.1 ordering choice
    /// on that same redirect check answers via `submit_replacement_choice`).
    /// A `Break` here carries the `ProcessState` to install directly (a
    /// further Copy/Devour/Amplify mid-ETB pause) rather than a `StepStatus`
    /// — `resolve_permanent_entry` and `submit_replacement_choice` each
    /// finish that off their own way (the former returns a real `StepStatus`
    /// up through `step()`; the latter's `resume: match { ... }` arms already
    /// evaluate directly to a `ProcessState`).
    pub(in crate::engine) fn continue_permanent_entry(
        &mut self,
        top: StackObject,
        legal_targets: &[Target],
        mut events: Vec<GameEventKind>,
        destination: ZoneKind,
    ) -> ControlFlow<ProcessState, (StackObject, Vec<GameEventKind>)> {
        let run_script = destination == ZoneKind::Battlefield;
        if let Some(event) = self.move_resolving_spell(top.stack_id, destination, run_script) {
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
                    if self.state.object(*object).is_some_and(|o| commands::is_aura(&o.definition)) =>
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
                    return ControlFlow::Break(ProcessState::AwaitingCopyChoice {
                        object: object_id,
                        controller: top.controller,
                        exceptions,
                        candidates,
                        stack_id: top.stack_id,
                        events_so_far: events,
                        colors_spent: colors_spent(&top.costs_paid),
                        // CR 707.9e: this is the first pause for this
                        // entering object - nothing has copied anything yet,
                        // so there's no outer round's exception to fall back
                        // to if this choice is declined.
                        carry_forward_if_declined: None,
                    });
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
                    return ControlFlow::Break(ProcessState::AwaitingDevourChoice {
                        object: object_id,
                        controller: top.controller,
                        per_creature,
                        candidates,
                        stack_id: top.stack_id,
                        events_so_far: events,
                    });
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
                    return ControlFlow::Break(ProcessState::AwaitingAmplifyChoice {
                        object: object_id,
                        controller: top.controller,
                        per_card,
                        candidates,
                        stack_id: top.stack_id,
                        events_so_far: events,
                    });
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
                        events.push(GameEventKind::Attached { object: aura_id, attached_to: target_id });
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
        ControlFlow::Continue((top, events))
    }
}
