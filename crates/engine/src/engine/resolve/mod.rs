//! Resolution of the top stack object (`ResolveStackTop`): the shell, fizzle checks and a
//! one-line-per-variant `AbilityEffect` dispatcher into sibling modules. See `DESIGN.md`.
//! Resolvers return `ControlFlow`: `Break` pauses, `Continue` carries events to the shared tail.
use std::ops::ControlFlow;

use crate::commands;
use crate::engine_context::{PipelineContext, PipelineExit};
use crate::types::ability::{AbilityEffect, InterveningIfSpec};
use crate::types::card::CardTypeSet;
use crate::types::event::GameEventKind;
use crate::types::ids::Target;
use crate::types::stack::{StackEntryKind, StackObject};
use crate::types::target::{TargetRequirement, TargetSpec};
use crate::types::zone::ZoneKind;

mod cascade;
mod counter;
mod destruction;
mod direct_effects;
mod discard;
mod each_player;
mod library_effects;
mod madness;
mod misc_effects;
mod modal;
mod permanent_entry;
mod reanimation;
mod tokens;

use super::{Engine, ProcessState, StepStatus};

pub(in crate::engine) use modal::ModalContinuation;

impl Engine {
    /// CR 608.2m: resolves the top object on the stack. See the module doc.
    pub(super) fn resolve_stack_top(&mut self) -> StepStatus {
        // CR 700.2e: a modal spell whose mode is chosen by another player has nothing
        // resolvable yet, so pause without popping. `submit_mode_choice` patches it
        // in place and re-enters here.
        if let Some(top) = self.state.stack.last() {
            if let Some(pending) = &top.pending_mode_choice {
                let status = StepStatus::AwaitingModeChoice {
                    player: pending.chooser,
                    stack_id: top.stack_id,
                    min: pending.min,
                    max: pending.max,
                };
                self.process_state = ProcessState::AwaitingModeChoice {
                    stack_id: top.stack_id,
                    chooser: pending.chooser,
                    min: pending.min,
                    max: pending.max,
                };
                return status;
            }
        }
        // A leftover continuation from an unfinished resolution must not resume into this one.
        self.modal_continuation = None;
        let Some(mut top) = self.state.stack.pop() else {
            // Defensive: the all-pass check saw a non-empty stack.
            self.process_state = ProcessState::PriorityWindow;
            return StepStatus::Progressed;
        };
        // CR 117.3b: priority returns to the active player once the object resolves.
        self.state.priority_player = self.state.active_player;
        let since_ledger = self.state.ledger.len();
        self.pipeline_context =
            Some(PipelineContext { since_ledger, caused_by_command: None, parent_event: None });
        self.pipeline_exit = PipelineExit::BackToPriority;

        // CR 608.2b: recheck every target against current state. `legal_targets` empty
        // means a full fizzle; otherwise only the illegal targets are skipped.
        // `top.source`, not `top.stack_id`: an ability's stack id is never a real
        // object, so protection and `exclude_source` checks would silently miss.
        let is_legal_now = |t: &Target, req: &TargetRequirement| {
            commands::target_is_legal(
                &self.state,
                &self.script_runtime,
                *t,
                &req.filter,
                top.controller,
                top.source,
                req.exclude_source,
                // CR 202.3: the announced X, as at cast time.
                top.x_value,
            )
        };
        let legal_targets: Vec<Target> = match &top.target_requirement {
            Some(TargetSpec::Primitive(req)) => {
                top.targets.iter().copied().filter(|t| is_legal_now(t, req)).collect()
            }
            // CR 601.2c, 608.2b: each word's legal subset, concatenated for the
            // uniform-verb effect arms. A failed split means the targets were invalid at
            // cast time; fall back to empty instead of panicking.
            Some(TargetSpec::Multiple(reqs)) => {
                commands::split_targets_by_requirement(&self.state, &self.script_runtime, reqs, &top.targets, top.controller, top.source, top.x_value)
                    .map(|chunks| {
                        reqs.iter()
                            .zip(chunks)
                            .flat_map(|(req, chunk)| chunk.into_iter().filter(|t| is_legal_now(t, req)).collect::<Vec<_>>())
                            .collect()
                    })
                    .unwrap_or_default()
            }
            // No targets, or a `Script`-backed requirement that can't be evaluated here.
            _ => top.targets.clone(),
        };
        // CR 700.2d: a Modal effect needs per-mode legal chunks, which the flattened
        // list loses. Computed here so the borrow of `self.state` ends before the
        // mutating match below.
        let modal_chunk_legals: Vec<Vec<Target>> = match &top.target_requirement {
            Some(TargetSpec::Multiple(reqs)) => {
                commands::split_targets_by_requirement(&self.state, &self.script_runtime, reqs, &top.targets, top.controller, top.source, top.x_value)
                    .map(|chunks| {
                        reqs.iter()
                            .zip(chunks)
                            .map(|(req, chunk)| chunk.into_iter().filter(|t| is_legal_now(t, req)).collect())
                            .collect()
                    })
                    .unwrap_or_default()
            }
            _ => Vec::new(),
        };
        let fizzles = match &top.target_requirement {
            // Gated on targets actually chosen, not the requirement's `count`: an "up to
            // N" requirement can legally choose zero and must not fizzle (CR 115.6).
            Some(TargetSpec::Primitive(_) | TargetSpec::Multiple(_)) if !top.targets.is_empty() => {
                legal_targets.is_empty()
            }
            _ => false,
        };

        // CR 603.4: an intervening-if is rechecked at resolution; false removes the
        // ability like a fizzle. `None` never fizzles this way.
        let intervening_if_fizzles = match &top.intervening_if {
            Some(InterveningIfSpec::Primitive(p)) => {
                !p.holds(&self.state, top.controller, top.source, top.source_lki.as_ref())
            }
            // A script error counts as "doesn't hold" and is logged as a `ScriptError`.
            Some(InterveningIfSpec::Script(handle)) => {
                let holds = match self.script_runtime.invoke_intervening_if(
                    *handle,
                    &self.state,
                    top.source,
                    top.controller,
                ) {
                    Ok(holds) => holds,
                    Err(e) => {
                        self.log_events(vec![GameEventKind::ScriptError {
                            message: format!(
                                "intervening-if script error resolving source {:?}: {e:?}",
                                top.source
                            ),
                        }]);
                        false
                    }
                };
                !holds
            }
            None => false,
        };
        let fizzles = fizzles || intervening_if_fizzles;

        let mut events = Vec::new();
        if fizzles {
            // CR 608.2b: a fizzled spell does nothing and lands in its owner's graveyard,
            // or exile if cast with an exiling alternative cost (CR 702.34a).
            if matches!(top.kind, StackEntryKind::Spell) {
                let destination =
                    if top.exile_on_leaving_stack { ZoneKind::Exile } else { ZoneKind::Graveyard };
                if let Some(event) = self.move_resolving_spell(top.stack_id, destination, false, false) {
                    events.push(event);
                }
            }
            events.push(GameEventKind::Fizzled { stack_id: top.stack_id });
        } else {
            // TODO: look up `top.operation` and invoke it via `invoke_operation` once the script bridge allows

            // CR 608.2m: a permanent goes to the battlefield; an instant or sorcery goes
            // to the graveyard or exile and its own effect resolves below. Computed
            // before the zone move removes `top.stack_id` from `objects` (CR 400.7).
            let is_instant_or_sorcery = matches!(top.kind, StackEntryKind::Spell)
                && self.state.object(top.stack_id).is_some_and(|obj| {
                    obj.definition.card_types.intersects(CardTypeSet::INSTANT | CardTypeSet::SORCERY)
                });

            if matches!(top.kind, StackEntryKind::Spell) {
                match self.resolve_permanent_entry(top, &legal_targets, events) {
                    ControlFlow::Break(status) => return status,
                    ControlFlow::Continue((resumed_top, resumed_events)) => {
                        top = resumed_top;
                        events = resumed_events;
                    }
                }
            }
            // Defensive: the spell object is always still at `top.stack_id`.

            // An ability or copy moves no card. They and an instant or sorcery resolve
            // `top.effect` through the dispatcher, or the script bridge when it is `None`
            // (CR 602.2b, 603.4). A permanent spell is excluded above.
            if matches!(
                top.kind,
                StackEntryKind::TriggeredAbility | StackEntryKind::ActivatedAbility
            ) || is_instant_or_sorcery
            {
                match self.resolve_ability_effect(&top, &legal_targets, &modal_chunk_legals, events) {
                    ControlFlow::Break(status) => return status,
                    ControlFlow::Continue(resumed) => events = resumed,
                }
            }
            events.push(GameEventKind::StackObjectResolved { stack_id: top.stack_id });
        }
        self.process_state =
            ProcessState::ReplacementCheck { remaining: events.into(), checked: Vec::new() };
        StepStatus::Progressed
    }

