//! Trigger-ordering answers, the `CollectTriggers` and `PutTriggersApnap`
//! stages, and `end_pipeline_run`. See `DESIGN.md`.

use std::collections::VecDeque;

use crate::commands;
use crate::engine_context::PipelineExit;
use crate::triggers::{self, PendingTrigger};
use crate::types::ability::AbilityEffect;
use crate::types::card::ModeChoice;
use crate::types::event::{GameEvent, GameEventKind};
use crate::types::ids::{ObjectId, PlayerId, Target};
use crate::types::stack::{StackEntryKind, StackObject};
use crate::types::target::{TargetFilter, TargetSpec};

use super::{
    Engine, ProcessState, StepStatus, TriggerModeChoiceError, TriggerOrderError,
    TriggerTargetsError,
};

/// Whether `chosen` is a reordering of `group`: the same triggers, each used
/// once. Exposed to `tests.rs`.
pub(super) fn is_permutation(chosen: &[PendingTrigger], group: &[PendingTrigger]) -> bool {
    if chosen.len() != group.len() {
        return false;
    }
    let mut remaining: Vec<PendingTrigger> = group.to_vec();
    for item in chosen {
        match remaining.iter().position(|candidate| candidate == item) {
            Some(idx) => {
                remaining.remove(idx);
            }
            None => return false,
        }
    }
    true
}

/// Whether `a` and `b` together hold exactly the ids of `whole`, each once.
pub(super) fn is_exact_partition(a: &[ObjectId], b: &[ObjectId], whole: &[ObjectId]) -> bool {
    if a.len() + b.len() != whole.len() {
        return false;
    }
    let mut remaining: Vec<ObjectId> = whole.to_vec();
    for &id in a.iter().chain(b.iter()) {
        match remaining.iter().position(|&candidate| candidate == id) {
            Some(idx) => {
                remaining.remove(idx);
            }
            None => return false,
        }
    }
    true
}

impl Engine {
    /// Answers a trigger ordering pause (CR 603.3b). `chosen_order` must be a
    /// reordering of the offered `group`. A separate entry point from
    /// `submit_action` because it answers a sub-decision.
    pub fn submit_trigger_order(
        &mut self,
        chosen_order: Vec<PendingTrigger>,
    ) -> Result<(), TriggerOrderError> {
        let ProcessState::AwaitTriggerOrderChoice { group, .. } = &self.process_state else {
            return Err(TriggerOrderError::NotAwaitingChoice);
        };
        if chosen_order.len() != group.len() {
            return Err(TriggerOrderError::WrongLength);
        }
        if !is_permutation(&chosen_order, group) {
            return Err(TriggerOrderError::NotAPermutation);
        }
        let ProcessState::AwaitTriggerOrderChoice {
            mut ordered,
            pending_groups,
            ..
        } = std::mem::replace(&mut self.process_state, ProcessState::PriorityWindow)
        else {
            unreachable!("checked above");
        };
        ordered.extend(chosen_order);
        self.process_state = ProcessState::PutTriggersApnap {
            ordered,
            pending_groups,
        };
        Ok(())
    }

    /// Consumes `pipeline_exit`, clears `pipeline_context` and returns the next
    /// state, so one place performs the run's final transition.
    pub(in crate::engine) fn end_pipeline_run(&mut self) -> ProcessState {
        self.pipeline_context = None;
        self.departed_triggered_abilities.clear();
        match std::mem::replace(&mut self.pipeline_exit, PipelineExit::BackToPriority) {
            // CR 605.3a: a nested mana ability resumes the payment it interrupted
            // instead of opening a priority window.
            PipelineExit::BackToPriority => {
                self.resume_suspended_payment_or(ProcessState::PriorityWindow)
            }
            PipelineExit::ResolveStackTop => ProcessState::ResolveStackTop,
            PipelineExit::AdvanceTurnStep => ProcessState::AdvanceTurnStep,
            PipelineExit::CleanupCheck { since_ledger } => {
                // Ledger growth catches an event from this pass. The stack check catches
                // a trigger placed earlier in cleanup, before `since_ledger` was taken.
                // Either one opens priority (CR 514.3a).
                if self.state.ledger.len() > since_ledger || !self.state.stack.is_empty() {
                    self.state.priority_player = self.state.active_player;
                    ProcessState::PriorityWindow
                } else {
                    ProcessState::AdvanceTurnStep
                }
            }
            PipelineExit::CleanupSbaCheck => ProcessState::CleanupSbaCheck,
        }
    }

