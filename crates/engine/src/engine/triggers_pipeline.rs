//! CR 603.3b trigger-ordering answers, the `CollectTriggers`/
//! `PutTriggersApnap` pipeline stages, and the shared pipeline-run
//! termination helper `end_pipeline_run` — split out of `engine.rs`.

use std::collections::VecDeque;

use crate::commands;
use crate::engine_context::PipelineExit;
use crate::triggers::{self, PendingTrigger};
use crate::types::ability::AbilityEffect;
use crate::types::card::ModeChoice;
use crate::types::event::{GameEvent, GameEventKind};
use crate::types::ids::{ObjectId, PlayerId, Target};
use crate::types::stack::{StackEntryKind, StackObject};
use crate::types::target::TargetSpec;

use super::{Engine, ProcessState, StepStatus, TriggerModeChoiceError, TriggerOrderError, TriggerTargetsError};

/// Whether `chosen` is a reordering of `group` — same `PendingTrigger`s
/// (by value), each used exactly once, any order. Used by
/// `submit_trigger_order` to reject a same-length answer that isn't
/// actually a permutation (swapped-in trigger, duplicate, etc).
///
/// `pub(super)`: also exercised directly by `tests.rs`'s own unit tests
/// (`use super::triggers_pipeline::is_permutation`), same as before this
/// file existed as its own module.
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

