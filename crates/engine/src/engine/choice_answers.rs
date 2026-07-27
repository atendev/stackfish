//! Answers to the remaining `ResolveStackTop`-raised mid-resolution pauses —
//! CR 303.4d Aura attach, backlog-item-9/9a script choices, CR 702.82a
//! Devour, CR 702.38a Amplify, CR 700.2e modal, and CR 707.6/614.12a copy —
//! split out of `engine.rs`.

use std::collections::HashSet;

use crate::commands;
use crate::replacement;
use crate::script::{OperationOutcome, ScriptChoiceRequest};
use crate::types::ability::AbilityEffect;
use crate::types::counter::CounterKind;
use crate::types::event::GameEventKind;
use crate::types::ids::{ObjectId, Target};
use crate::types::target::{TargetFilter, TargetRequirement, TargetSpec};
use crate::types::zone::ZoneKind;

use super::{
    AmplifyChoiceError, AttachChoiceError, CopyChoiceError, DevourChoiceError, Engine, ModeChoiceError,
    ProcessState, ScriptChoiceError, ScriptedZoneMoveKind,
};

impl Engine {
    /// Answers a pending CR 303.4d attach choice raised by
    /// `StepStatus::AwaitingAttachChoice` — `chosen` must be one of that
    /// status's `candidates`. Dormant as of this slice (see
    /// `AttachChoiceError`'s own doc comment) — nothing in real gameplay
    /// reaches this pause yet, so `resume`s to the generic `PriorityWindow`
    /// rather than a specific continuation, unlike `submit_legend_rule_choice`/
    /// `submit_commander_return_choice`'s `SbaResumeThen` (there's no real
    /// pipeline stage to hand back to).
    pub fn submit_attach_choice(&mut self, chosen: ObjectId) -> Result<(), AttachChoiceError> {
        let ProcessState::AwaitingAttachChoice { candidates, .. } = &self.process_state else {
            return Err(AttachChoiceError::NotAwaitingChoice);
        };
        if !candidates.contains(&chosen) {
            return Err(AttachChoiceError::InvalidCandidate);
        }
        let ProcessState::AwaitingAttachChoice { aura, .. } =
            std::mem::replace(&mut self.process_state, ProcessState::PriorityWindow)
        else {
            unreachable!("checked above");
        };
        commands::attach_permanent(&mut self.state, aura, chosen);
        Ok(())
    }

    /// Answers a pending backlog-item-9 `Game.SelectTargets` pause raised by
    /// `StepStatus::AwaitingScriptChoice` — `chosen` must satisfy the
    /// request's own `TargetFilter` (checked via the same `target_is_legal`
    /// a spell's static target requirement already uses; that check already
    /// branches correctly on `Target::Player`/`Target::Object` for
    /// `TargetFilter::Player`/`AnyTarget`, so widening `chosen` from
    /// `ObjectId` to `Target` here needed no change to `target_is_legal`
    /// itself). Deliberately a separate entry point from `submit_action`,
    /// same reason `submit_replacement_choice`/`submit_legend_rule_choice`
    /// are.
    ///
    /// Resumes the suspended coroutine (`ScriptRuntime::resume_operation`)
    /// with `chosen`. A script error on resume is silently dropped, same
    /// pre-existing "no path from `ResolveStackTop` back to a `StepStatus`
    /// error for a mid-resolution script failure" stance the original
    /// (non-yielding) `invoke_operation` call already had — not a new gap
    /// this method introduces.
    pub fn submit_script_choice(&mut self, chosen: Target) -> Result<(), ScriptChoiceError> {
        let ProcessState::AwaitingScriptChoice { request, controller, stack_id, .. } = &self.process_state else {
            return Err(ScriptChoiceError::NotAwaitingChoice);
        };
        let ScriptChoiceRequest::SelectTarget { filter } = request else {
            return Err(ScriptChoiceError::WrongRequestKind);
        };
        // CR 702.16c: the resolving StackObject's own id is the "source"
        // protection checks against, not the controller - same distinction
        // validate_targets' own callers already make for a static target
        // requirement.
        // TODO(CR 115.4/"another target"): ScriptChoiceRequest::SelectTarget
        // carries a bare TargetFilter, not a full TargetRequirement, so
        // there's no exclude_source to read here - a Game.SelectTargets
        // mid-resolution choice can't express "another target" yet, only a
        // static card-level target_requirement can (see validate_targets'
        // own call site).
        if !commands::target_is_legal(&self.state, &self.script_runtime, chosen, filter, *controller, *stack_id, false)
        {
            return Err(ScriptChoiceError::InvalidCandidate);
        }
        let ProcessState::AwaitingScriptChoice { thread, stack_id, controller, mut events_so_far, .. } =
            std::mem::replace(&mut self.process_state, ProcessState::PriorityWindow)
        else {
            unreachable!("checked above");
        };

        let outcome = self
            .script_runtime
            .resume_operation(thread, &mut self.state, controller, chosen)
            .unwrap_or(OperationOutcome::Finished);
        self.process_state = match outcome {
            OperationOutcome::Finished => {
                events_so_far.push(GameEventKind::StackObjectResolved { stack_id });
                ProcessState::ReplacementCheck { remaining: events_so_far.into(), checked: Vec::new() }
            }
            OperationOutcome::NeedsChoice { thread, request } => {
                ProcessState::AwaitingScriptChoice { thread, request, stack_id, controller, events_so_far }
            }
        };
        Ok(())
    }