    /// Scans the ledger from `since` for triggers, records once-per-turn matches
    /// (CR 603.2h), logs script errors and groups the result in APNAP order.
    pub(super) fn resolve_collect_triggers_state(&mut self, since: usize) -> StepStatus {
        self.sync_off_battlefield_scripts();
        let mut pending = Vec::new();
        let mut script_errors = Vec::new();
        for event in &self.state.ledger[since..] {
            let (matches, errors) = triggers::collect_triggers(
                &self.state,
                event,
                &self.script_runtime,
                &self.departed_triggered_abilities,
            );
            // CR 603.2h: recorded before the next event in the batch is checked so a
            // second qualifying event this turn is excluded too.
            for trigger in &matches {
                if trigger.once_per_turn {
                    self.state.once_per_turn_triggered.insert(trigger.ability);
                }
            }
            pending.extend(matches);
            script_errors.extend(errors);
        }
        // Logged after the loop releases its borrow of the ledger.
        if !script_errors.is_empty() {
            self.log_events(
                script_errors
                    .into_iter()
                    .map(|message| GameEventKind::ScriptError { message })
                    .collect(),
            );
        }
        let groups = triggers::group_apnap(&self.state, pending);
        self.process_state = ProcessState::PutTriggersApnap {
            ordered: Vec::new(),
            pending_groups: groups.into(),
        };
        StepStatus::Progressed
    }

    /// Walks the APNAP groups one per `step()`, pausing for the order of any group
    /// with several triggers (CR 603.3b), then pushes the ordered triggers on the stack.
    pub(super) fn resolve_put_triggers_apnap_state(
        &mut self,
        mut ordered: Vec<PendingTrigger>,
        mut pending_groups: VecDeque<(PlayerId, Vec<PendingTrigger>)>,
    ) -> StepStatus {
        // One group per `step()` so an ordering choice can pause the whole run.
        let Some((player, group)) = pending_groups.pop_front() else {
            return self.push_ordered_triggers(ordered.into());
        };
        if group.len() <= 1 {
            ordered.extend(group);
            self.process_state = ProcessState::PutTriggersApnap {
                ordered,
                pending_groups,
            };
        } else {
            self.process_state = ProcessState::AwaitTriggerOrderChoice {
                player,
                group,
                ordered,
                pending_groups,
            };
        }
        StepStatus::Progressed
    }

