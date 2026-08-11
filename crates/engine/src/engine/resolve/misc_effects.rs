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
use crate::types::event::GameEventKind;
use crate::types::ids::Target;
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
                    // TODO: a script error here is silently
                    // dropped, same gap every other
                    // run_card_script call site in this file
                    // already flags.
                    let _ = self.script_runtime.run_card_script(&mut self.state, &snapshot, source_id);
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
                    // TODO: a script error here is
                    // silently dropped, same gap
                    // every other run_card_script
                    // call site in this file already
                    // flags.
                    let _ = self.script_runtime.run_card_script(&mut self.state, &a_new, a_id);
                    let _ = self.script_runtime.run_card_script(&mut self.state, &b_new, b_id);

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
                // TODO: a script error here is silently
                // dropped - no path from ResolveStackTop
                // back to a StepStatus error exists for a
                // mid-resolution script failure yet.
                match self.script_runtime.invoke_operation(top.operation, &mut self.state, top.source, top.controller) {
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
                }
            }
        }
    }
}