    /// Answers a pending backlog-item-9a `Game.MayChoose` pause (CR 608.2d)
    /// raised by `StepStatus::AwaitingScriptChoice` — sibling of
    /// `submit_script_choice` for the boolean-answer request shape instead
    /// of the target-answer one. No legality check: unlike `SelectTarget`,
    /// any `bool` is a legal answer to a "may" choice. Deliberately a
    /// separate entry point rather than widening `submit_script_choice`'s
    /// own `chosen: Target` parameter — same "one method per pause shape"
    /// precedent `submit_replacement_choice`/`submit_legend_rule_choice`
    /// already set.
    pub fn submit_script_may_choice(&mut self, chosen: bool) -> Result<(), ScriptChoiceError> {
        let ProcessState::AwaitingScriptChoice { request, .. } = &self.process_state else {
            return Err(ScriptChoiceError::NotAwaitingChoice);
        };
        let ScriptChoiceRequest::MayChoose = request else {
            return Err(ScriptChoiceError::WrongRequestKind);
        };
        let ProcessState::AwaitingScriptChoice { thread, stack_id, controller, mut events_so_far, .. } =
            std::mem::replace(&mut self.process_state, ProcessState::PriorityWindow)
        else {
            unreachable!("checked above");
        };

        let outcome = self
            .script_runtime
            .resume_operation_bool(thread, &mut self.state, controller, chosen)
            .unwrap_or(OperationOutcome::Finished);
        self.process_state = match outcome {
            OperationOutcome::Finished => {
                events_so_far.push(GameEventKind::StackObjectResolved { stack_id });
                ProcessState::ReplacementCheck { remaining: events_so_far.into(), checked: Vec::new() }
            }
            OperationOutcome::NeedsChoice { thread, request } => {
                ProcessState::AwaitingScriptChoice { thread, request, stack_id, controller, events_so_far }
            }
        };
        Ok(())
    }