    /// Pushes each trigger onto the stack, pausing on `AwaitingTriggerTargets` for
    /// the first that needs a target choice (CR 603.3d). Shared by the initial push
    /// and the resume after a target answer, so the queue survives the pause.
    pub(super) fn push_ordered_triggers(
        &mut self,
        mut remaining: VecDeque<PendingTrigger>,
    ) -> StepStatus {
        while let Some(trigger) = remaining.pop_front() {
            // CR 603.10: a dies or leaves trigger's source is gone from `abilities`, so
            // fall back to the departed stash.
            let ability =
                commands::find_ability_by_id(&self.state, trigger.ability).or_else(|| {
                    self.departed_triggered_abilities
                        .iter()
                        .find(|a| a.id == trigger.ability)
                });
            let Some(ability) = ability else {
                // TODO: sacrifice, discard-as-cost and other departures don't feed the departed stash, so their dies-triggers are dropped here (CR 603.3d)
                continue;
            };
            // CR 603.3c: the mode choice comes before the target choice. A modal
            // ability's own `target` and `effect` are placeholders; the data is in `modes`.
            if !ability.modes.is_empty() {
                let modes = ability.modes.clone();
                let mode_choice = ability.mode_choice;
                // CR 700.2e: mirrors the `CastSpell` chooser rule. Every mode must be
                // targetless and costless and exactly one other player must remain; else the
                // trigger is skipped, since a trigger can't be rejected like a cast.
                // TODO: three or more players, where the controller picks the chooser (CR 700.2e)
                let chooser = match mode_choice {
                    ModeChoice::ExactlyChosenByOpponent(_)
                    | ModeChoice::UpToChosenByOpponent(_) => {
                        let modes_ok = modes
                            .iter()
                            .all(|m| m.target_requirement.is_none() && m.additional_cost.is_none());
                        let others: Vec<PlayerId> = commands::non_eliminated_players(&self.state)
                            .into_iter()
                            .filter(|&p| p != trigger.controller)
                            .collect();
                        match (modes_ok, others.as_slice()) {
                            (true, [only]) => *only,
                            _ => continue,
                        }
                    }
                    _ => trigger.controller,
                };
                self.process_state = ProcessState::AwaitingTriggerModeChoice {
                    trigger,
                    modes,
                    mode_choice,
                    remaining,
                    chooser,
                };
                return StepStatus::Progressed;
            }
            // CR 603.3d: targets are chosen before the trigger goes on the stack
            // because no player action supplied them.
            // CR 702.21a: Ward targets the spell or ability that caused it, so it is
            // filled in here. If that object left the stack the trigger is removed.
            if let Some(TargetSpec::Primitive(req)) = &ability.target
                && req.filter == TargetFilter::TriggeringStackObject
            {
                let target = trigger.triggering_stack.and_then(|id| {
                    let so = self.state.stack.iter().find(|so| so.stack_id == id)?;
                    Some(if commands::is_ability_stack_entry(&so.kind) {
                        Target::Ability(id)
                    } else {
                        Target::Object(id)
                    })
                });
                if let Some(target) = target {
                    self.push_trigger_stack_object(&trigger, vec![target], None);
                }
                continue;
            }
            if let Some(spec) = &ability.target {
                let spec = spec.clone();
                let candidates = commands::legal_target_candidates_for_spec(
                    &self.state,
                    &self.script_runtime,
                    &spec,
                    trigger.controller,
                    trigger.source,
                );
                let minimums = commands::target_spec_minimums(&spec);
                let satisfiable = candidates
                    .iter()
                    .zip(&minimums)
                    .all(|(c, &min)| c.len() as u32 >= min);
                if !satisfiable {
                    // CR 603.3d: no legal choice removes the ability. One empty target
                    // word voids the whole trigger (CR 601.2c).
                    continue;
                }
                self.process_state = ProcessState::AwaitingTriggerTargets {
                    trigger,
                    spec,
                    candidates,
                    remaining,
                    modal_effect: None,
                };
                return StepStatus::Progressed;
            }
            self.push_trigger_stack_object(&trigger, Vec::new(), None);
        }
        self.process_state = match self.recollect_triggers_since.take() {
            Some(since) => ProcessState::CollectTriggers { since },
            None => self.end_pipeline_run(),
        };
        StepStatus::Progressed
    }

