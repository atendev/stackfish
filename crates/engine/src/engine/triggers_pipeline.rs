//! CR 603.3b trigger-ordering answers, the `CollectTriggers`/
//! `PutTriggersApnap` pipeline stages, and the shared pipeline-run
//! termination helper `end_pipeline_run` — split out of `engine.rs`.

use std::collections::VecDeque;

use crate::commands;
use crate::engine_context::PipelineExit;
use crate::triggers::{self, PendingTrigger};
use crate::types::event::{GameEvent, GameEventKind};
use crate::types::ids::{ObjectId, PlayerId};
use crate::types::stack::{StackEntryKind, StackObject};

use super::{Engine, ProcessState, StepStatus, TriggerOrderError};

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
            // Push each trigger in `ordered` onto self.state.stack
            // as a real StackObject (CR 603.3b/603.4) - looked up by
            // its own stable AbilityId via commands::find_ability_by_id
            // (phase5/ability-stable-id; see AbilityId's own doc
            // comment for why a position was no longer safe here).
            for trigger in ordered {
                // CR 603.10: a Dies/LeavesBattlefield trigger's
                // source never resolves live in state.abilities
                // (retire_object already purged it) - fall back to
                // the same departed_triggered_abilities stash
                // collect_triggers itself consulted to produce this
                // PendingTrigger in the first place, matching on the
                // same stable id.
                let ability = commands::find_ability_by_id(&self.state, trigger.ability)
                    .or_else(|| {
                        self.departed_triggered_abilities.iter().find(|a| a.id == trigger.ability)
                    });
                let Some(ability) = ability else {
                    // Neither state.abilities nor the departed stash
                    // resolved this trigger - TODO(CR 603.3d):
                    // sacrifice/discard-as-cost/other zone
                    // departures aren't wired to the departed stash
                    // yet (see retire_object's own doc comment), so
                    // a dies-trigger from one of those still lands
                    // here and gets silently dropped.
                    continue;
                };
                let operation = ability.operation;
                let effect = ability.effect.clone();
                let target_requirement = ability.target.clone();
                let intervening_if = ability.intervening_if.clone();
                let stack_id = self.state.allocate_object_id();
                // TODO(CR 603.3c): triggered abilities that require
                // targets ("when this dies, destroy target
                // creature") aren't choosable yet - targets is
                // always empty here, same script-bridge-mid-
                // pipeline gap invoke_target/select_targets already
                // flag. target_requirement is still baked onto the
                // StackObject (in case a future ability sets a
                // fixed/no-choice target) so ResolveStackTop's CR
                // 608.2b recheck applies uniformly regardless of
                // StackEntryKind.
                // CR 800.4a: the source's own current owner when
                // it's still live; `PendingTrigger` has no
                // last-known-owner of its own for the departed-
                // source case (a Dies/LeavesBattlefield trigger),
                // so that narrow case falls back to `controller` -
                // same proxy this field replaces everywhere else,
                // now scoped to just this one gap.
                let source_owner =
                    self.state.object(trigger.source).map(|o| o.owner).unwrap_or(trigger.controller);
                self.state.stack.push(StackObject::new(
                    stack_id,
                    StackEntryKind::TriggeredAbility,
                    trigger.source,
                    source_owner,
                    trigger.controller,
                    Vec::new(),
                    Vec::new(),
                    target_requirement,
                    None,
                    false,
                    operation,
                    effect,
                    intervening_if,
                    // CR 700.2e: only a modal spell can defer its
                    // mode choice this way - a triggered ability
                    // never does.
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
            self.process_state = self.end_pipeline_run();
            return StepStatus::Progressed;
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
}