    /// Answers a pending CR 702.82a Devour choice raised by `StepStatus::
    /// AwaitingDevourChoice` — `sacrificed` must be a subset (any size,
    /// including empty — CR 702.82a's "any number") of that status's own
    /// `candidates`, no duplicates. Performs the sacrifices via
    /// `continue_scripted_zone_move` (so CR 616.1's ordering choice and
    /// dies-triggers all apply exactly as they would for a real `Sacrifice`
    /// ability effect), then folds this resolution's own pre-choice
    /// `events_so_far` plus a `CounterAdded` (if `sacrificed` wasn't empty)
    /// into that same continuation's `trailing` parameter, so both reach
    /// `ReplacementCheck`/`CommitEvents` together with the sacrifice's own
    /// events, not as a separate untracked mutation.
    pub fn submit_devour_choice(&mut self, sacrificed: Vec<ObjectId>) -> Result<(), DevourChoiceError> {
        let ProcessState::AwaitingDevourChoice { candidates, .. } = &self.process_state else {
            return Err(DevourChoiceError::NotAwaitingChoice);
        };
        if !sacrificed.iter().all(|id| candidates.contains(id)) {
            return Err(DevourChoiceError::InvalidCandidate);
        }
        let mut seen = HashSet::new();
        if !sacrificed.iter().all(|id| seen.insert(*id)) {
            return Err(DevourChoiceError::DuplicateCandidate);
        }
        let ProcessState::AwaitingDevourChoice { object, per_creature, stack_id, mut events_so_far, .. } =
            std::mem::replace(&mut self.process_state, ProcessState::PriorityWindow)
        else {
            unreachable!("checked above");
        };
        // CR 614.13/701.19b-style consumption bookkeeping - same helper
        // apply_enters_battlefield_modifiers's own tail uses for every
        // other EntersWithCounters* payload, called here instead since this
        // payload is never routed through that function at all (see
        // EntersWithCountersDevour's own doc comment).
        let candidate_event =
            GameEventKind::ZoneChange { object, from: ZoneKind::Stack, to: ZoneKind::Battlefield, last_known_info: None };
        replacement::apply_one_replacement(&mut self.state, object, candidate_event);
        let amount = per_creature * sacrificed.len() as i32;
        if amount > 0 {
            events_so_far.push(GameEventKind::CounterAdded { object, kind: CounterKind::PlusOnePlusOne, amount });
        }
        self.process_state = self.continue_scripted_zone_move(
            ScriptedZoneMoveKind::Sacrifice,
            sacrificed.into(),
            stack_id,
            events_so_far,
        );
        Ok(())
    }

    /// Answers a pending CR 702.38a Amplify choice raised by `StepStatus::
    /// AwaitingAmplifyChoice` — `revealed` must be a subset (any size,
    /// including empty — CR 702.38a's "any number") of that status's own
    /// `candidates`, no duplicates. Unlike `submit_devour_choice`, revealing
    /// a hand card doesn't move it anywhere or trigger anything, so this
    /// just folds `events_so_far` plus a `CounterAdded` (if `revealed` wasn't
    /// empty) straight into the same `StackObjectResolved`/`ReplacementCheck`
    /// tail every other synchronous-only resolution already ends on — no
    /// `continue_scripted_zone_move` needed.
    pub fn submit_amplify_choice(&mut self, revealed: Vec<ObjectId>) -> Result<(), AmplifyChoiceError> {
        let ProcessState::AwaitingAmplifyChoice { candidates, .. } = &self.process_state else {
            return Err(AmplifyChoiceError::NotAwaitingChoice);
        };
        if !revealed.iter().all(|id| candidates.contains(id)) {
            return Err(AmplifyChoiceError::InvalidCandidate);
        }
        let mut seen = HashSet::new();
        if !revealed.iter().all(|id| seen.insert(*id)) {
            return Err(AmplifyChoiceError::DuplicateCandidate);
        }
        let ProcessState::AwaitingAmplifyChoice { object, per_card, stack_id, mut events_so_far, .. } =
            std::mem::replace(&mut self.process_state, ProcessState::PriorityWindow)
        else {
            unreachable!("checked above");
        };
        // CR 614.13/701.19b-style consumption bookkeeping - same helper
        // apply_enters_battlefield_modifiers's own tail uses for every other
        // EntersWithCounters* payload, called here instead since this
        // payload is never routed through that function at all (see
        // EntersWithCountersAmplify's own doc comment).
        let candidate_event =
            GameEventKind::ZoneChange { object, from: ZoneKind::Stack, to: ZoneKind::Battlefield, last_known_info: None };
        replacement::apply_one_replacement(&mut self.state, object, candidate_event);
        let amount = per_card * revealed.len() as i32;
        if amount > 0 {
            events_so_far.push(GameEventKind::CounterAdded { object, kind: CounterKind::PlusOnePlusOne, amount });
        }
        events_so_far.push(GameEventKind::StackObjectResolved { stack_id });
        self.process_state = ProcessState::ReplacementCheck { remaining: events_so_far.into(), checked: Vec::new() };
        Ok(())
    }

