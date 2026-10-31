//! Answers to the mid-resolution pauses raised by `ResolveStackTop`: attach,
//! script, devour, amplify, madness, cascade, modal and copy choices. See `DESIGN.md`.

use std::collections::{HashSet, VecDeque};

use crate::commands;
use crate::engine_context::PipelineContext;
use crate::replacement::{self, ReplacementOutcome};
use crate::script::{OperationOutcome, ScriptChoiceRequest};
use crate::types::ability::{AbilityEffect, CopyExceptions, UnlessPaidConsequence};
use crate::types::command::{EngineCommand, PlayerActionKind};
use crate::types::cost::{AlternativeCost, PaymentPlan};
use crate::types::counter::CounterKind;
use crate::types::event::GameEventKind;
use crate::types::game::GameState;
use crate::types::ids::{ObjectId, PlayerId, Target};
use crate::types::target::{TargetFilter, TargetRequirement, TargetSpec};
use crate::types::zone::ZoneKind;

use super::{
    AmplifyChoiceError, AttachChoiceError, CascadeChoiceError, CopyChoiceError, DevourChoiceError,
    DiscardChoiceError, Engine, MadnessChoiceError, ModeChoiceError, ProcessState, ReplacementResumeTarget,
    ScriptChoiceError, ScriptedZoneMoveKind, ScriptedZoneMoveThen, UnlessPaidError,
};