    /// The `top.effect` dispatch: one line per variant, delegating to a sibling module.
    fn resolve_ability_effect(
        &mut self,
        top: &StackObject,
        legal_targets: &[Target],
        modal_chunk_legals: &[Vec<Target>],
        events: Vec<GameEventKind>,
    ) -> ControlFlow<StepStatus, Vec<GameEventKind>> {
        match &top.effect {
            // CR 700.2d: 2+ chosen modes. `target_requirement` is always `Multiple`
            // with one entry per mode. The flattened `legal_targets` loses chunk
            // boundaries, so per-mode chunks are re-derived and rechecked (CR 608.2b);
            // `modal.rs` runs each mode and holds the continuation for pausing modes.
            Some(AbilityEffect::Modal { effects, words }) => {
                self.resolve_modal_effect(top, effects, words, modal_chunk_legals, events)
            }
            // CR 608.2c: a modal loop with every entry chosen and one shared target set.
            Some(AbilityEffect::Chain { entries, words }) => {
                self.resolve_chain_effect(top, entries, words.as_deref(), legal_targets, modal_chunk_legals, events)
            }
            Some(AbilityEffect::GainLife(amount)) => self.resolve_gain_life_effect(top, *amount, events),
            // CR 608.2m: damage emits a `DamageDealt` event through the replacement
            // pipeline so prevention and lifelink apply. Never combat damage (CR 903.10a).
            // CR 608.2b: individually illegal targets are skipped.
            // TODO: divide-as-you-choose damage; each legal target takes the full amount (CR 601.2d)
            Some(AbilityEffect::Damage(amount)) => {
                self.resolve_damage_effect(top, legal_targets, *amount, events)
            }
            // CR 701.17: sacrifice each legal target independently. Never destruction
            // (CR 614.8) and indestructibility doesn't apply (CR 702.12b). An ordering
            // pause (CR 616.1) resumes the remaining targets afterward.
            // TODO: "sacrifice a permanent you control" needs a resolution-time player choice
            Some(AbilityEffect::Sacrifice) => self.resolve_sacrifice_effect(top, legal_targets, events),
            // CR 701.8: destroy each legal target independently. A zone change always
            // describes a completed move, so this can't push an unapplied event. A
            // regeneration shield can intercept it (CR 614.8). An ordering pause
            // (CR 616.1) resumes the remaining targets afterward.
            // TODO: a "can't be regenerated" qualifier (CR 701.19c)
            Some(AbilityEffect::Destroy) => self.resolve_destroy_effect(top, legal_targets, events),
            // CR 509.1h: mutates `CombatState` directly via `mark_effect_blocked`.
            Some(AbilityEffect::BecomeBlocked) => self.resolve_become_blocked_effect(legal_targets, events),
            // CR 701.8: untargeted; collects every battlefield object matching `filter`
            // and uses the same pipeline as `Destroy`, so indestructibility and
            // ordering pauses apply.
            Some(AbilityEffect::DestroyAll(filter)) => self.resolve_destroy_all_effect(top, filter, events),
            // CR 701.21: candidates drawn without replacement via `next_rng`.
            Some(AbilityEffect::SacrificeRandom { filter, count }) => {
                self.resolve_sacrifice_random_effect(top, filter, *count, events)
            }
            // CR 701.18: exile each legal target from the battlefield; indestructibility
            // is irrelevant (CR 702.12b). Ordering pauses work as for `Destroy`.
            // TODO: exiling from the graveyard or hand needs the source zone known at resolution
            Some(AbilityEffect::Exile) => self.resolve_exile_effect(top, legal_targets, events),
            // CR 610.3
            Some(AbilityEffect::ExileUntilSourceLeaves) => self.resolve_exile_until_source_leaves_effect(top, legal_targets, events),
            // CR 400.3
            Some(AbilityEffect::ReturnToHand) => self.resolve_return_to_hand_effect(top, legal_targets, events),
            // CR 400.3: untargeted.
            Some(AbilityEffect::ReturnSelfToHand) => self.resolve_return_self_to_hand_effect(top, events),
            Some(AbilityEffect::ReturnAllToHand { filter, zone }) => self.resolve_return_all_to_hand_effect(top, filter, *zone, events),
            Some(AbilityEffect::ExileSelf) => self.resolve_exile_self_effect(top, events),
            Some(AbilityEffect::ExileAll { filter, zone }) => self.resolve_exile_all_effect(top, filter, *zone, events),
            // CR 110.2a: replacement-checked, with the full entry treatment.
            Some(AbilityEffect::ReturnToBattlefield { control, tapped }) => {
                self.resolve_return_to_battlefield_effect(top, legal_targets, *control, *tapped, events)
            }
            Some(AbilityEffect::ReturnAllToBattlefield { filter, zone, control, tapped }) => {
                self.resolve_return_all_to_battlefield_effect(top, filter, *zone, *control, *tapped, events)
            }
            // CR 701.26: each flips `tapped` and pushes an event only on a real change.
            Some(AbilityEffect::Tap) => self.resolve_tap_effect(legal_targets, true, events),
            Some(AbilityEffect::Untap) => self.resolve_tap_effect(legal_targets, false, events),
            Some(AbilityEffect::TapSelf) => self.resolve_tap_self_effect(top, true, events),
            Some(AbilityEffect::UntapSelf) => self.resolve_tap_self_effect(top, false, events),
            Some(AbilityEffect::TapAll { filter }) => self.resolve_tap_all_effect(top, filter, true, events),
            Some(AbilityEffect::UntapAll { filter }) => self.resolve_tap_all_effect(top, filter, false, events),
            // CR 701.6a: removes a different stack object than `top`.
            Some(AbilityEffect::Counter) => self.resolve_counter_effect(legal_targets, events),
            // CR 118.12: pauses for a mid-resolution payment decision.
            Some(AbilityEffect::UnlessPaid { cost, otherwise }) => {
                self.resolve_unless_paid_effect(top, legal_targets, cost, otherwise, events)
            }
            // CR 701.9b: like `SacrificeRandom`, from the controller's hand.
            Some(AbilityEffect::DiscardRandom { filter, count }) => self.resolve_discard_random_effect(top, filter, *count, events),
            // CR 701.9b: pauses for the targeted player's own choice.
            Some(AbilityEffect::Discard { count }) => self.resolve_discard_effect(top, legal_targets, *count, events),
            // CR 101.4: invokes the operation per player in APNAP order, accumulating
            // zone-move ids so the actions happen simultaneously.
            Some(AbilityEffect::ForEachPlayer(wrapped)) => self.resolve_for_each_player_effect(top, wrapped, events),
            // CR 122.1a: pushes an unapplied `CounterAdded`, like damage. Each legal
            // object target gets the full amount (CR 601.2d); players are skipped.
            Some(AbilityEffect::Counters { kind, amount }) => {
                self.resolve_counters_effect(legal_targets, kind, *amount, events)
            }
            // CR 611.2a: each legal object target gets its own continuous effect pushed
            // directly, pinned to the target with `self_only`.
            Some(AbilityEffect::Pump { power, toughness, duration }) => {
                self.resolve_pump_effect(legal_targets, *power, *toughness, duration, events)
            }
            // CR 613.1b: like `Pump` but layer 2. The new controller is always `top.controller`.
            Some(AbilityEffect::GainControl(duration)) => {
                self.resolve_gain_control_effect(top, legal_targets, duration, events)
            }
            // CR 701.19a: each legal object target gets a one-shot shield pinned with
            // `self_only`. A target that can't be regenerated gets none, checked at
            // creation (CR 701.19c).
            Some(AbilityEffect::Regenerate) => self.resolve_regenerate_effect(legal_targets, events),
            // CR 121: the controller draws, routed through replacement checks so "would
            // draw" effects intercept each draw. A count queue (`ScriptedDraw`) since each
            // draw re-peeks the library. Stops at an empty library.
            // TODO: the empty-library loss (CR 704.5b), as in the draw step
            Some(AbilityEffect::DrawCards(amount)) => {
                self.resolve_draw_cards_effect(top, legal_targets, *amount, events)
            }
            // CR 701.17a: the controller, or the first legal player target.
            Some(AbilityEffect::Mill(amount)) => self.resolve_mill_effect(top, legal_targets, *amount, events),
            // CR 119.3
            Some(AbilityEffect::LoseLife(amount)) => self.resolve_lose_life_effect(top, legal_targets, *amount, events),
            // CR 701.21a: untargeted.
            Some(AbilityEffect::SacrificeSelf) => self.resolve_sacrifice_self_effect(top, events),
            // CR 701.19b: `shuffle_library` logs its own events, so none are pushed.
            Some(AbilityEffect::Shuffle) => self.resolve_shuffle_effect(top, events),
            // CR 701.22a: peeks the top `n` without mutating, then pauses for the
            // controller's decision. `n == 0` or an empty library falls through (CR 701.22b).
            Some(AbilityEffect::Scry(n)) => self.resolve_scry_effect(top, *n, events),
            // CR 701.25a
            Some(AbilityEffect::Surveil(n)) => self.resolve_surveil_effect(top, *n, events),
            // CR 701.23a: searches the whole library, pre-filtered so the answer only
            // needs a subset check. No candidates means no pause.
            Some(AbilityEffect::Search { filter, count, destination, shuffle_after }) => self
                .resolve_search_effect(top, filter, *count, *destination, *shuffle_after, events),
            // CR 707.2: `top.source` becomes a copy of its first target's copiable values,
            // written once to `copiable_override` (layer 1, CR 613.1a). Triggers no
            // enters or leaves effects (CR 707.4). The old self-registered abilities are
            // purged and the script re-runs on the new snapshot.
            Some(AbilityEffect::BecomeCopy(exceptions)) => {
                self.resolve_become_copy_effect(top, exceptions, events)
            }
            // CR 612.5: pairwise, so both targets are rechecked against their own
            // requirement instead of the flattened list. Fewer than two legal objects
            // is a no-op.
            Some(AbilityEffect::ExchangeTextBoxes) => self.resolve_exchange_text_boxes_effect(top, events),
            // CR 701.14: reads per-word chunks to keep the pairing.
            Some(AbilityEffect::Fight) => self.resolve_fight_effect(top, legal_targets, modal_chunk_legals, true, events),
            Some(AbilityEffect::Bite) => self.resolve_fight_effect(top, legal_targets, modal_chunk_legals, false, events),
            // CR 111: creates tokens under the controller's control. Named tokens register
            // their abilities. Scripts are skipped except for `CopyOf` (CR 707.10f), whose
            // definition is a snapshot with abilities. Cessation is enforced by
            // `collect_ceasing_tokens` (CR 111.7). Enters-the-battlefield modifiers apply,
            // with no colors spent since a token is never cast (CR 702.44b).
            // TODO: devour and amplify pauses for tokens; this resolver can't raise a mid-entry pause
            Some(AbilityEffect::CreateToken { spec, count }) => {
                self.resolve_create_token_effect(top, spec, *count, events)
            }
            // CR 702.6a: attaches the Equipment to its one legality-checked target,
            // like the Aura attach path; `attach_permanent` cleans up a retarget.
            Some(AbilityEffect::Attach) => self.resolve_attach_effect(top, legal_targets, events),
            // CR 702.79a: locates the graveyard object via lineage and returns it as a new object (CR 400.7).
            Some(AbilityEffect::ReturnSelfWithCounter(kind)) => {
                self.resolve_return_self_with_counter_effect(top, kind.clone(), events)
            }
            // CR 702.35a: always pauses for a cast-or-decline choice.
            Some(AbilityEffect::MadnessCastOrDiscard) => self.resolve_madness_effect(top, events),
            // CR 702.85a: a synchronous library walk, then a cast-or-decline pause.
            Some(AbilityEffect::Cascade) => self.resolve_cascade_effect(top, events),
            // Mana abilities never reach the stack (CR 605.3b), and `CastSpell` unpacks
            // `Script` at push time, so these are unreachable and defensive no-ops.
            // `None` is the script-bridge fallback in `resolve_misc_or_script_effect`.
            Some(AbilityEffect::AddMana(_))
            | Some(AbilityEffect::AddManaChoice { .. })
            | Some(AbilityEffect::Script(_))
            | None => self.resolve_misc_or_script_effect(top, events),
        }
    }
}
