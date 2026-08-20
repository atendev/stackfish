//! Answers to the remaining `ResolveStackTop`-raised mid-resolution pauses —
//! CR 303.4d Aura attach, backlog-item-9/9a script choices, CR 702.82a
//! Devour, CR 702.38a Amplify, CR 700.2e modal, and CR 707.6/614.12a copy —
//! split out of `engine.rs`.

use std::collections::HashSet;

use crate::commands;
use crate::engine_context::PipelineContext;
use crate::replacement::{self, ReplacementOutcome};
use crate::script::{OperationOutcome, ScriptChoiceRequest};
use crate::types::ability::{AbilityEffect, CopyExceptions};
use crate::types::command::{EngineCommand, PlayerActionKind};
use crate::types::cost::{AlternativeCost, CostSpec, PaymentPlan};
use crate::types::counter::CounterKind;
use crate::types::event::GameEventKind;
use crate::types::game::GameState;
use crate::types::ids::{ObjectId, Target};
use crate::types::target::{TargetFilter, TargetRequirement, TargetSpec};
use crate::types::zone::ZoneKind;

use super::{
    AmplifyChoiceError, AttachChoiceError, CascadeChoiceError, CopyChoiceError, CounterUnlessPaidError,
    DevourChoiceError, Engine, MadnessChoiceError, ModeChoiceError, ProcessState, ReplacementResumeTarget,
    ScriptChoiceError, ScriptedZoneMoveKind, ScriptedZoneMoveThen,
};