/// CR 707.9a: applies the copy's additional-effect exceptions. Shared by the
/// no-chain path and the chained-pause-declined path, where this round's copy
/// was never superseded (CR 707.9e).
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
    /// Answers an attach pause (CR 303.4d). `chosen` must be a candidate. It
    /// resumes `PriorityWindow` because no gameplay reaches this pause yet.
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

    /// Answers a token entry-attach pause (CR 303.4f). `chosen` must be a candidate.
    /// It resumes `continue_create_token` for the tokens still owed. This pause is
    /// reached mid-resolution, so finishing the loop here must push
    /// `StackObjectResolved` itself, like `submit_amplify_choice`.
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
            // Candidates never include an ability.
            Target::Ability(_) => unreachable!("token attach candidates are never abilities"),
        }
        match self.continue_create_token(&spec, &copy_snapshot, remaining_count, controller, stack_id, events_so_far) {
            std::ops::ControlFlow::Continue(mut events) => {
                events.push(GameEventKind::StackObjectResolved { stack_id });
                self.process_state = ProcessState::ReplacementCheck { remaining: events.into(), checked: Vec::new() };
            }
            // A further token hit its own pause, which `continue_create_token` already set.
            std::ops::ControlFlow::Break(_) => {}
        }
        Ok(())
    }

    /// Answers a `Game.SelectTargets` pause. `chosen` must pass `target_is_legal`
    /// for the request's filter. Resumes the suspended script coroutine with it.
    // TODO: a script error on resume is dropped; no path from `ResolveStackTop` back to a `StepStatus` error
    pub fn submit_script_choice(&mut self, chosen: Target) -> Result<(), ScriptChoiceError> {
        let ProcessState::AwaitingScriptChoice { request, controller, source, .. } = &self.process_state else {
            return Err(ScriptChoiceError::NotAwaitingChoice);
        };
        let ScriptChoiceRequest::SelectTarget { filter, exclude_ids } = request else {
            return Err(ScriptChoiceError::WrongRequestKind);
        };
        // `Game.ChooseN` excludes earlier picks; see `ScriptChoiceRequest::SelectTarget`.
        if let Target::Object(id) = chosen
            && exclude_ids.contains(&id) {
                return Err(ScriptChoiceError::InvalidCandidate);
            }
        // CR 702.16c: protection checks the resolving object's source, not the controller
        // or the stack id.
        // TODO: `SelectTarget` carries a bare filter, so "another target" can't be expressed (CR 115.4)
        // CR 202.3: no script builds a `mana_value` filter here, so `None` loses nothing.
        if !commands::target_is_legal(&self.state, &self.script_runtime, chosen, filter, *controller, *source, false, None)
        {
            return Err(ScriptChoiceError::InvalidCandidate);
        }
        let ProcessState::AwaitingScriptChoice { thread, stack_id, source, controller, events_so_far, each_player, .. } =
            std::mem::replace(&mut self.process_state, ProcessState::PriorityWindow)
        else {
            unreachable!("checked above");
        };

        let result = self.script_runtime.resume_operation(thread, &mut self.state, source, controller, chosen);
        if let Err(ref e) = result {
            self.log_events(vec![GameEventKind::ScriptError {
                message: format!("operation script error on source {stack_id:?}: {e:?}"),
            }]);
        }
        let outcome = result.unwrap_or(OperationOutcome::Finished);
        self.process_state =
            self.settle_script_operation_outcome(outcome, stack_id, source, controller, events_so_far, each_player);
        Ok(())
    }

    /// Answers a `Game.MayChoose` pause (CR 608.2d). Any `bool` is legal, so no
    /// legality check. A separate entry point from `submit_script_choice`, one per pause shape.
    pub fn submit_script_may_choice(&mut self, chosen: bool) -> Result<(), ScriptChoiceError> {
        let ProcessState::AwaitingScriptChoice { request, .. } = &self.process_state else {
            return Err(ScriptChoiceError::NotAwaitingChoice);
        };
        let ScriptChoiceRequest::MayChoose = request else {
            return Err(ScriptChoiceError::WrongRequestKind);
        };
        let ProcessState::AwaitingScriptChoice { thread, stack_id, source, controller, events_so_far, each_player, .. } =
            std::mem::replace(&mut self.process_state, ProcessState::PriorityWindow)
        else {
            unreachable!("checked above");
        };

        let result = self.script_runtime.resume_operation_bool(thread, &mut self.state, source, controller, chosen);
        if let Err(ref e) = result {
            self.log_events(vec![GameEventKind::ScriptError {
                message: format!("operation script error on source {stack_id:?}: {e:?}"),
            }]);
        }
        let outcome = result.unwrap_or(OperationOutcome::Finished);
        self.process_state =
            self.settle_script_operation_outcome(outcome, stack_id, source, controller, events_so_far, each_player);
        Ok(())
    }

    /// Answers a devour pause (CR 702.82a). `sacrificed` is any subset of the
    /// candidates, empty included. The sacrifices run through
    /// `continue_scripted_zone_move`, which receives the counter event as `trailing`
    /// so everything reaches `ReplacementCheck` together.
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
        // CR 614.13: consumption bookkeeping, since this payload never passes through `apply_enters_battlefield_modifiers`.
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

    /// Answers an amplify pause (CR 702.38a). `revealed` is any subset of the
    /// candidates, empty included. Revealing moves nothing, so the counter event
    /// joins the usual resolved tail directly.
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
        // CR 614.13: consumption bookkeeping, since this payload never passes through `apply_enters_battlefield_modifiers`.
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

    /// Declines a madness cast (CR 702.35a): exiles to graveyard through the
    /// ordinary zone-move pipeline. `submit_madness_cast` is the "yes" sibling.
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
            // Defensive: nothing can move `card` while the pause is open.
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

    /// Accepts a madness cast (CR 702.35a). Runs a synthetic `CastSpell` of the
    /// card's exile alternative cost through `execute_command` (CR 702.35b). The
    /// one-shot `pending_timing_bypass` waives timing, since the cast happens as the
    /// trigger resolves (CR 601.3). The resolved marker and the cast's events share
    /// one `ReplacementCheck` batch. A failed cast leaves the pause open.
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
                self.next_command_id = command_id.0;
                return Err(match err {
                    commands::CommandError::Rejected(rejected) => MadnessChoiceError::IllegalCast(rejected),
                    commands::CommandError::Internal(internal) => {
                        self.poison(internal);
                        MadnessChoiceError::Internal
                    }
                });
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
        self.set_pending_payment(outcome.requires_payment);
        if outcome.broke_pass_chain {
            self.state.consecutive_passes = 0;
        }
        self.process_state = ProcessState::ReplacementCheck { remaining: events_so_far.into(), checked: Vec::new() };
        Ok(())
    }

    /// Declines a cascade cast (CR 702.85a): every exiled card goes to the bottom
    /// of the library in random order. `submit_cascade_cast` is the "yes" sibling.
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

    /// Accepts a cascade cast (CR 702.85a). Casts `candidate` for free via
    /// `pending_free_cast`, like `submit_madness_cast`, and bottoms the rest in random
    /// order. An illegal cast leaves the pause open.
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
                self.next_command_id = command_id.0;
                return Err(match err {
                    commands::CommandError::Rejected(rejected) => CascadeChoiceError::IllegalCast(rejected),
                    commands::CommandError::Internal(internal) => {
                        self.poison(internal);
                        CascadeChoiceError::Internal
                    }
                });
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
        self.set_pending_payment(outcome.requires_payment);
        if outcome.broke_pass_chain {
            self.state.consecutive_passes = 0;
        }
        self.process_state = ProcessState::ReplacementCheck { remaining: events_so_far.into(), checked: Vec::new() };
        Ok(())
    }

    /// Answers an "unless paid" pause with a payment (CR 118.12). Only the front of
    /// `payer_queue` may answer. A plan that covers the cost is committed; an
    /// invalid plan leaves the pause open. `otherwise` never fires.
    pub fn submit_unless_paid_payment(&mut self, payer: PlayerId, plan: PaymentPlan) -> Result<(), UnlessPaidError> {
        let ProcessState::AwaitingUnlessPaidChoice { payer_queue, cost, stack_id, .. } = &self.process_state else {
            return Err(UnlessPaidError::NotAwaitingChoice);
        };
        if payer_queue.front() != Some(&payer) {
            return Err(UnlessPaidError::NotCurrentPayer);
        }
        let cost = cost.clone();
        let stack_id = *stack_id;
        // `source` is a placeholder; both real producers have mana-only costs.
        if !plan.validate(&cost, &self.state, &self.script_runtime, payer, stack_id) {
            return Err(UnlessPaidError::InvalidPlan);
        }
        let ProcessState::AwaitingUnlessPaidChoice { mut events_so_far, .. } =
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

    /// Declines an "unless paid" pause (CR 118.12). Pops the front of `payer_queue`
    /// and re-raises for the next payer, or dispatches `otherwise` when none remain.
    /// Neither branch is a CR 601.6 unwind. The stack object was popped before the
    /// pause, so `SacrificeSelf` calls `continue_scripted_zone_move` with the saved id.
    pub fn decline_unless_paid(&mut self, payer: PlayerId) -> Result<(), UnlessPaidError> {
        let ProcessState::AwaitingUnlessPaidChoice { payer_queue, .. } = &self.process_state else {
            return Err(UnlessPaidError::NotAwaitingChoice);
        };
        if payer_queue.front() != Some(&payer) {
            return Err(UnlessPaidError::NotCurrentPayer);
        }
        let ProcessState::AwaitingUnlessPaidChoice {
            mut payer_queue,
            cost,
            otherwise,
            target_id,
            source,
            stack_id,
            mut events_so_far,
        } = std::mem::replace(&mut self.process_state, ProcessState::PriorityWindow)
        else {
            unreachable!("checked above");
        };
        payer_queue.pop_front();
        if !payer_queue.is_empty() {
            self.process_state = ProcessState::AwaitingUnlessPaidChoice {
                payer_queue,
                cost,
                otherwise,
                target_id,
                source,
                stack_id,
                events_so_far,
            };
            return Ok(());
        }
        match otherwise {
            UnlessPaidConsequence::Counter => {
                if let Some(target_id) = target_id {
                    self.counter_stack_object(target_id, &mut events_so_far);
                }
                events_so_far.push(GameEventKind::StackObjectResolved { stack_id });
                self.process_state =
                    ProcessState::ReplacementCheck { remaining: events_so_far.into(), checked: Vec::new() };
            }
            UnlessPaidConsequence::SacrificeSelf => {
                self.process_state = self.continue_scripted_zone_move(
                    ScriptedZoneMoveKind::Sacrifice,
                    VecDeque::from([source]),
                    ScriptedZoneMoveThen::StackResolution { stack_id },
                    events_so_far,
                );
            }
        }
        Ok(())
    }

    /// Answers a targeted-discard pause (CR 701.9). `player` is the targeted player,
    /// not necessarily the controller. Delegates to `continue_targeted_discard`.
    pub fn submit_discard_choice(&mut self, player: PlayerId, chosen: ObjectId) -> Result<(), DiscardChoiceError> {
        let ProcessState::AwaitingDiscardChoice { player: awaiting, candidates, .. } = &self.process_state else {
            return Err(DiscardChoiceError::NotAwaitingChoice);
        };
        if player != *awaiting {
            return Err(DiscardChoiceError::NotCurrentPlayer);
        }
        if !candidates.contains(&chosen) {
            return Err(DiscardChoiceError::InvalidCandidate);
        }
        let ProcessState::AwaitingDiscardChoice { player, remaining_count, stack_id, events_so_far, .. } =
            std::mem::replace(&mut self.process_state, ProcessState::PriorityWindow)
        else {
            unreachable!("checked above");
        };
        self.process_state = self.continue_targeted_discard(chosen, player, remaining_count, stack_id, events_so_far);
        Ok(())
    }

    /// Answers an opponent-chosen modal pause (CR 700.2e). `modes` must respect the
    /// announced range with no duplicates. Patches the stack object in place and
    /// clears `pending_mode_choice` so the next `step()` resolves it. Modes are
    /// targetless and costless by cast-time narrowing, so no target validation.
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
            if !chosen_modes.iter().all(|mode| commands::mode_can_join_others(mode)) {
                return Err(ModeChoiceError::UnsupportedModeCombination);
            }
            // Every mode is targetless, so each gets a zero-count placeholder.
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
            let words = vec![1; effects.len()];
            (Some(TargetSpec::Multiple(reqs)), Some(AbilityEffect::Modal { effects, words }))
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

    /// Answers an enter-as-a-copy pause (CR 707.9, 614.12a). `chosen` is `None` to
    /// decline or a candidate. A copy reuses `BecomeCopy`'s machinery and re-runs
    /// the card script on the new snapshot. Either way the pause then applies the
    /// enters-the-battlefield modifiers with the carried-through `colors_spent`.
    ///
    /// CR 707.9e: if the copied definition is itself a clone, a fresh pause is
    /// raised and this round's additional-effect exceptions are deferred in
    /// `carry_forward_if_declined`. A copy at the next pause supersedes them; a
    /// decline applies them after all.
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
        // CR 614.13: consumption bookkeeping, since this payload never passes through `apply_enters_battlefield_modifiers`.
        let candidate_event =
            GameEventKind::ZoneChange { object, from: ZoneKind::Stack, to: ZoneKind::Battlefield, last_known_info: None };
        replacement::apply_one_replacement(&mut self.state, object, candidate_event);
        if let Some(copied) = chosen {
            // CR 707.9c: captured before anything mutates, for `retain_name` and `retain_color`.
            let original_definition = self.state.object(object).map(|o| o.definition.clone());
            let snapshot = self.state.object(copied).map(|obj| {
                obj.copiable_override.clone().unwrap_or_else(|| obj.definition.clone())
            });
            if let Some(mut snapshot) = snapshot {
                exceptions.apply(&mut snapshot, original_definition.as_ref());
                if let Some(obj) = self.state.object_mut(object) {
                    obj.copiable_override = Some(snapshot.clone());
                    // CR 707.9e
                    obj.copy_generation += 1;
                }
                commands::purge_granted_abilities(&mut self.state, object);
                let result = self.script_runtime.run_card_script(&mut self.state, &snapshot, object);
                self.record_script_error(object, &snapshot.script_ref, result);
                events_so_far.push(GameEventKind::BecameCopy { object, copied });

                // CR 707.9e: a second copy supersedes any earlier round's deferred exception.
                if let Some((chained_exceptions, chained_candidates)) =
                    replacement::pending_copy_choice(&self.state, &self.script_runtime, object, ZoneKind::Stack)
                {
                    // Chain into a fresh pause; this round's exceptions are deferred.
                    self.process_state = ProcessState::AwaitingCopyChoice {
                        object,
                        controller,
                        exceptions: chained_exceptions,
                        candidates: chained_candidates,
                        stack_id,
                        events_so_far,
                        colors_spent,
                        carry_forward_if_declined: Some(exceptions),
                        // `object`'s X never changes across a chain.
                        x_value,
                    };
                    return Ok(());
                }

                apply_copy_additional_effect(&mut self.state, object, &exceptions, x_value, &mut events_so_far);
            }
            // The chosen creature is gone; `object` stays itself.
        } else if let Some(deferred) = carry_forward_if_declined {
            // CR 707.9e: no second copy was applied, so the deferred exception applies after all.
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