/// Whether `a` and `b` together contain exactly `whole`'s ids, each exactly
/// once (any split, any order) — used by `submit_scry_choice`/
/// `submit_surveil_choice` to validate the two answered piles against the
/// pending `cards` set. Same "manual multiset comparison" shape
/// `is_permutation` above already uses, kept separate since this checks two
/// lists against one reference set rather than one list being a reordering
/// of another.
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
    /// Answers a pending CR 603.3b intra-group ordering choice raised by
    /// `StepStatus::AwaitingTriggerOrderChoice` — `chosen_order` must be a
    /// reordering of that status's `group` (same triggers, player's chosen
    /// sequence). Separate entry point from `submit_action` for the same
    /// reason `submit_replacement_choice` is: this is a sub-decision
    /// mid-pipeline, not a `PlayerAction`/priority decision.
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
        let ProcessState::AwaitTriggerOrderChoice { mut ordered, pending_groups, .. } =
            std::mem::replace(&mut self.process_state, ProcessState::PriorityWindow)
        else {
            unreachable!("checked above");
        };
        ordered.extend(chosen_order);
        self.process_state = ProcessState::PutTriggersApnap { ordered, pending_groups };
        Ok(())
    }

    /// Pipeline run's final transition: consumes `self.pipeline_exit`,
    /// clears `self.pipeline_context` (the MUST documented on that field —
    /// the run that set it is finishing right here, in this call), and
    /// returns the `ProcessState` the engine should sit in next. Pulled out
    /// as its own function rather than left inline inside
    /// `PutTriggersApnap` so there's exactly one place that performs this
    /// transition, however many call sites eventually need it.
    ///
    /// `pub(in crate::engine)`: `payment.rs`'s `unwind_payment` also calls
    /// this.
    pub(in crate::engine) fn end_pipeline_run(&mut self) -> ProcessState {
        self.pipeline_context = None;
        self.departed_triggered_abilities.clear();
        match std::mem::replace(&mut self.pipeline_exit, PipelineExit::BackToPriority) {
            PipelineExit::BackToPriority => ProcessState::PriorityWindow,
            PipelineExit::ResolveStackTop => ProcessState::ResolveStackTop,
            PipelineExit::AdvanceTurnStep => ProcessState::AdvanceTurnStep,
            PipelineExit::CleanupCheck { since_ledger } => {
                // Ledger growth catches an SBA event or a trigger placed
                // during *this* pass; `!stack.is_empty()` catches a trigger
                // placed earlier in the same cleanup sequence (e.g.
                // Engine::submit_discard's own DiscardsCard trigger,
                // reached via PipelineExit::CleanupSbaCheck just before
                // this SBA pass ran) that this pass's own since_ledger
                // marker can't see, since it was captured after that
                // trigger already landed. Either one means CR 514.3 says
                // open a priority window; only both being false means
                // nothing at all is waiting and the turn advances silently.
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

    /// `ProcessState::CollectTriggers { since }` — scans `state.ledger[since..]`
    /// for matching triggered abilities (`triggers::collect_triggers`),
    /// records CR 603.2h once-per-turn matches, logs any script errors
    /// surfaced along the way, then groups the result APNAP order
    /// (`triggers::group_apnap`) ready for `PutTriggersApnap`.
    pub(super) fn resolve_collect_triggers_state(&mut self, since: usize) -> StepStatus {
        let mut pending = Vec::new();
        let mut script_errors = Vec::new();
        for event in &self.state.ledger[since..] {
            let (matches, errors) = triggers::collect_triggers(
                &self.state,
                event,
                &self.script_runtime,
                &self.departed_triggered_abilities,
            );
            // CR 603.2h: record a once-per-turn match now, before
            // the next event in this same batch is checked -
            // collect_triggers's own cap check reads this same set,
            // so a second qualifying event later in this batch (not
            // just a later turn) is correctly excluded too.
            for trigger in &matches {
                if trigger.once_per_turn {
                    self.state.once_per_turn_triggered.insert(trigger.ability);
                }
            }
            pending.extend(matches);
            script_errors.extend(errors);
        }
        // Logged once the loop above (which holds an immutable
        // borrow of self.state.ledger) has fully released it -
        // log_events needs &mut self (`phase5/script-error-
        // surfacing`).
        if !script_errors.is_empty() {
            self.log_events(
                script_errors.into_iter().map(|message| GameEventKind::ScriptError { message }).collect(),
            );
        }
        let groups = triggers::group_apnap(&self.state, pending);
        self.process_state =
            ProcessState::PutTriggersApnap { ordered: Vec::new(), pending_groups: groups.into() };
        StepStatus::Progressed
    }

    /// `ProcessState::PutTriggersApnap { ordered, pending_groups }` — walks
    /// `pending_groups` (already in APNAP order) one group per `step()`
    /// call, pausing on `AwaitTriggerOrderChoice` (CR 603.3b) for any group
    /// with more than one trigger; once the queue empties, pushes every
    /// ordered trigger onto `state.stack` as a real `StackObject` (CR
    /// 603.3b/603.4) and hands off to `end_pipeline_run`.
    pub(super) fn resolve_put_triggers_apnap_state(
        &mut self,
        mut ordered: Vec<PendingTrigger>,
        mut pending_groups: VecDeque<(PlayerId, Vec<PendingTrigger>)>,
    ) -> StepStatus {
        // Walks pending_groups (already in APNAP order, per
        // triggers::group_apnap) one group per step() call rather
        // than ordering+pushing them all in one shot - a group with
        // more than one trigger needs its controller to choose the
        // intra-group order (CR 603.3b), which has to pause the
        // whole run on AwaitTriggerOrderChoice, not get resolved
        // without them.
        let Some((player, group)) = pending_groups.pop_front() else {
            // Push every trigger in `ordered` onto self.state.stack as a
            // real StackObject (CR 603.3b/603.4) - see
            // push_ordered_triggers's own doc comment for why this can no
            // longer be a plain unpaused for-loop.
            return self.push_ordered_triggers(ordered.into());
        };
        if group.len() <= 1 {
            ordered.extend(group);
            self.process_state = ProcessState::PutTriggersApnap { ordered, pending_groups };
        } else {
            self.process_state =
                ProcessState::AwaitTriggerOrderChoice { player, group, ordered, pending_groups };
        }
        StepStatus::Progressed
    }

    /// Pushes every trigger in `remaining` onto `state.stack` as a real
    /// `StackObject`, one at a time - pausing on `ProcessState::
    /// AwaitingTriggerTargets` (CR 603.3d) the moment one needs a real
    /// target choice (single- or multi-word, any `TargetSpec`) its
    /// controller hasn't made yet, instead of the old unconditional
    /// `targets: Vec::new()`. Shared by
    /// `resolve_put_triggers_apnap_state`'s own initial call (the whole
    /// freshly-ordered queue) and `submit_trigger_targets`'s resume
    /// (whatever's left after the just-answered trigger) - same "the queue
    /// survives the pause, one shared driver walks it either way" shape
    /// `AwaitTriggerOrderChoice`'s own `pending_groups` already established
    /// one level up.
    pub(super) fn push_ordered_triggers(&mut self, mut remaining: VecDeque<PendingTrigger>) -> StepStatus {
        while let Some(trigger) = remaining.pop_front() {
            // CR 603.10: a Dies/LeavesBattlefield trigger's source never
            // resolves live in state.abilities (retire_object already
            // purged it) - fall back to the same departed_triggered_abilities
            // stash collect_triggers itself consulted to produce this
            // PendingTrigger in the first place, matching on the same
            // stable id.
            let ability = commands::find_ability_by_id(&self.state, trigger.ability)
                .or_else(|| self.departed_triggered_abilities.iter().find(|a| a.id == trigger.ability));
            let Some(ability) = ability else {
                // Neither state.abilities nor the departed stash resolved
                // this trigger - TODO(CR 603.3d): sacrifice/discard-as-cost/
                // other zone departures aren't wired to the departed stash
                // yet (see retire_object's own doc comment), so a
                // dies-trigger from one of those still lands here and gets
                // silently dropped.
                continue;
            };
            // CR 603.3c: a modal triggered ability's controller announces
            // the mode choice before its target choice - 603.3c's own mode
            // choice happens before 603.3d's (mirrors 601.2b before 601.2c
            // for a cast spell) - so this is checked, and paused on, before
            // `ability.target` is even consulted below. A modal
            // `RegisteredAbility`'s own `target`/`effect` fields are
            // meaningless (the real per-mode data lives in `modes`), same
            // "empty/None means doesn't apply" stance `modes` itself
            // documents.
            if !ability.modes.is_empty() {
                let modes = ability.modes.clone();
                let mode_choice = ability.mode_choice;
                // CR 700.2e: "a player other than [the ability's] controller
                // chooses" - real for a trigger's own modes now too, mirroring
                // CastSpell's own ChosenByOpponent branch exactly: every mode
                // must be targetless and costless (nothing here defers cost
                // payment the way a spell's own resolution-time pause could),
                // and the game must have exactly one other non-eliminated
                // player (CR 700.2e's own 3+-player "controller decides who
                // chooses" sub-clause isn't supported, same narrowing
                // CastSpell's branch already carries). Either precondition
                // failing means this ability can never validly go on the
                // stack at all - there's no "reject the cast" step for a
                // trigger to fall back on, so it's simply skipped, same CR
                // 603.3d "removed from stack" stance a trigger with no legal
                // target already takes below.
                let chooser = match mode_choice {
                    ModeChoice::ExactlyChosenByOpponent(_) | ModeChoice::UpToChosenByOpponent(_) => {
                        let modes_ok =
                            modes.iter().all(|m| m.target_requirement.is_none() && m.additional_cost.is_none());
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
                self.process_state =
                    ProcessState::AwaitingTriggerModeChoice { trigger, modes, mode_choice, remaining, chooser };
                return StepStatus::Progressed;
            }
            // CR 603.3d/601.2c: a triggered ability that names a target (or
            // targets - CR 601.2c's multi-word form works here exactly like
            // it does for a spell/activated ability) must have it/them
            // chosen before it goes on the stack at all - unlike
            // CastSpell/ActivateAbility, nothing submitted `targets` up
            // front, since a triggered ability isn't initiated by a player
            // action in the first place.
            if let Some(spec) = &ability.target {
                let spec = spec.clone();
                let candidates =
                    commands::legal_target_candidates_for_spec(&self.state, &self.script_runtime, &spec, trigger.controller, trigger.source);
                let minimums = commands::target_spec_minimums(&spec);
                let satisfiable = candidates.iter().zip(&minimums).all(|(c, &min)| c.len() as u32 >= min);
                if !satisfiable {
                    // CR 603.3d: "if a choice is required ... but no legal
                    // choices can be made for it ... the ability is simply
                    // removed from the stack" - never pushed at all,
                    // silently move on to the next trigger in this batch.
                    // Same all-or-nothing stance CR 601.2c/e already takes
                    // for a spell with 2+ target words: one word coming up
                    // empty voids the whole cast/trigger, not just that word.
                    continue;
                }
                self.process_state =
                    ProcessState::AwaitingTriggerTargets { trigger, spec, candidates, remaining, modal_effect: None };
                return StepStatus::Progressed;
            }
            self.push_trigger_stack_object(&trigger, Vec::new(), None);
        }
        self.process_state = self.end_pipeline_run();
        StepStatus::Progressed
    }

    /// The actual `StackObject`/`StackObjectPushed` construction one
    /// trigger's own resolved `RegisteredAbility` needs - pulled out of
    /// `push_ordered_triggers` so both its no-target-needed fast path and
    /// `submit_trigger_targets`'s resume (after a real choice was made) call
    /// exactly one shared implementation. `ability`'s own fields are
    /// re-looked-up here (rather than threaded through as a parameter) so
    /// the borrow of `self.state`/`self.departed_triggered_abilities` ends
    /// before this function's own `&mut self.state` mutations - the same
    /// "extract owned copies, then mutate" shape the original inline loop
    /// this was split from already used.
    ///
    /// `modal_override`: `None` means "use `ability.target`/`ability.effect`
    /// as-is" (the pre-existing, non-modal shape - both call sites below
    /// pass this). `Some((target_requirement, effect))` is CR 603.3c's own
    /// case - `Engine::submit_trigger_mode_choice` already combined the
    /// chosen mode(s) via `commands::combine_chosen_modes`, and a modal
    /// ability's own `target`/`effect` fields are meaningless placeholders
    /// (see `RegisteredAbility::modes`'s own doc comment), so they must
    /// never be read here in that case.
    fn push_trigger_stack_object(
        &mut self,
        trigger: &PendingTrigger,
        targets: Vec<Target>,
        modal_override: Option<(Option<TargetSpec>, Option<AbilityEffect>)>,
    ) {
        let ability = commands::find_ability_by_id(&self.state, trigger.ability)
            .or_else(|| self.departed_triggered_abilities.iter().find(|a| a.id == trigger.ability));
        let Some(ability) = ability else { return };
        let operation = ability.operation;
        let (target_requirement, effect) = match modal_override {
            Some(pair) => pair,
            None => (ability.target.clone(), ability.effect.clone()),
        };
        let intervening_if = ability.intervening_if.clone();
        let stack_id = self.state.allocate_object_id();
        // CR 800.4a: the source's own current owner when it's still live;
        // `PendingTrigger` has no last-known-owner of its own for the
        // departed-source case (a Dies/LeavesBattlefield trigger), so that
        // narrow case falls back to `controller` - same proxy this field
        // replaces everywhere else, now scoped to just this one gap.
        let source_owner = self.state.object(trigger.source).map(|o| o.owner).unwrap_or(trigger.controller);
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
            // CR 702.27a: buyback is a spell-only concept - a triggered
            // ability never carries it.
            false,
            // CR 702.138b: "escaped" is a spell-cast-shaped concept - a
            // triggered ability never carries it either.
            false,
            operation,
            effect,
            intervening_if,
            // CR 700.2e: only a modal spell can defer its mode choice this
            // way - a triggered ability never does.
            None,
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
    }

    /// Answers a pending CR 603.3d target choice raised by
    /// `StepStatus::AwaitingTriggerTargets` - `chosen` must satisfy `spec` as
    /// a whole (validated via `commands::validate_targets`, the exact same
    /// count/dedup/legality/per-word-splitting checks `CastSpell`/
    /// `ActivateAbility` already enforce - one flat list, split by
    /// `split_targets_by_requirement` under the hood for a `Multiple` spec).
    /// Separate entry point from `submit_action` for the same reason
    /// `submit_trigger_order`/`submit_copy_choice` are: this is a
    /// sub-decision mid-pipeline, not a `PlayerAction`/priority decision.
    pub fn submit_trigger_targets(&mut self, chosen: Vec<Target>) -> Result<(), TriggerTargetsError> {
        let ProcessState::AwaitingTriggerTargets { trigger, spec, .. } = &self.process_state else {
            return Err(TriggerTargetsError::NotAwaitingChoice);
        };
        if commands::validate_targets(&self.state, &self.script_runtime, Some(spec), &chosen, trigger.controller, trigger.source)
            .is_err()
        {
            return Err(TriggerTargetsError::InvalidTargets);
        }
        let ProcessState::AwaitingTriggerTargets { trigger, spec, remaining, modal_effect, .. } =
            std::mem::replace(&mut self.process_state, ProcessState::PriorityWindow)
        else {
            unreachable!("checked above");
        };
        // CR 603.3c: `modal_effect` is `Some` only when this pause was
        // reached via `submit_trigger_mode_choice` (a modal trigger whose
        // chosen mode(s) needed a target) - `spec` itself is already the
        // combined `TargetSpec` in that case, same as the plain non-modal
        // path, so it's always the real override for `target_requirement`;
        // only `effect` differs by path (see `push_trigger_stack_object`'s
        // own doc comment).
        let modal_override = modal_effect.map(|effect| (Some(spec), effect));
        self.push_trigger_stack_object(&trigger, chosen, modal_override);
        // Defers the rest of `remaining` to the next step() call, same
        // "submit_* mutates now, advance() drives the actual continuation
        // later" convention every other choice-pause resume in this engine
        // already follows - reuses PutTriggersApnap's own empty-
        // pending_groups branch (`resolve_put_triggers_apnap_state` calls
        // `push_ordered_triggers` the moment `pending_groups` is empty)
        // rather than a third call site for the same loop.
        self.process_state = ProcessState::PutTriggersApnap { ordered: remaining.into(), pending_groups: VecDeque::new() };
        Ok(())
    }

    /// Answers a pending CR 603.3c modal triggered-ability mode choice
    /// raised by `StepStatus::AwaitingTriggerModeChoice` - `chosen` must
    /// satisfy the offered `mode_choice`'s count/duplicate rules (same
    /// `ModeChoice::bounds` rules `CastSpell`'s own caster-chosen branch
    /// enforces), each index valid into the ability's own `modes`. Combines
    /// the chosen mode(s) via `commands::combine_chosen_modes` (shared with
    /// `CastSpell`'s own caster-chosen branch - see that function's own doc
    /// comment), then either chains straight into the *existing*
    /// `AwaitingTriggerTargets` pause (CR 603.3c's own mode choice precedes
    /// 603.3d's target choice) if the combined mode(s) need a target, or
    /// pushes the ability immediately if not. If the combined target
    /// requirement turns out to have no legal candidates, the ability is
    /// removed from the stack (CR 603.3d), same all-or-nothing stance
    /// `push_ordered_triggers`'s own plain target check already takes.
    pub fn submit_trigger_mode_choice(&mut self, chosen: Vec<u32>) -> Result<(), TriggerModeChoiceError> {
        let ProcessState::AwaitingTriggerModeChoice { modes, mode_choice, .. } = &self.process_state else {
            return Err(TriggerModeChoiceError::NotAwaitingChoice);
        };
        if let ModeChoice::PawprintBudget(budget) = mode_choice {
            // CR 700.2i: weight-sum constraint, not a count bound - see
            // `ModeChoice::PawprintBudget`'s own doc comment and
            // `commands::cast::execute_command`'s identical `CastSpell`
            // branch (the two aren't shared since one validates against a
            // `Vec<SpellMode>` reference already borrowed from
            // `ProcessState`, the other from a freshly-looked-up
            // `object.definition.modes`).
            let mut seen = std::collections::HashSet::new();
            if !chosen.iter().all(|m| seen.insert(*m)) {
                return Err(TriggerModeChoiceError::DuplicateMode);
            }
            let mut total_weight: u32 = 0;
            for &idx in &chosen {
                let mode = modes.get(idx as usize).ok_or(TriggerModeChoiceError::NoSuchMode)?;
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
        // CR 700.2's own convention (per Wizards' rulings on e.g. Cryptic
        // Command): chosen modes' effects happen in the order printed on
        // the card/ability, not the order the player listed them in.
        let mut chosen_indices = chosen;
        chosen_indices.sort_unstable();
        if chosen_indices.iter().any(|&idx| modes.get(idx as usize).is_none()) {
            return Err(TriggerModeChoiceError::NoSuchMode);
        }
        let (target_requirement, effect) = commands::combine_chosen_modes(modes, &chosen_indices)
            .map_err(|_| TriggerModeChoiceError::UnsupportedModeCombination)?;

        let ProcessState::AwaitingTriggerModeChoice { trigger, remaining, .. } =
            std::mem::replace(&mut self.process_state, ProcessState::PriorityWindow)
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
            let satisfiable = candidates.iter().zip(&minimums).all(|(c, &min)| c.len() as u32 >= min);
            if satisfiable {
                self.process_state = ProcessState::AwaitingTriggerTargets {
                    trigger,
                    spec,
                    candidates,
                    remaining,
                    modal_effect: Some(effect),
                };
            } else {
                // CR 603.3d: no legal choice for the chosen mode(s)' own
                // target(s) - the ability is simply removed from the stack,
                // never pushed at all.
                self.process_state =
                    ProcessState::PutTriggersApnap { ordered: remaining.into(), pending_groups: VecDeque::new() };
            }
            return Ok(());
        }

        // Every chosen mode is targetless - push immediately, same
        // no-target fast path `push_ordered_triggers` itself takes.
        self.push_trigger_stack_object(&trigger, Vec::new(), Some((None, effect)));
        self.process_state = ProcessState::PutTriggersApnap { ordered: remaining.into(), pending_groups: VecDeque::new() };
        Ok(())
    }
}
