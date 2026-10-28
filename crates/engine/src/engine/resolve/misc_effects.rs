//! The remaining `AbilityEffect` arms: `Attach` (Equip), `BecomeCopy`, `ExchangeTextBoxes`,
//! unreachable-shape no-ops and the `None` fallback into the Lua script bridge. See `DESIGN.md`.

use std::ops::ControlFlow;

use crate::commands;
use crate::script::OperationOutcome;
use crate::types::ability::{apply_text_box, AbilityEffect, CopyExceptions};
use crate::types::command::{EngineCommand, PlayerActionKind};
use crate::types::event::GameEventKind;
use crate::types::ids::{ObjectId, PlayerId, ScriptHandle, Target};
use crate::types::stack::StackObject;
use crate::types::target::{TargetRequirement, TargetSpec};
use crate::types::zone::ZoneKind;

use super::super::{Engine, ProcessState, ScriptedZoneMoveThen, StepStatus};

impl Engine {
    /// CR 702.6a: attaches `top.source` to its one legality-checked target like the
    /// Aura attach path. `attach_permanent` cleans up a retarget. Players are skipped.
    pub(super) fn resolve_attach_effect(
        &mut self,
        top: &StackObject,
        legal_targets: &[Target],
        mut events: Vec<GameEventKind>,
    ) -> ControlFlow<StepStatus, Vec<GameEventKind>> {
        if let Some(Target::Object(target_id)) = legal_targets.first().copied() {
            commands::attach_permanent(&mut self.state, top.source, target_id);
            events.push(GameEventKind::Attached { object: top.source, attached_to: target_id });
        }
        ControlFlow::Continue(events)
    }

    /// CR 707.2: `top.source` becomes a copy of its first target's copiable values,
    /// written once to `copiable_override` since layer 1 can't be live-queried
    /// (CR 613.1a). It triggers no enters or leaves effects (CR 707.4). The old
    /// self-registered abilities are purged and the script re-runs on the snapshot.
    pub(super) fn resolve_become_copy_effect(
        &mut self,
        top: &StackObject,
        exceptions: &CopyExceptions,
        mut events: Vec<GameEventKind>,
    ) -> ControlFlow<StepStatus, Vec<GameEventKind>> {
        if let Some(Target::Object(target_id)) = top.targets.first().copied() {
            let source_id = top.source;
            // CR 707.9c: captured before anything mutates, for `retain_name` and `retain_color`.
            let original_definition = self.state.object(source_id).map(|o| o.definition.clone());
            let snapshot = self
                .state
                .object(target_id)
                .map(|obj| obj.copiable_override.clone().unwrap_or_else(|| obj.definition.clone()));
            if let Some(mut snapshot) = snapshot {
                exceptions.apply(&mut snapshot, original_definition.as_ref());
                let became_copy = self.state.objects.get_mut(&source_id).is_some_and(|source_obj| {
                    if source_obj.zone == ZoneKind::Battlefield {
                        source_obj.copiable_override = Some(snapshot.clone());
                        // CR 707.9e
                        source_obj.copy_generation += 1;
                        true
                    } else {
                        false
                    }
                });
                if became_copy {
                    commands::purge_granted_abilities(&mut self.state, source_id);
                    let result = self.script_runtime.run_card_script(&mut self.state, &snapshot, source_id);
                    self.record_script_error(source_id, &snapshot.script_ref, result);
                    // CR 707.9a: Vesuvan's linked upkeep ability.
                    let generation = self.state.object(source_id).map(|o| o.copy_generation).unwrap_or(0);
                    commands::grant_linked_copy_ability(&mut self.state, source_id, exceptions, generation);
                    // CR 707.9e: Spark Double's extra counter. `x_value` is plumbed
                    // through for parity with `EntersAsCopy`.
                    // TODO: no real `BecomeCopy` producer uses `CounterAmount::X`
                    if let Some(counter_event) =
                        commands::apply_copy_additional_counters(source_id, exceptions, top.x_value)
                    {
                        events.push(counter_event);
                    }
                    events.push(GameEventKind::BecameCopy { object: source_id, copied: target_id });
                }
            }
        }
        // No target, or a player: a silent no-op.
        ControlFlow::Continue(events)
    }