    /// Answers a pending CR 700.2e "chosen by opponent" modal choice raised
    /// by `StepStatus::AwaitingModeChoice` — `modes` must have between `min`
    /// and `max` entries (that status's own announced range), no duplicates,
    /// each a valid index into the spell's own `CardDefinition.modes`. Builds
    /// `target_requirement`/`effect` exactly the way `commands::
    /// execute_command`'s `CastSpell` arm would have for the caster-chosen
    /// variants (bare single-mode shape for one chosen mode, `TargetSpec::
    /// Multiple`/`AbilityEffect::Modal` for 2+ — see that arm's own doc
    /// comment), then patches the still-on-the-stack `StackObject` in place
    /// and clears `pending_mode_choice` back to `None` so the very next
    /// `step()` re-enters `ProcessState::ResolveStackTop` and actually
    /// resolves it. Narrowed to the same target-less/cost-less mode shape
    /// `ModeChoice::ExactlyChosenByOpponent`/`UpToChosenByOpponent` already
    /// enforce at cast time, so unlike the caster-chosen path there's no
    /// target validation to redo here.
    pub fn submit_mode_choice(&mut self, modes: Vec<u32>) -> Result<(), ModeChoiceError> {
        let ProcessState::AwaitingModeChoice { stack_id, min, max, .. } = &self.process_state else {
            return Err(ModeChoiceError::NotAwaitingChoice);
        };
        let (stack_id, min, max) = (*stack_id, *min, *max);

        let chosen_count = modes.len() as u32;
        if chosen_count < min || chosen_count > max {
            return Err(ModeChoiceError::InvalidCount);
        }
        let mut seen = HashSet::new();
        if !modes.iter().all(|m| seen.insert(*m)) {
            return Err(ModeChoiceError::DuplicateMode);
        }

        let Some(definition) = self.state.object(stack_id).map(|o| o.definition.clone()) else {
            return Err(ModeChoiceError::NoSuchMode);
        };
        let mut chosen_indices = modes;
        chosen_indices.sort_unstable();
        let mut chosen_modes = Vec::with_capacity(chosen_indices.len());
        for &idx in &chosen_indices {
            let Some(mode) = definition.modes.get(idx as usize) else {
                return Err(ModeChoiceError::NoSuchMode);
            };
            chosen_modes.push(mode);
        }

        let (target_requirement, effect) = if let [only] = chosen_modes.as_slice() {
            (only.target_requirement.clone(), only.effect.clone())
        } else {
            for mode in &chosen_modes {
                if let Some(effect) = &mode.effect {
                    if !commands::is_modal_resolvable(effect) {
                        return Err(ModeChoiceError::UnsupportedModeCombination);
                    }
                }
            }
            // Every mode is target-less by this `ModeChoice` variant's own
            // cast-time narrowing - a synthetic zero-count placeholder per
            // chosen mode, same shape the caster-chosen multi-mode path
            // uses for a target-less mode.
            let reqs: Vec<TargetRequirement> = chosen_modes
                .iter()
                .map(|_| TargetRequirement {
                    filter: TargetFilter::AnyTarget,
                    count: 0,
                    up_to: false,
                    at_least: false,
                    exclude_source: false,
                })
                .collect();
            let effects: Vec<Option<AbilityEffect>> = chosen_modes.iter().map(|m| m.effect.clone()).collect();
            (Some(TargetSpec::Multiple(reqs)), Some(AbilityEffect::Modal(effects)))
        };

        let Some(top) = self.state.stack.last_mut().filter(|o| o.stack_id == stack_id) else {
            return Err(ModeChoiceError::NoSuchMode);
        };
        top.chosen_modes = chosen_indices;
        top.target_requirement = target_requirement;
        top.effect = effect;
        top.pending_mode_choice = None;

        self.process_state = ProcessState::ResolveStackTop;
        Ok(())
    }