    /// Builds the `StackObject` for one resolved trigger. `ability` is looked up
    /// again so the borrow ends before the mutations. `modal_override` is
    /// `Some((target_requirement, effect))` for a modal trigger (CR 603.3c),
    /// whose own `target` and `effect` fields are placeholders.
    fn push_trigger_stack_object(
        &mut self,
        trigger: &PendingTrigger,
        targets: Vec<Target>,
        modal_override: Option<(Option<TargetSpec>, Option<AbilityEffect>)>,
    ) {
        let ability = commands::find_ability_by_id(&self.state, trigger.ability).or_else(|| {
            self.departed_triggered_abilities
                .iter()
                .find(|a| a.id == trigger.ability)
        });
        let Some(ability) = ability else { return };
        let operation = ability.operation;
        let (target_requirement, effect) = match modal_override {
            Some(pair) => pair,
            None => (ability.target.clone(), ability.effect.clone()),
        };
        let intervening_if = ability.intervening_if.clone();
        let stack_id = self.state.allocate_object_id();
        // CR 800.4a: a departed source has no live owner, so fall back to `controller`.
        let source_is_live = self.state.object(trigger.source).is_some();
        let source_owner = self
            .state
            .object(trigger.source)
            .map(|o| o.owner)
            .unwrap_or(trigger.controller);
        // CR 603.4: only a departed source with an intervening-if needs the ledger scan.
        let source_lki = if !source_is_live && intervening_if.is_some() {
            triggers::find_departed_lki(&self.state, trigger.source)
        } else {
            None
        };
        self.state.stack.push(StackObject::new(
            stack_id,
            StackEntryKind::TriggeredAbility,
            trigger.source,
            source_owner,
            trigger.controller,
            Vec::new(),
            targets,
            target_requirement,
            None,
            false,
            // CR 702.27a: a triggered ability has no buyback.
            false,
            // CR 702.138b: nor can it be escaped.
            false,
            operation,
            effect,
            intervening_if,
            // CR 700.2e: only a modal spell defers its mode choice.
            None,
            source_lki,
        ));
        let event_id = self.state.allocate_event_id();
        let sequence_no = self.state.allocate_sequence_no();
        self.state.ledger.push(GameEvent {
            event_id,
            sequence_no,
            kind: GameEventKind::StackObjectPushed { stack_id },
            caused_by_command: None,
            parent_event: None,
        });
        // CR 603.2: the targets become targeted here. Nothing scans these events
        // yet, so `push_ordered_triggers` re-enters `CollectTriggers` once its queue
        // empties, otherwise Ward would never see a triggered ability (CR 702.21a).
        let ledger_before = self.state.ledger.len();
        self.log_became_target(stack_id);
        if self.state.ledger.len() > ledger_before && self.recollect_triggers_since.is_none() {
            self.recollect_triggers_since = Some(ledger_before);
        }
    }

    /// Answers a trigger target pause (CR 603.3d). `chosen` is validated as a whole
    /// against the spec, like `CastSpell`. A separate entry point from
    /// `submit_action` because it answers a sub-decision.
    pub fn submit_trigger_targets(
        &mut self,
        chosen: Vec<Target>,
    ) -> Result<(), TriggerTargetsError> {
        let ProcessState::AwaitingTriggerTargets { trigger, spec, .. } = &self.process_state else {
            return Err(TriggerTargetsError::NotAwaitingChoice);
        };
        // CR 202.3: a triggered ability has no announced X, so an X bound is 0.
        if commands::validate_targets(
            &self.state,
            &self.script_runtime,
            Some(spec),
            &chosen,
            trigger.controller,
            trigger.source,
            None,
        )
        .is_err()
        {
            return Err(TriggerTargetsError::InvalidTargets);
        }
        let ProcessState::AwaitingTriggerTargets {
            trigger,
            spec,
            remaining,
            modal_effect,
            ..
        } = std::mem::replace(&mut self.process_state, ProcessState::PriorityWindow)
        else {
            unreachable!("checked above");
        };
        // CR 603.3c: `modal_effect` is set only when the pause came from a modal
        // trigger's mode choice.
        let modal_override = modal_effect.map(|effect| (Some(spec), effect));
        self.push_trigger_stack_object(&trigger, chosen, modal_override);
        // Defers `remaining` to the next `step()` through the empty-groups branch of
        // `PutTriggersApnap` instead of a third copy of the push loop.
        self.process_state = ProcessState::PutTriggersApnap {
            ordered: remaining.into(),
            pending_groups: VecDeque::new(),
        };
        Ok(())
    }