    /// CR 612.5: pairwise, so both targets are rechecked against their own
    /// requirement rather than the flattened list, which loses the pairing. Fewer
    /// than two legal objects is a no-op.
    pub(super) fn resolve_exchange_text_boxes_effect(
        &mut self,
        top: &StackObject,
        mut events: Vec<GameEventKind>,
    ) -> ControlFlow<StepStatus, Vec<GameEventKind>> {
        // Inlined `target_is_legal` instead of the caller's closure, whose borrow of
        // `self.state` would span every other match arm.
        let is_legal_now = |t: &Target, req: &TargetRequirement| {
            commands::target_is_legal(
                &self.state,
                &self.script_runtime,
                *t,
                &req.filter,
                top.controller,
                top.source,
                req.exclude_source,
                top.x_value,
            )
        };
        if let (Some(TargetSpec::Multiple(reqs)), [Target::Object(a_id), Target::Object(b_id)]) =
            (&top.target_requirement, top.targets.as_slice())
        {
            let (a_id, b_id) = (*a_id, *b_id);
            if reqs.len() == 2
                && is_legal_now(&Target::Object(a_id), &reqs[0])
                && is_legal_now(&Target::Object(b_id), &reqs[1])
            {
                let a_snapshot = self
                    .state
                    .object(a_id)
                    .map(|obj| obj.copiable_override.clone().unwrap_or_else(|| obj.definition.clone()));
                let b_snapshot = self
                    .state
                    .object(b_id)
                    .map(|obj| obj.copiable_override.clone().unwrap_or_else(|| obj.definition.clone()));
                if let (Some(a_snapshot), Some(b_snapshot)) = (a_snapshot, b_snapshot) {
                    let mut a_new = a_snapshot.clone();
                    apply_text_box(&mut a_new, &b_snapshot);
                    let mut b_new = b_snapshot.clone();
                    apply_text_box(&mut b_new, &a_snapshot);

                    if let Some(obj) = self.state.object_mut(a_id) {
                        obj.copiable_override = Some(a_new.clone());
                    }
                    if let Some(obj) = self.state.object_mut(b_id) {
                        obj.copiable_override = Some(b_new.clone());
                    }
                    commands::purge_granted_abilities(&mut self.state, a_id);
                    commands::purge_granted_abilities(&mut self.state, b_id);
                    let a_result = self.script_runtime.run_card_script(&mut self.state, &a_new, a_id);
                    self.record_script_error(a_id, &a_new.script_ref, a_result);
                    let b_result = self.script_runtime.run_card_script(&mut self.state, &b_new, b_id);
                    self.record_script_error(b_id, &b_new.script_ref, b_result);

                    events.push(GameEventKind::TextBoxesExchanged { a: a_id, b: b_id });
                }
            }
        }
        // Fewer than two object targets, or the wrong shape: a no-op.
        ControlFlow::Continue(events)
    }