    /// Answers a pending CR 707.6/614.12a "enter as a copy" choice (Clone
    /// itself) raised by `StepStatus::AwaitingCopyChoice` — `chosen` must be
    /// `None` (CR 707.6's "may," declining) or `Some` of one of that
    /// status's own `candidates`. If a copy is chosen, reuses
    /// `AbilityEffect::BecomeCopy`'s own machinery wholesale: applies
    /// `exceptions` to the chosen creature's current snapshot, purges
    /// whatever `object` had self-registered under its pre-copy identity,
    /// sets `copiable_override`, and re-runs `run_card_script` against the
    /// new snapshot — then logs the same `GameEventKind::BecameCopy`
    /// `BecomeCopy` does, since it's exactly the same fact. Either way
    /// (copied or not), the resume then runs `apply_enters_battlefield_
    /// modifiers` with the pause's own carried-through `colors_spent` -
    /// this is what makes a copied Fading/EntersTapped/Sunburst creature's
    /// own ETB modifiers apply for real, not just its copied keywords.
    pub fn submit_copy_choice(&mut self, chosen: Option<ObjectId>) -> Result<(), CopyChoiceError> {
        let ProcessState::AwaitingCopyChoice { candidates, .. } = &self.process_state else {
            return Err(CopyChoiceError::NotAwaitingChoice);
        };
        if chosen.is_some_and(|id| !candidates.contains(&id)) {
            return Err(CopyChoiceError::InvalidCandidate);
        }
        let ProcessState::AwaitingCopyChoice { object, exceptions, stack_id, mut events_so_far, colors_spent, .. } =
            std::mem::replace(&mut self.process_state, ProcessState::PriorityWindow)
        else {
            unreachable!("checked above");
        };
        // CR 614.13/701.19b-style consumption bookkeeping - same helper
        // apply_enters_battlefield_modifiers's own tail uses for every
        // other EntersWithCounters* payload, called here instead since this
        // payload is never routed through that function at all (see
        // EntersAsCopy's own doc comment).
        let candidate_event =
            GameEventKind::ZoneChange { object, from: ZoneKind::Stack, to: ZoneKind::Battlefield, last_known_info: None };
        replacement::apply_one_replacement(&mut self.state, object, candidate_event);
        if let Some(copied) = chosen {
            // CR 707.9c/d: `object` (entering as the copy) is the one whose
            // own pre-copy definition retain_name/retain_color fall back to
            // - captured before anything mutates.
            let original_definition = self.state.object(object).map(|o| o.definition.clone());
            let snapshot = self.state.object(copied).map(|obj| {
                obj.copiable_override.clone().unwrap_or_else(|| obj.definition.clone())
            });
            if let Some(mut snapshot) = snapshot {
                exceptions.apply(&mut snapshot, original_definition.as_ref());
                if let Some(obj) = self.state.object_mut(object) {
                    obj.copiable_override = Some(snapshot.clone());
                }
                commands::purge_granted_abilities(&mut self.state, object);
                // TODO: a script error here is silently dropped, same gap
                // every other run_card_script call site in this file
                // already flags.
                let _ = self.script_runtime.run_card_script(&mut self.state, &snapshot, object);
                events_so_far.push(GameEventKind::BecameCopy { object, copied });
            }
            // else: the chosen creature no longer resolves (departed
            // between the pause and this answer) - skip rather than guess,
            // same stance every other unresolvable-reference case in this
            // codebase already takes; `object` simply stays itself.
        }
        let counter_events = replacement::apply_enters_battlefield_modifiers(
            &mut self.state,
            &self.script_runtime,
            object,
            ZoneKind::Stack,
            colors_spent,
        );
        events_so_far.extend(counter_events);
        events_so_far.push(GameEventKind::StackObjectResolved { stack_id });
        self.process_state = ProcessState::ReplacementCheck { remaining: events_so_far.into(), checked: Vec::new() };
        Ok(())
    }
}