    /// Answers a modal trigger's mode choice (CR 603.3c). Validates `chosen`
    /// against the offered bounds, combines the modes, then chains into the target
    /// pause (CR 603.3d) or pushes the ability. No legal target removes it.
    pub fn submit_trigger_mode_choice(
        &mut self,
        chosen: Vec<u32>,
    ) -> Result<(), TriggerModeChoiceError> {
        let ProcessState::AwaitingTriggerModeChoice {
            modes, mode_choice, ..
        } = &self.process_state
        else {
            return Err(TriggerModeChoiceError::NotAwaitingChoice);
        };
        if let ModeChoice::PawprintBudget(budget) = mode_choice {
            // CR 700.2i: a weight-sum constraint rather than a count bound.
            let mut seen = std::collections::HashSet::new();
            if !chosen.iter().all(|m| seen.insert(*m)) {
                return Err(TriggerModeChoiceError::DuplicateMode);
            }
            let mut total_weight: u32 = 0;
            for &idx in &chosen {
                let mode = modes
                    .get(idx as usize)
                    .ok_or(TriggerModeChoiceError::NoSuchMode)?;
                total_weight += mode.pawprint_weight;
            }
            if total_weight > *budget {
                return Err(TriggerModeChoiceError::InvalidCount);
            }
        } else {
            let (min, max, repeatable) = mode_choice.bounds();
            let chosen_count = chosen.len() as u32;
            if chosen_count < min || chosen_count > max {
                return Err(TriggerModeChoiceError::InvalidCount);
            }
            if !repeatable {
                let mut seen = std::collections::HashSet::new();
                if !chosen.iter().all(|m| seen.insert(*m)) {
                    return Err(TriggerModeChoiceError::DuplicateMode);
                }
            }
        }
        // CR 700.2: modes happen in printed order, not the order chosen.
        let mut chosen_indices = chosen;
        chosen_indices.sort_unstable();
        if chosen_indices
            .iter()
            .any(|&idx| modes.get(idx as usize).is_none())
        {
            return Err(TriggerModeChoiceError::NoSuchMode);
        }
        let (target_requirement, effect) =
            commands::combine_chosen_modes(modes, &chosen_indices)
                .map_err(|_| TriggerModeChoiceError::UnsupportedModeCombination)?;

        let ProcessState::AwaitingTriggerModeChoice {
            trigger, remaining, ..
        } = std::mem::replace(&mut self.process_state, ProcessState::PriorityWindow)
        else {
            unreachable!("checked above");
        };

        if let Some(spec) = target_requirement {
            let candidates = commands::legal_target_candidates_for_spec(
                &self.state,
                &self.script_runtime,
                &spec,
                trigger.controller,
                trigger.source,
            );
            let minimums = commands::target_spec_minimums(&spec);
            let satisfiable = candidates
                .iter()
                .zip(&minimums)
                .all(|(c, &min)| c.len() as u32 >= min);
            if satisfiable {
                self.process_state = ProcessState::AwaitingTriggerTargets {
                    trigger,
                    spec,
                    candidates,
                    remaining,
                    modal_effect: Some(effect),
                };
            } else {
                // CR 603.3d
                self.process_state = ProcessState::PutTriggersApnap {
                    ordered: remaining.into(),
                    pending_groups: VecDeque::new(),
                };
            }
            return Ok(());
        }

        // Every chosen mode is targetless, so push immediately.
        self.push_trigger_stack_object(&trigger, Vec::new(), Some((None, effect)));
        self.process_state = ProcessState::PutTriggersApnap {
            ordered: remaining.into(),
            pending_groups: VecDeque::new(),
        };
        Ok(())
    }
}