    /// Mana abilities never reach the stack (CR 605.3b) and `CastSpell` unpacks
    /// `Script` at push time, so those variants are defensive no-ops. `None` is the
    /// reachable case: it invokes `top.operation` through the script bridge, after
    /// the native fast paths found no match.
    pub(super) fn resolve_misc_or_script_effect(
        &mut self,
        top: &StackObject,
        events: Vec<GameEventKind>,
    ) -> ControlFlow<StepStatus, Vec<GameEventKind>> {
        match &top.effect {
            Some(AbilityEffect::AddMana(_)) | Some(AbilityEffect::AddManaChoice { .. }) => {
                ControlFlow::Continue(events)
            }
            Some(AbilityEffect::Script(_)) => ControlFlow::Continue(events),
            _ => {
                // A script error is logged as a `ScriptError` event; resolution carries
                // on since no path leads back to a `StepStatus` error.
                // Skipped for `ScriptHandle(0)`, the placeholder `cast.rs` leaves on
                // vanilla spells, which would log a false failure every time.
                // TODO: `cast.rs` collapses a `ForEachPlayer`-wrapped handle to `ScriptHandle(0)`, so a plain custom function that is the runtime's first handle reads as the placeholder
                let outcome = self.script_runtime.invoke_operation(top.operation, &mut self.state, top.source, top.controller);
                if let Err(ref e) = outcome {
                    if top.operation != ScriptHandle(0) {
                        self.log_events(vec![GameEventKind::ScriptError {
                            message: format!("operation script error on source {:?}: {e:?}", top.source),
                        }]);
                    }
                }
                match outcome {
                    Ok(OperationOutcome::Finished) | Err(_) => ControlFlow::Continue(events),
                    // The script yielded mid-resolution: pause the run here. `events` moves
                    // into `events_so_far` and `submit_script_choice` finishes the resolution.
                    Ok(OperationOutcome::NeedsChoice { thread, request }) => {
                        self.process_state = ProcessState::AwaitingScriptChoice {
                            thread,
                            request,
                            stack_id: top.stack_id,
                            source: top.source,
                            controller: top.controller,
                            events_so_far: events,
                            each_player: None,
                        };
                        ControlFlow::Break(StepStatus::Progressed)
                    }
                    // No pause: the events fold into this resolution's list like `Finished`.
                    Ok(OperationOutcome::PendingEvents(new_events)) => {
                        let mut events = events;
                        events.extend(new_events);
                        ControlFlow::Continue(events)
                    }
                    // `draw_cards` with no `ForEachPlayer` context.
                    Ok(OperationOutcome::PendingDraw { amount }) => {
                        self.process_state = self.continue_draw_cards(amount, top.controller, top.stack_id, events);
                        ControlFlow::Break(StepStatus::Progressed)
                    }
                    // A standalone script operation has no batching context, so each
                    // verb's ids move immediately.
                    Ok(OperationOutcome::PendingZoneMoves(moves)) => {
                        self.process_state = self.chain_pending_zone_moves(
                            moves,
                            ScriptedZoneMoveThen::StackResolution { stack_id: top.stack_id },
                            events,
                        );
                        ControlFlow::Break(StepStatus::Progressed)
                    }
                    // `Game.Ops.CastSpell` with no `ForEachPlayer` context.
                    Ok(OperationOutcome::PendingCast { card, modes, targets, x_value, free }) => {
                        self.process_state =
                            self.resolve_pending_cast(card, modes, targets, x_value, free, top.stack_id, top.controller, events);
                        ControlFlow::Break(StepStatus::Progressed)
                    }
                }
            }
        }
    }

    /// Shared tail for a script's `CastSpell` result, from an immediate result or a
    /// resumed coroutine. Runs a synthetic `CastSpell` through `execute_command` like
    /// the cascade and madness casts, but inside an active pipeline run, so
    /// `pipeline_context` is left untouched. An illegal cast is logged as a
    /// `ScriptError`; the script is not told why.
    pub(super) fn resolve_pending_cast(
        &mut self,
        card: ObjectId,
        modes: Vec<u32>,
        targets: Vec<Target>,
        x_value: Option<u32>,
        free: bool,
        stack_id: ObjectId,
        controller: PlayerId,
        events_so_far: Vec<GameEventKind>,
    ) -> ProcessState {
        self.state.pending_timing_bypass = Some(card);
        if free {
            self.state.pending_free_cast = Some(card);
        }
        let command_id = self.allocate_command_id();
        let command = EngineCommand {
            command_id,
            actor: controller,
            action: PlayerActionKind::CastSpell { card, modes, targets, x_value, alternative_cost: None, pay_buyback: false },
        };
        match commands::execute_command(&mut self.state, &self.script_runtime, command) {
            Ok(outcome) => {
                let mut events_so_far = events_so_far;
                events_so_far.push(GameEventKind::StackObjectResolved { stack_id });
                events_so_far.extend(outcome.events);
                self.set_pending_payment(outcome.requires_payment);
                if outcome.broke_pass_chain {
                    self.state.consecutive_passes = 0;
                }
                ProcessState::ReplacementCheck { remaining: events_so_far.into(), checked: Vec::new() }
            }
            Err(err) => {
                self.state.pending_timing_bypass = None;
                self.state.pending_free_cast = None;
                // A rejected cast is the script's mistake and is logged below; a broken invariant is fatal.
                if let commands::CommandError::Internal(internal) = &err {
                    self.poison(internal.clone());
                }
                self.log_events(vec![GameEventKind::ScriptError {
                    message: format!("script-requested cast failed on source {stack_id:?}: {err:?}"),
                }]);
                let mut final_events: std::collections::VecDeque<GameEventKind> = events_so_far.into();
                final_events.push_back(GameEventKind::StackObjectResolved { stack_id });
                ProcessState::ReplacementCheck { remaining: final_events, checked: Vec::new() }
            }
        }
    }
}