/// CR 707.9a/e: applies whatever "additional effect" exceptions
/// `exceptions` carries for real (Vesuvan's linked upkeep ability, Spark
/// Double's extra counter) — shared by `submit_copy_choice`'s "no chain"
/// path and its "chained pause was declined" path, since both mean the
/// same thing: this round's copy application was never superseded by a
/// later one (CR 707.9e's own condition for suppression).
fn apply_copy_additional_effect(
    state: &mut GameState,
    object: ObjectId,
    exceptions: &CopyExceptions,
    x_value: Option<u32>,
    events_so_far: &mut Vec<GameEventKind>,
) {
    let generation = state.object(object).map(|o| o.copy_generation).unwrap_or(0);
    commands::grant_linked_copy_ability(state, object, exceptions, generation);
    if let Some(counter_event) = commands::apply_copy_additional_counters(object, exceptions, x_value) {
        events_so_far.push(counter_event);
    }
}

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

    /// Answers a pending CR 303.4f/301.5e/301.6 token-creation entry attach
    /// choice raised by `StepStatus::AwaitingTokenAttachChoice`
    /// (`phase5/token-enters-attached`) — `chosen` must be one of that
    /// status's `candidates`. Unlike `submit_attach_choice`, this resumes a
    /// real continuation: `continue_create_token` picks back up with
    /// whatever tokens this `CreateToken` effect still owes
    /// (`remaining_count`). Mirrors `submit_entry_attach_choice`'s own
    /// object/player dispatch exactly, but the tail differs: since this
    /// pause is reached mid-`ResolveStackTop` (not via a separate library
    /// pre-phase the way Search's own pause is), finishing the resumed loop
    /// without a further pause means *this* method - not `resolve_stack_
    /// top`'s own shared tail, which never runs for a resumed pause - must
    /// push `StackObjectResolved` and transition to `ReplacementCheck`
    /// itself, the same "bypassing the normal `ControlFlow::Continue`
    /// caller" tail shape `submit_amplify_choice` already establishes for
    /// the identical reason.
    pub fn submit_token_attach_choice(&mut self, chosen: Target) -> Result<(), AttachChoiceError> {
        let ProcessState::AwaitingTokenAttachChoice { candidates, .. } = &self.process_state else {
            return Err(AttachChoiceError::NotAwaitingChoice);
        };
        if !candidates.contains(&chosen) {
            return Err(AttachChoiceError::InvalidCandidate);
        }
        let ProcessState::AwaitingTokenAttachChoice {
            attaching, controller, stack_id, mut events_so_far, remaining_count, spec, copy_snapshot, ..
        } = std::mem::replace(&mut self.process_state, ProcessState::PriorityWindow)
        else {
            unreachable!("checked above");
        };
        match chosen {
            Target::Object(target_id) => {
                commands::attach_permanent(&mut self.state, attaching, target_id);
                events_so_far.push(GameEventKind::Attached { object: attaching, attached_to: target_id });
            }
            Target::Player(player) => {
                commands::attach_permanent_to_player(&mut self.state, attaching, player);
                events_so_far.push(GameEventKind::AttachedToPlayer { object: attaching, player });
            }
            // `candidates` only ever holds what legal_entry_attach_candidates
            // produces (Aura/Equipment/Fortification entry targets) - never
            // Target::Ability, so `chosen` can't be one either (checked
            // above via candidates.contains).
            Target::Ability(_) => unreachable!("token attach candidates are never abilities"),
        }
        match self.continue_create_token(&spec, &copy_snapshot, remaining_count, controller, stack_id, events_so_far) {
            std::ops::ControlFlow::Continue(mut events) => {
                events.push(GameEventKind::StackObjectResolved { stack_id });
                self.process_state = ProcessState::ReplacementCheck { remaining: events.into(), checked: Vec::new() };
            }
            // A further token still owed by this effect also hit 2+
            // candidates - continue_create_token already set
            // self.process_state to the next AwaitingTokenAttachChoice
            // pause itself, nothing more to do here.
            std::ops::ControlFlow::Break(_) => {}
        }
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
        let ScriptChoiceRequest::SelectTarget { filter, exclude_ids } = request else {
            return Err(ScriptChoiceError::WrongRequestKind);
        };
        // `phase5/mass-choice-effects`: Game.ChooseN's own repeat-ask
        // exclusion - see ScriptChoiceRequest::SelectTarget's own doc
        // comment for why this lives here rather than folded into
        // target_is_legal/TargetFilter.
        if let Target::Object(id) = chosen {
            if exclude_ids.contains(&id) {
                return Err(ScriptChoiceError::InvalidCandidate);
            }
        }
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
        // CR 202.3: a `Game.SelectTargets`/`Game.ChooseN` mid-resolution ask
        // carries only a bare `TargetFilter`, not a live X context - no
        // script builds a `mana_value` filter through this path (it's only
        // reachable via a card's own static `target_requirement()`), so
        // `None` is a real no-op here, not a narrowing.
        if !commands::target_is_legal(&self.state, &self.script_runtime, chosen, filter, *controller, *stack_id, false, None)
        {
            return Err(ScriptChoiceError::InvalidCandidate);
        }
        let ProcessState::AwaitingScriptChoice { thread, stack_id, controller, events_so_far, each_player, .. } =
            std::mem::replace(&mut self.process_state, ProcessState::PriorityWindow)
        else {
            unreachable!("checked above");
        };

        let result = self.script_runtime.resume_operation(thread, &mut self.state, controller, chosen);
        if let Err(ref e) = result {
            self.log_events(vec![GameEventKind::ScriptError {
                message: format!("operation script error on source {stack_id:?}: {e:?}"),
            }]);
        }
        let outcome = result.unwrap_or(OperationOutcome::Finished);
        self.process_state = self.settle_script_operation_outcome(outcome, stack_id, controller, events_so_far, each_player);
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
        let ProcessState::AwaitingScriptChoice { thread, stack_id, controller, events_so_far, each_player, .. } =
            std::mem::replace(&mut self.process_state, ProcessState::PriorityWindow)
        else {
            unreachable!("checked above");
        };

        let result = self.script_runtime.resume_operation_bool(thread, &mut self.state, controller, chosen);
        if let Err(ref e) = result {
            self.log_events(vec![GameEventKind::ScriptError {
                message: format!("operation script error on source {stack_id:?}: {e:?}"),
            }]);
        }
        let outcome = result.unwrap_or(OperationOutcome::Finished);
        self.process_state = self.settle_script_operation_outcome(outcome, stack_id, controller, events_so_far, each_player);
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
            ScriptedZoneMoveThen::StackResolution { stack_id },
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

    /// Answers a pending CR 702.35a Madness choice raised by `StepStatus::
    /// AwaitingMadnessChoice` with "no" — moves `card` from exile to
    /// graveyard via the ordinary propose/replacement-check pipeline (a
    /// ZoneChange no different from any other structural zone move, unlike
    /// `ReturnSelfWithCounter`'s own battlefield-entry mover), then folds
    /// this trigger's own `events_so_far` plus that proposal into the same
    /// `ReplacementCheck` tail every other synchronous-only resolution ends
    /// on. `Engine::submit_madness_cast` is the "yes" sibling.
    pub fn decline_madness_cast(&mut self) -> Result<(), MadnessChoiceError> {
        if !matches!(self.process_state, ProcessState::AwaitingMadnessChoice { .. }) {
            return Err(MadnessChoiceError::NotAwaitingChoice);
        }
        let ProcessState::AwaitingMadnessChoice { card, stack_id, mut events_so_far, .. } =
            std::mem::replace(&mut self.process_state, ProcessState::PriorityWindow)
        else {
            unreachable!("checked above");
        };
        let Some(proposed) = commands::propose_zone_move(&self.state, card, ZoneKind::Exile, ZoneKind::Graveyard) else {
            // Defensive - `card` was confirmed present when this pause was
            // raised, and nothing between then and now can move it again
            // (the pause blocks every ordinary action). Same "nothing to
            // do" no-op stance every other pause here takes for an
            // unreachable-in-practice shape.
            events_so_far.push(GameEventKind::StackObjectResolved { stack_id });
            self.process_state = ProcessState::ReplacementCheck { remaining: events_so_far.into(), checked: Vec::new() };
            return Ok(());
        };
        match replacement::apply_replacements(&mut self.state, &self.script_runtime, proposed, false) {
            ReplacementOutcome::Resolved(event) => {
                let (committed, departed) = commands::commit_zone_move(&mut self.state, event);
                self.departed_triggered_abilities.extend(departed);
                self.state.ledger.push(committed);
                events_so_far.push(GameEventKind::StackObjectResolved { stack_id });
                self.process_state = ProcessState::ReplacementCheck { remaining: events_so_far.into(), checked: Vec::new() };
            }
            ReplacementOutcome::NeedsOrderingChoice { affected_player, event, candidates, excluded, .. } => {
                self.process_state = ProcessState::AwaitingReplacementChoice {
                    affected_player,
                    pending_event: event,
                    candidates,
                    excluded,
                    resume: ReplacementResumeTarget::MadnessDecline { card, stack_id, events_so_far },
                };
            }
        }
        Ok(())
    }

    /// Answers a pending CR 702.35a Madness choice raised by `StepStatus::
    /// AwaitingMadnessChoice` with "yes" — CR 702.35b: casting this way
    /// follows the ordinary alternative-cost rules (601.2b/601.2f-h), so
    /// this builds and runs a synthetic `PlayerActionKind::CastSpell`
    /// against `card`'s own madness `AlternativeCost::Replacement { from_zone:
    /// Exile, .. }` entry through the exact same `commands::execute_command`
    /// path `Engine::handle_pending_action` uses for an ordinary priority
    /// action - the resolving triggered ability's own "you may cast it" text
    /// is itself the CR 601.3 permission to do so outside `card`'s normal
    /// casting timing, granted one-shot via `GameState::pending_timing_bypass`
    /// (consumed by `commands::cast::execute_cast_spell`) rather than a
    /// property threaded through `PlayerActionKind::CastSpell` itself (see
    /// that field's own doc comment for why). On success, this trigger's own
    /// `StackObjectResolved { stack_id }` and the cast's own proposed events
    /// are folded together into one `ReplacementCheck` batch — CR 702.35a's
    /// casting happens *as* the ability resolves, not as a separate later
    /// priority action, so both belong in the same pipeline step. On
    /// failure (bad targets, unpayable cost, ...) the pause is left
    /// untouched, same "no-partial-payment" stance every other choice-answer
    /// in this file already takes — the caller may retry or call
    /// `decline_madness_cast` instead.
    pub fn submit_madness_cast(
        &mut self,
        modes: Vec<u32>,
        targets: Vec<Target>,
        x_value: Option<u32>,
    ) -> Result<(), MadnessChoiceError> {
        let ProcessState::AwaitingMadnessChoice { player, card, .. } = &self.process_state else {
            return Err(MadnessChoiceError::NotAwaitingChoice);
        };
        let (player, card) = (*player, *card);
        let alt_idx = self
            .state
            .object(card)
            .and_then(|o| {
                o.definition
                    .alternative_costs
                    .iter()
                    .position(|ac| matches!(ac, AlternativeCost::Replacement { from_zone: ZoneKind::Exile, .. }))
            })
            .ok_or(MadnessChoiceError::NoMadnessCost)?;

        self.state.pending_timing_bypass = Some(card);
        let command_id = self.allocate_command_id();
        let since_ledger = self.state.ledger.len();
        let command = EngineCommand {
            command_id,
            actor: player,
            action: PlayerActionKind::CastSpell {
                card,
                modes,
                targets,
                x_value,
                alternative_cost: Some(alt_idx),
                pay_buyback: false,
            },
        };
        let outcome = match commands::execute_command(&mut self.state, &self.script_runtime, command) {
            Ok(outcome) => outcome,
            Err(err) => {
                self.state.pending_timing_bypass = None;
                return Err(MadnessChoiceError::IllegalCast(err));
            }
        };

        let ProcessState::AwaitingMadnessChoice { stack_id, mut events_so_far, .. } =
            std::mem::replace(&mut self.process_state, ProcessState::PriorityWindow)
        else {
            unreachable!("checked above");
        };
        events_so_far.push(GameEventKind::StackObjectResolved { stack_id });
        events_so_far.extend(outcome.events);

        self.pipeline_context =
            Some(PipelineContext { since_ledger, caused_by_command: Some(command_id), parent_event: None });
        self.pending_payment = outcome.requires_payment;
        if outcome.broke_pass_chain {
            self.state.consecutive_passes = 0;
        }
        self.process_state = ProcessState::ReplacementCheck { remaining: events_so_far.into(), checked: Vec::new() };
        Ok(())
    }

    /// Answers a pending CR 702.85a Cascade choice raised by `StepStatus::
    /// AwaitingCascadeChoice` with "no" (or "there was no candidate to
    /// cast") — puts every exiled card on the bottom of the library in a
    /// random order (`Engine::bottom_cards_in_random_order`), then folds
    /// this trigger's own `events_so_far` into the same `ReplacementCheck`
    /// tail every other synchronous-only resolution ends on.
    /// `Engine::submit_cascade_cast` is the "yes" sibling.
    pub fn decline_cascade_cast(&mut self) -> Result<(), CascadeChoiceError> {
        if !matches!(self.process_state, ProcessState::AwaitingCascadeChoice { .. }) {
            return Err(CascadeChoiceError::NotAwaitingChoice);
        }
        let ProcessState::AwaitingCascadeChoice { exiled, stack_id, mut events_so_far, .. } =
            std::mem::replace(&mut self.process_state, ProcessState::PriorityWindow)
        else {
            unreachable!("checked above");
        };
        self.bottom_cards_in_random_order(exiled);
        events_so_far.push(GameEventKind::StackObjectResolved { stack_id });
        self.process_state = ProcessState::ReplacementCheck { remaining: events_so_far.into(), checked: Vec::new() };
        Ok(())
    }

    /// Answers a pending CR 702.85a Cascade choice raised by `StepStatus::
    /// AwaitingCascadeChoice` with "yes" — casts `candidate` paying no mana
    /// cost at all (`GameState::pending_free_cast`, consumed by `commands::
    /// cast::execute_cast_spell`), through the same synthetic-`CastSpell`-
    /// via-`commands::execute_command` shape `submit_madness_cast` already
    /// established (including the timing waiver, since this cast also
    /// happens *as* the trigger resolves, not as a later priority action).
    /// Every *other* exiled card (everything except `candidate`) goes to the
    /// bottom of the library in a random order — same as `decline_cascade_
    /// cast`, just excluding whichever card was actually cast. Errors
    /// (`NoCandidate`, or `IllegalCast` if the cast itself is illegal) leave
    /// the pause untouched, same no-partial-commitment stance
    /// `submit_madness_cast` already takes.
    pub fn submit_cascade_cast(
        &mut self,
        modes: Vec<u32>,
        targets: Vec<Target>,
        x_value: Option<u32>,
    ) -> Result<(), CascadeChoiceError> {
        let ProcessState::AwaitingCascadeChoice { player, candidate, .. } = &self.process_state else {
            return Err(CascadeChoiceError::NotAwaitingChoice);
        };
        let (player, candidate) = (*player, *candidate);
        let Some(candidate) = candidate else {
            return Err(CascadeChoiceError::NoCandidate);
        };

        self.state.pending_free_cast = Some(candidate);
        self.state.pending_timing_bypass = Some(candidate);
        let command_id = self.allocate_command_id();
        let since_ledger = self.state.ledger.len();
        let command = EngineCommand {
            command_id,
            actor: player,
            action: PlayerActionKind::CastSpell {
                card: candidate,
                modes,
                targets,
                x_value,
                alternative_cost: None,
                pay_buyback: false,
            },
        };
        let outcome = match commands::execute_command(&mut self.state, &self.script_runtime, command) {
            Ok(outcome) => outcome,
            Err(err) => {
                self.state.pending_free_cast = None;
                self.state.pending_timing_bypass = None;
                return Err(CascadeChoiceError::IllegalCast(err));
            }
        };

        let ProcessState::AwaitingCascadeChoice { exiled, stack_id, mut events_so_far, .. } =
            std::mem::replace(&mut self.process_state, ProcessState::PriorityWindow)
        else {
            unreachable!("checked above");
        };
        let leftover: Vec<ObjectId> = exiled.into_iter().filter(|&id| id != candidate).collect();
        self.bottom_cards_in_random_order(leftover);

        events_so_far.push(GameEventKind::StackObjectResolved { stack_id });
        events_so_far.extend(outcome.events);

        self.pipeline_context =
            Some(PipelineContext { since_ledger, caused_by_command: Some(command_id), parent_event: None });
        self.pending_payment = outcome.requires_payment;
        if outcome.broke_pass_chain {
            self.state.consecutive_passes = 0;
        }
        self.process_state = ProcessState::ReplacementCheck { remaining: events_so_far.into(), checked: Vec::new() };
        Ok(())
    }

    /// Answers a pending CR 701.6a/118.12 counter-unless-paid choice raised
    /// by `StepStatus::AwaitingCounterUnlessPaidChoice` with "yes, I pay" —
    /// validates `plan` against the pending mana-only `cost` (`CostSpec::
    /// mana_only`, same shape `PaymentPlan::validate` already checks
    /// elsewhere) and, if it covers the cost, commits it immediately via
    /// `commands::commit_payment` - the same mana-deduction primitive
    /// `submit_payment` itself uses, just without that method's own
    /// cast/activation-shaped pause wrapped around it (CR 601.6 never
    /// applies here - declining doesn't unwind anything, see `Engine::
    /// decline_counter_unless_paid`'s own doc comment). The targeted spell/
    /// ability survives; only this `CounterUnlessPaid` ability/spell's own
    /// resolution finishes (`StackObjectResolved`). On an invalid plan, the
    /// pause is left untouched (`PaymentError::InvalidPlan`'s own "no
    /// partial payment" stance) so the caller can retry or call
    /// `decline_counter_unless_paid` instead.
    pub fn submit_counter_unless_paid_payment(&mut self, plan: PaymentPlan) -> Result<(), CounterUnlessPaidError> {
        let ProcessState::AwaitingCounterUnlessPaidChoice { payer, cost, stack_id, .. } = &self.process_state else {
            return Err(CounterUnlessPaidError::NotAwaitingChoice);
        };
        let payer = *payer;
        let cost_spec = CostSpec::mana_only(cost.clone());
        // `source` (for AdditionalCost::TapSelf) is meaningless for a
        // mana-only cost - stack_id is a harmless placeholder, same
        // convention PendingPaymentRequest::source's own doc comment
        // already documents for a spell's own cast (nothing in a mana-only
        // CostSpec ever reads it).
        if !plan.validate(&cost_spec, &self.state, &self.script_runtime, payer, *stack_id) {
            return Err(CounterUnlessPaidError::InvalidPlan);
        }
        let ProcessState::AwaitingCounterUnlessPaidChoice { stack_id, mut events_so_far, .. } =
            std::mem::replace(&mut self.process_state, ProcessState::PriorityWindow)
        else {
            unreachable!("checked above");
        };
        for kind in commands::commit_payment(&mut self.state, payer, &plan.payments) {
            self.log_events(vec![kind]);
        }
        events_so_far.push(GameEventKind::StackObjectResolved { stack_id });
        self.process_state = ProcessState::ReplacementCheck { remaining: events_so_far.into(), checked: Vec::new() };
        Ok(())
    }

    /// Answers a pending CR 701.6a/118.12 counter-unless-paid choice with
    /// "no" (or the payer simply never intends to pay) — `target_id` is
    /// actually countered, via the same `counter_stack_object` removal
    /// `resolve_counter_effect`'s own unconditional case uses. Not a CR
    /// 601.6 unwind (nothing about *this* `CounterUnlessPaid` spell/ability's
    /// own cast is undone) - only the targeted object leaves the stack.
    pub fn decline_counter_unless_paid(&mut self) -> Result<(), CounterUnlessPaidError> {
        if !matches!(self.process_state, ProcessState::AwaitingCounterUnlessPaidChoice { .. }) {
            return Err(CounterUnlessPaidError::NotAwaitingChoice);
        }
        let ProcessState::AwaitingCounterUnlessPaidChoice { target_id, stack_id, mut events_so_far, .. } =
            std::mem::replace(&mut self.process_state, ProcessState::PriorityWindow)
        else {
            unreachable!("checked above");
        };
        self.counter_stack_object(target_id, &mut events_so_far);
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
    ///
    /// CR 707.9e (chained copies, `phase5/chained-copy-effects`): the
    /// `run_card_script` call below re-registers whatever `copied`'s own
    /// definition declares — if that's itself a Clone/Altered-Ego-family
    /// creature, a fresh `EntersAsCopy` replacement effect now sits on
    /// `object`. Immediately re-checking `pending_copy_choice` (same
    /// synthetic-event shape the original raise site uses — it matches on
    /// the candidate event's `from`/`to` fields, not `object`'s live zone,
    /// so this works even though `object` already sits on the battlefield)
    /// detects that chain. If found, this round's own additional-effect
    /// exceptions (`enters_with_additional_counters`/
    /// `grant_recurring_copy_trigger` — CR 707.9e's own phrasing, "an
    /// exception that's an additional effect," covers both alike, not just
    /// counters) are *deferred* rather than dropped: `AwaitingCopyChoice`'s
    /// own `carry_forward_if_declined` carries them into the next pause. If
    /// that next choice picks a real target, a second copy effect really
    /// was applied, so this round's deferred exceptions are conclusively
    /// superseded and simply never read again (CR 707.9e's own "the
    /// exception's effect doesn't happen"). If instead it's declined, no
    /// second copy effect was ever actually applied, so the deferred
    /// exceptions are applied after all — see the `chosen.is_none()` arm
    /// below. No new `ProcessState`/`StepStatus` variant is needed for the
    /// re-pause itself, since `Engine::mod.rs`'s own step-status derivation
    /// already reads whatever's in `ProcessState::AwaitingCopyChoice`
    /// generically.
    pub fn submit_copy_choice(&mut self, chosen: Option<ObjectId>) -> Result<(), CopyChoiceError> {
        let ProcessState::AwaitingCopyChoice { candidates, .. } = &self.process_state else {
            return Err(CopyChoiceError::NotAwaitingChoice);
        };
        if chosen.is_some_and(|id| !candidates.contains(&id)) {
            return Err(CopyChoiceError::InvalidCandidate);
        }
        let ProcessState::AwaitingCopyChoice {
            object,
            controller,
            exceptions,
            stack_id,
            mut events_so_far,
            colors_spent,
            carry_forward_if_declined,
            x_value,
            ..
        } = std::mem::replace(&mut self.process_state, ProcessState::PriorityWindow)
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
                    // CR 707.9e/g: a real, distinct copy application - see
                    // GameObject::copy_generation's own doc comment.
                    obj.copy_generation += 1;
                }
                commands::purge_granted_abilities(&mut self.state, object);
                let result = self.script_runtime.run_card_script(&mut self.state, &snapshot, object);
                self.record_script_error(object, &snapshot.script_ref, result);
                events_so_far.push(GameEventKind::BecameCopy { object, copied });

                // CR 707.9e: a second copy effect was just applied to
                // `object` - `carry_forward_if_declined` from any *earlier*
                // round (if this itself was answering a chained pause) is
                // conclusively superseded now and simply dropped here
                // (never read again).
                if let Some((chained_exceptions, chained_candidates)) =
                    replacement::pending_copy_choice(&self.state, &self.script_runtime, object, ZoneKind::Stack)
                {
                    // Chain into a fresh pause instead of applying this
                    // round's own additional-effect exceptions yet - see
                    // this function's own doc comment.
                    self.process_state = ProcessState::AwaitingCopyChoice {
                        object,
                        controller,
                        exceptions: chained_exceptions,
                        candidates: chained_candidates,
                        stack_id,
                        events_so_far,
                        colors_spent,
                        carry_forward_if_declined: Some(exceptions),
                        // `object`'s own X never changes across a chain -
                        // see AwaitingCopyChoice::x_value's own doc comment.
                        x_value,
                    };
                    return Ok(());
                }

                apply_copy_additional_effect(&mut self.state, object, &exceptions, x_value, &mut events_so_far);
            }
            // else: the chosen creature no longer resolves (departed
            // between the pause and this answer) - skip rather than guess,
            // same stance every other unresolvable-reference case in this
            // codebase already takes; `object` simply stays itself.
        } else if let Some(deferred) = carry_forward_if_declined {
            // CR 707.9e: declining this pause means no second copy effect
            // was ever actually *applied* (only offered) - the earlier
            // round's own exception was never superseded, so it applies
            // after all.
            apply_copy_additional_effect(&mut self.state, object, &deferred, x_value, &mut events_so_far);
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
