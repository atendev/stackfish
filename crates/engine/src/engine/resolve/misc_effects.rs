//! `resolve_ability_effect`'s remaining arms that don't fit the other
//! themed siblings: `Attach` (CR 702.6a Equip), `BecomeCopy` (CR 707.2/
//! 707.3), `ExchangeTextBoxes` (CR 612.5), the `AddMana`/`AddManaChoice`/
//! `Script` unreachable-shape no-ops, and the `None`-effect fallback that
//! invokes the real Lua script bridge (`ScriptRuntime::invoke_operation`),
//! pausing on `ProcessState::AwaitingScriptChoice` if the script itself
//! yields mid-resolution (backlog item 9's `Game.SelectTargets`).

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
    /// CR 702.6a: Equip's own resolution - attaches `top.source` (the
    /// Equipment) to its first (only, per CR 702.6a's singular "target
    /// creature") already legality-checked (CR 608.2b, by the caller)
    /// target. Same `commands::attach_permanent` + `GameEventKind::Attached`
    /// convention the Aura ETB-attach path (the `Spell`-kind zone-move
    /// block) already established - `attach_permanent`'s own reattach-
    /// safety fix (`phase5/equip-ability`) means retargeting an already-
    /// equipped Equipment cleans up the old target's attachments list for
    /// free. Object targets only, same "silent no-op" stance every other
    /// arm here takes for an unreachable target shape.
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

    /// CR 707.2/707.3/613.1a (layer 1): `top.source` becomes a copy of its
    /// first chosen target's current copiable values - a real mutation
    /// written once (`GameObject.copiable_override`), not a live-queried
    /// `ContinuousEffect` (see that field's own doc comment for why layer 1
    /// can't fit the other layers' shape). CR 707.4: this never triggers
    /// ETB/LTB (`top.source` stays on the battlefield, same `ObjectId`) -
    /// but CR 707.2a's abilities are copiable, so whatever `top.source` had
    /// self-registered under its pre-copy identity is purged first and
    /// `run_card_script` re-runs against the new snapshot right after.
    pub(super) fn resolve_become_copy_effect(
        &mut self,
        top: &StackObject,
        exceptions: &CopyExceptions,
        mut events: Vec<GameEventKind>,
    ) -> ControlFlow<StepStatus, Vec<GameEventKind>> {
        if let Some(Target::Object(target_id)) = top.targets.first().copied() {
            let source_id = top.source;
            // CR 707.9c/d: the copying object's own
            // true pre-copy definition, for
            // retain_name/retain_color to fall back
            // to - captured before anything mutates.
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
                        // CR 707.9e/g: a real, distinct copy application -
                        // see GameObject::copy_generation's own doc comment.
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
                    // CR 707.9a: Vesuvan Doppelganger's own
                    // "except it has [linked upkeep
                    // ability]" - see
                    // grant_linked_copy_ability's own doc
                    // comment.
                    let generation = self.state.object(source_id).map(|o| o.copy_generation).unwrap_or(0);
                    commands::grant_linked_copy_ability(&mut self.state, source_id, exceptions, generation);
                    // CR 707.9e: Spark Double's own "enters with an
                    // additional +1/+1 counter" - see
                    // apply_copy_additional_counters's own doc comment.
                    // `top.x_value` (`phase5/altered-ego-x-counters`) is
                    // plumbed through for symmetry with the EntersAsCopy/
                    // submit_copy_choice path, though no real BecomeCopy
                    // producer (Vesuvan's triggered ability) needs
                    // CounterAmount::X - TODO: unexercised by a real card,
                    // proven only by direct construction if a test needs it.
                    if let Some(counter_event) =
                        commands::apply_copy_additional_counters(source_id, exceptions, top.x_value)
                    {
                        events.push(counter_event);
                    }
                    events.push(GameEventKind::BecameCopy { object: source_id, copied: target_id });
                }
            }
        }
        // else: no target, or a Target::Player
        // (BecomeCopy only ever targets a permanent)
        // - silent no-op, same stance every other
        // arm here takes for an unreachable target
        // shape.
        ControlFlow::Continue(events)
    }

    /// CR 612.5: inherently pairwise, unlike a uniform multi-word spell
    /// (Plague Spores' "destroy target creature and target land," where
    /// each surviving legal target is still independently destroyed) -
    /// rechecks both targets directly against their own
    /// `TargetRequirement` rather than consuming the flattened
    /// `legal_targets` list, which loses the positional pairing this
    /// role-differentiated effect needs (see `ExchangeTextBoxes`'s own doc
    /// comment). Anything short of two still-legal `Target::Object` targets
    /// is a silent no-op - no partner to exchange with.
    pub(super) fn resolve_exchange_text_boxes_effect(
        &mut self,
        top: &StackObject,
        mut events: Vec<GameEventKind>,
    ) -> ControlFlow<StepStatus, Vec<GameEventKind>> {
        // Not `is_legal_now` (the caller's own closure, in
        // `resolve_stack_top`) - reusing it here would extend its
        // captured `&self.state` borrow across every `&mut
        // self.state` use in every other arm of the original match,
        // since the borrow checker can't see that match arms are
        // mutually exclusive at runtime. Same `target_is_legal`
        // call, inlined instead.
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
        // else: fewer than two Target::Object
        // targets, or the requirement isn't
        // Multiple([_, _]) - silent no-op, same
        // stance BecomeCopy's own arm takes above.
        ControlFlow::Continue(events)
    }

    /// `AddMana`/`AddManaChoice` are only ever set on a mana ability, which
    /// resolves immediately and never reaches the stack (CR 605.1a/605.3b),
    /// so no `StackObject` of any kind should ever carry either variant.
    /// `CastSpell` always unpacks `AbilityEffect::Script` into
    /// `operation`/`effect: None` at push time (see that variant's own doc
    /// comment), so no `StackObject` should ever actually carry it on its
    /// `effect` field either. Both are defensive no-ops rather than a panic.
    ///
    /// `None` is the real, reachable case: look up `top.operation`
    /// (`ScriptHandle`) and invoke it via `ScriptRuntime::invoke_operation`
    /// to actually carry out whatever the spell/ability says (CR 602.2b/
    /// 603.4/608.2m's resolution proper) - `top.effect`'s Rust-native fast
    /// path (the other arms in this dispatch) covers a growing set of
    /// recognized primitives without a Lua call at all, and only falls back
    /// here when none matched.
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
                // A script error here is logged as a GameEventKind::
                // ScriptError rather than silently dropped - no path from
                // ResolveStackTop back to a StepStatus error exists for a
                // mid-resolution script failure, so the resolving spell/
                // ability still falls through to StackObjectResolved/
                // ReplacementCheck as though its effect ran (unchanged
                // fallback), but the failure is now visible in the ledger.
                // Skipped for `ScriptHandle(0)` - `cast.rs`'s own
                // `(operation, effect) = match effect { Some(Script(handle))
                // => (handle, None), other => (ScriptHandle(0), other) }`
                // uses it as "nothing real bound to it," the placeholder
                // every permanent/vanilla spell and every hand-built
                // RegisteredAbility with no script operation carries;
                // logging there would be a guaranteed false "failure" for
                // the overwhelmingly common case rather than a real script
                // bug. Not a fully reserved sentinel, though - confirmed by
                // a real regression while building this slice:
                // `Game.Ops.ForEachPlayer(fn)`'s own wrapped-function
                // handle (`decode_operation_value`'s `ForEachPlayerTag::
                // Function` arm) is a genuinely different, separately
                // allocated handle that `cast.rs`'s match above still
                // discards down to this same `ScriptHandle(0)` (a real,
                // pre-existing bug in that unpacking, not something this
                // slice attempts to fix) - so a `ForEachPlayer`-wrapped
                // custom function is *always* invoked through
                // `each_player.rs`'s own path, never reaches here, and this
                // guard's only real gap is the narrow, pre-existing
                // ambiguity `phase5_script_operation.rs`'s own
                // `operation_function_decodes_a_custom_function_to_a_real_handle`
                // test comment already named: a plain (non-`ForEachPlayer`)
                // custom-function operation that happens to be the very
                // first script handle a fresh `ScriptRuntime` ever allocates
                // would coincidentally read as this same placeholder if it
                // errors. Narrower and pre-existing, not introduced here.
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
                    // Backlog item 9: the script called
                    // Game.SelectTargets and yielded mid-
                    // resolution - pause the whole pipeline
                    // run right here rather than falling
                    // through to StackObjectResolved/
                    // ReplacementCheck below. events (this
                    // resolution's own progress so far, e.g.
                    // a permanent's ETB ZoneChange) moves
                    // into events_so_far untouched -
                    // submit_script_choice pushes the real
                    // StackObjectResolved and continues into
                    // ReplacementCheck once answered.
                    Ok(OperationOutcome::NeedsChoice { thread, request }) => {
                        self.process_state = ProcessState::AwaitingScriptChoice {
                            thread,
                            request,
                            stack_id: top.stack_id,
                            controller: top.controller,
                            events_so_far: events,
                            each_player: None,
                        };
                        ControlFlow::Break(StepStatus::Progressed)
                    }
                    // `phase5/mass-choice-effects`: a standalone (not
                    // ForEachPlayer-wrapped) script operation's own
                    // sacrifice_chosen/discard_chosen result - unlike
                    // resolve_for_each_player_effect's own handling of the
                    // same outcome, there's no batching context here (this
                    // is the only player whose choice this resolution will
                    // ever ask for), so each verb's ids move immediately via
                    // the normal continue_scripted_zone_move pipeline.
                    Ok(OperationOutcome::PendingZoneMoves(moves)) => {
                        self.process_state = self.chain_pending_zone_moves(
                            moves,
                            ScriptedZoneMoveThen::StackResolution { stack_id: top.stack_id },
                            events,
                        );
                        ControlFlow::Break(StepStatus::Progressed)
                    }
                    // `Game.Ops.CastSpell`'s own result, with no `ForEachPlayer`
                    // context (this call site never has one) - see
                    // `resolve_pending_cast`'s own doc comment.
                    Ok(OperationOutcome::PendingCast { card, modes, targets, x_value, free }) => {
                        self.process_state =
                            self.resolve_pending_cast(card, modes, targets, x_value, free, top.stack_id, top.controller, events);
                        ControlFlow::Break(StepStatus::Progressed)
                    }
                }
            }
        }
    }

    /// Shared tail for `Game.Ops.CastSpell`'s own result (`OperationOutcome::
    /// PendingCast`) - called both from a standalone operation's immediate
    /// result (`resolve_misc_or_script_effect` above) and from a resumed
    /// coroutine's result once it stops yielding (`settle_script_operation_
    /// outcome`, `each_player.rs` - e.g. a script that calls `Game.
    /// SelectTargets` first to pick the cast's own targets, then returns
    /// `Game.Ops.CastSpell(...)` once resumed with the answer). Same
    /// synthetic-`PlayerActionKind::CastSpell`-via-`commands::execute_command`
    /// shape `Engine::submit_cascade_cast`/`submit_madness_cast` already
    /// establish for their own Rust-driven equivalents (see those two
    /// methods' `choice_answers.rs` bodies) - unlike those two, every caller
    /// here runs *inside* an already-active pipeline run (this whole
    /// resolution is itself `resolve_stack_top`'s own tail, whether directly
    /// or via a resumed coroutine), so `self.pipeline_context` - already set
    /// for this pass by `resolve_stack_top` itself - is deliberately left
    /// untouched, unlike the two Rust-driven callers (each a fresh top-level
    /// entry point answering a raised pause, not a nested call).
    ///
    /// A script-requested cast that turns out illegal (bad target, wrong
    /// zone, ...) is logged as a `GameEventKind::ScriptError` rather than
    /// silently dropped - same fallback `resolve_misc_or_script_effect`'s
    /// own `invoke_operation` arm takes. There's still no path back to the
    /// script itself to report why the cast didn't happen (`OperationOutcome
    /// ::PendingCast` is fire-and-forget); this only makes the failure
    /// observable in the ledger.
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
                self.pending_payment = outcome.requires_payment;
                if outcome.broke_pass_chain {
                    self.state.consecutive_passes = 0;
                }
                ProcessState::ReplacementCheck { remaining: events_so_far.into(), checked: Vec::new() }
            }
            Err(err) => {
                self.state.pending_timing_bypass = None;
                self.state.pending_free_cast = None;
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
