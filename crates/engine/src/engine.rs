use std::ops::Range;

use crate::commands::{self, CommandError, PriorityHandoff};
use crate::engine_context::{PipelineContext, PipelineExit};
use crate::replacement::{self, ReplacementOutcome};
use crate::sba;
use crate::triggers::PendingTrigger;
use crate::types::command::{EngineCommand, PlayerAction};
use crate::types::event::{GameEvent, GameEventKind};
use crate::types::game::GameState;
use crate::types::ids::{CommandId, ObjectId, PlayerId};

/// Why `submit_action` rejected a `PlayerAction` outright (wrong player has
/// priority, action doesn't match what's currently legal, etc). Distinct
/// from in-progress command execution failures, which surface through
/// `CommandError` instead once the action is accepted and being carried out.
#[derive(Debug)]
pub enum ActionError {
    NotYourTurn,
    IllegalAction(String),
}

/// Why `submit_replacement_choice` rejected an answer to a CR 616.1
/// ordering choice.
#[derive(Debug)]
pub enum ReplacementChoiceError {
    /// Engine isn't sitting in `ProcessState::AwaitingReplacementChoice`.
    NotAwaitingChoice,
    /// `chosen` wasn't one of the offered `candidates`.
    InvalidCandidate,
}

/// Why `submit_trigger_order` rejected an answer to a CR 603.3b
/// intra-group ordering choice.
#[derive(Debug)]
pub enum TriggerOrderError {
    /// Engine isn't sitting in `ProcessState::AwaitTriggerOrderChoice`.
    NotAwaitingChoice,
    /// `chosen_order`'s length doesn't match the offered group's — caller
    /// must supply a reordering of the exact group, not a different set.
    WrongLength,
}

/// What happened on one `step()` call, and what the engine needs next.
pub enum StepStatus {
    /// Made progress; safe to call `step()` again immediately.
    Progressed,
    /// Needs a `PlayerAction` from `player` before it can continue — the
    /// caller should prompt and `submit_action`, then `step()` again.
    AwaitingAction { player: PlayerId },
    /// CR 616.1: more than one `ReplacementEffect` matched the same event
    /// mid-`ReplacementCheck` — the caller should prompt `player` to pick
    /// one of `candidates` (each a `ReplacementEffect::source`) and call
    /// `submit_replacement_choice`, then `step()` again. Distinct from
    /// `AwaitingAction`: this isn't a `PlayerAction`/priority decision, it's
    /// a sub-decision mid-pipeline that `submit_action` must keep rejecting
    /// while it's pending (see `submit_action`'s `PriorityWindow`-only
    /// check).
    AwaitingReplacementChoice { player: PlayerId, candidates: Vec<ObjectId> },
    /// CR 603.3b: `player` controls more than one trigger that matched the
    /// same batch of events and must choose the order to put them on the
    /// stack — the caller should prompt and call `submit_trigger_order`,
    /// then `step()` again.
    AwaitingTriggerOrderChoice { player: PlayerId, group: Vec<PendingTrigger> },
    /// A command failed mid-pipeline. The engine resets to `PriorityWindow`
    /// so it doesn't get stuck — it does NOT retry or roll back whatever
    /// partial mutation happened before the error, since `execute_command`
    /// doesn't yet guarantee atomicity. Surfacing that gap honestly rather
    /// than hiding it behind a panic.
    Error(CommandError),
    GameOver { winner: Option<PlayerId> },
}

/// Result of one `step()` call. `events_added` indexes into
/// `Engine::state().ledger` rather than cloning events out, since the
/// ledger already holds them — this just tells the caller which slice is
/// new since the call.
pub struct StepResult {
    pub events_added: Range<usize>,
    pub status: StepStatus,
}

/// The pipeline stage `Engine` is currently sitting in — the explicit
/// state machine behind `step()`. One `step()` call generally advances by
/// one variant; `SbaLoop` is the exception, re-entering itself across
/// multiple `step()` calls until CR 704.3's fixed point is reached, so a
/// replay viewer can observe each individual SBA pass rather than the loop
/// being opaque.
///
/// `COLLECT_ACTIONS` and `PRIORITY_WINDOW` (in the human-design version
/// this came from) are merged into `PriorityWindow` here: there's no
/// `Engine::legal_actions()` yet to do a real "collect" step against, so
/// there's nothing to separate it from "waiting for a submit_action" yet.
/// Revisit if/when legal-action enumeration becomes a real feature.
pub enum ProcessState {
    PriorityWindow,
    /// Accepted by `submit_action`, waiting for the next `step()` to
    /// execute it via `commands::execute_command`.
    PendingAction(PlayerAction),
    /// All players passed in a row with a non-empty stack (CR 117.4) —
    /// resolve the top `StackObject` next.
    ResolveStackTop,
    /// Proposed events waiting on replacement/prevention checks (CR
    /// 614-616) before they're allowed to commit. `checked` accumulates
    /// events already resolved by earlier iterations of this same
    /// `ReplacementCheck` run; `remaining` is what's still unprocessed.
    ReplacementCheck { remaining: Vec<GameEventKind>, checked: Vec<GameEventKind> },
    /// CR 616.1: paused mid-`ReplacementCheck` because `pending_event`
    /// matched more than one `ReplacementEffect` (`candidates`, identified
    /// by `source` `ObjectId`) and `affected_player` must choose which
    /// applies first. `checked`/`remaining` carry the rest of the same
    /// `ReplacementCheck` run so it can resume once
    /// `submit_replacement_choice` answers.
    AwaitingReplacementChoice {
        affected_player: PlayerId,
        pending_event: GameEventKind,
        candidates: Vec<ObjectId>,
        remaining: Vec<GameEventKind>,
        checked: Vec<GameEventKind>,
    },
    /// Checked events ready to become real `GameEvent`s in the ledger.
    CommitEvents { checked: Vec<GameEventKind> },
    /// CR 704.3's fixed-point loop.
    SbaLoop,
    /// Collecting triggered abilities that matched ledger events from
    /// index `since` onward.
    CollectTriggers { since: usize },
    /// Triggers collected and grouped by controller in APNAP order
    /// (`triggers::group_apnap`) — waiting to be pushed to the stack.
    /// `ordered` accumulates groups already finalized (each in that
    /// controller's chosen intra-group order); `pending_groups` is what's
    /// left, still in APNAP order.
    PutTriggersApnap {
        ordered: Vec<PendingTrigger>,
        pending_groups: Vec<(PlayerId, Vec<PendingTrigger>)>,
    },
    /// CR 603.3b: paused mid-`PutTriggersApnap` because `group` (all
    /// controlled by `player`) has more than one trigger and `player` must
    /// choose the order to put them on the stack. `ordered`/`pending_groups`
    /// carry the rest of the same `PutTriggersApnap` run so it can resume
    /// once `submit_trigger_order` answers.
    AwaitTriggerOrderChoice {
        player: PlayerId,
        group: Vec<PendingTrigger>,
        ordered: Vec<PendingTrigger>,
        pending_groups: Vec<(PlayerId, Vec<PendingTrigger>)>,
    },
    /// All players passed with an empty stack — advance turn/phase/step
    /// (CR 500-514) before priority reopens.
    AdvanceTurnStep,
}

/// The step-able rules processor (DESIGN.md's "step-able core"). Wraps
/// `GameState` (pure data) with the actual turn/phase/priority/stack
/// advancement logic. Live play drives `step()` in a tight loop between
/// player decisions; a replay viewer re-feeds the same seed + action stream
/// into a fresh `Engine` and calls `step()` one at a time to pause/scrub.
pub struct Engine {
    state: GameState,
    process_state: ProcessState,
    /// `None` while sitting in `PriorityWindow` with nothing in flight; set
    /// when a pipeline run begins (`PendingAction`/`ResolveStackTop`), read
    /// by `CommitEvents`/`CollectTriggers`, and MUST be reset back to
    /// `None` by `PutTriggersApnap` (the run's last stage) — otherwise the
    /// next run's `CommitEvents`/`CollectTriggers` would silently read this
    /// run's stale `since_ledger`/`caused_by_command`/`parent_event`.
    pipeline_context: Option<PipelineContext>,
    /// Decided when a pipeline run begins; consumed once that run's
    /// trigger-placement stage finishes to pick the next `ProcessState`.
    pipeline_exit: PipelineExit,
    /// Dedicated monotonic counter for `CommandId` allocation — see the
    /// same rationale as `GameState::allocate_event_id`; was previously
    /// derived from `ledger.len()`, which doesn't even make sense for
    /// command ids (commands aren't ledger entries).
    next_command_id: u64,
}

impl Engine {
    pub fn new(state: GameState) -> Self {
        Engine {
            state,
            process_state: ProcessState::PriorityWindow,
            pipeline_context: None,
            pipeline_exit: PipelineExit::BackToPriority,
            next_command_id: 0,
        }
    }

    pub fn state(&self) -> &GameState {
        &self.state
    }

    fn allocate_command_id(&mut self) -> CommandId {
        let id = CommandId(self.next_command_id);
        self.next_command_id += 1;
        id
    }

    /// Validates `action` is legal right now (right player, matches what's
    /// actually available) and queues it for the next `step()` to act on.
    /// Does not itself advance the ledger — `step()` does that.
    ///
    /// Only checks "are we even waiting for an action right now" and "is
    /// it this player's priority" (CR 117.3a). Cost/target/timing legality
    /// beyond that isn't designed yet — see `commands::execute_command`'s
    /// TODOs for where that has to land.
    pub fn submit_action(&mut self, action: PlayerAction) -> Result<(), ActionError> {
        match self.process_state {
            ProcessState::PriorityWindow => {}
            _ => {
                return Err(ActionError::IllegalAction(
                    "engine is mid-pipeline, not waiting for a player action".to_string(),
                ));
            }
        }
        if action.player != self.state.priority_player {
            return Err(ActionError::NotYourTurn);
        }
        self.process_state = ProcessState::PendingAction(action);
        Ok(())
    }

    /// Answers a pending CR 616.1 ordering choice raised by
    /// `StepStatus::AwaitingReplacementChoice` — `chosen` must be one of
    /// that status's `candidates` (a `ReplacementEffect::source`).
    /// Deliberately a separate entry point from `submit_action`: this isn't
    /// a `PlayerAction`/priority decision, it's a sub-decision mid-pipeline,
    /// so it can't go through `submit_action`'s `PriorityWindow`-only gate.
    pub fn submit_replacement_choice(
        &mut self,
        chosen: ObjectId,
    ) -> Result<(), ReplacementChoiceError> {
        let ProcessState::AwaitingReplacementChoice { candidates, .. } = &self.process_state
        else {
            return Err(ReplacementChoiceError::NotAwaitingChoice);
        };
        if !candidates.contains(&chosen) {
            return Err(ReplacementChoiceError::InvalidCandidate);
        }
        // TODO: apply the chosen ReplacementEffect's rewrite to
        // pending_event (needs ReplacementEffect's "apply" callback - same
        // gap noted in replacement.rs, doesn't exist yet), then re-run
        // apply_replacements on the result (CR 616's stabilization
        // requirement, since the rewritten form may match yet another
        // effect), pushing the eventual Resolved form onto `checked` before
        // transitioning back to `ProcessState::ReplacementCheck { remaining,
        // checked }` so the rest of this run's events still get processed.
        todo!()
    }

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
        // TODO: validate chosen_order is actually a permutation of `group`
        // (same multiset of PendingTrigger, just reordered) rather than
        // trusting the caller blindly.
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
    fn end_pipeline_run(&mut self) -> ProcessState {
        self.pipeline_context = None;
        match std::mem::replace(&mut self.pipeline_exit, PipelineExit::BackToPriority) {
            PipelineExit::BackToPriority => ProcessState::PriorityWindow,
            PipelineExit::ResolveStackTop => ProcessState::ResolveStackTop,
            PipelineExit::AdvanceTurnStep => ProcessState::AdvanceTurnStep,
        }
    }

    /// Advances by one pipeline stage (see `ProcessState`) and returns.
    /// Callers that don't care about pausing (live play) just call this in
    /// a loop until `AwaitingAction`/`GameOver`; a replay viewer calls it
    /// once per UI "step".
    pub fn step(&mut self) -> StepResult {
        let events_start = self.state.ledger.len();
        let status = self.advance();
        StepResult { events_added: events_start..self.state.ledger.len(), status }
    }

    fn advance(&mut self) -> StepStatus {
        // `PriorityWindow` has no payload, so it's a cheap placeholder to
        // swap in while we take ownership of whatever state we're actually
        // in — every other variant gets explicitly replaced below before
        // this function returns.
        match std::mem::replace(&mut self.process_state, ProcessState::PriorityWindow) {
            ProcessState::PriorityWindow => {
                StepStatus::AwaitingAction { player: self.state.priority_player }
            }

            ProcessState::PendingAction(action) => {
                let command_id = self.allocate_command_id();
                let since_ledger = self.state.ledger.len();
                let command =
                    EngineCommand { command_id, actor: action.player, action: action.kind };

                let outcome = match commands::execute_command(&mut self.state, command) {
                    Ok(outcome) => outcome,
                    Err(err) => {
                        // Reset to PriorityWindow rather than leaving process_state
                        // in limbo - see StepStatus::Error's doc comment for why
                        // this doesn't attempt rollback.
                        self.process_state = ProcessState::PriorityWindow;
                        return StepStatus::Error(err);
                    }
                };

                self.pipeline_context = Some(PipelineContext {
                    since_ledger,
                    caused_by_command: Some(command_id),
                    parent_event: None,
                });

                // engine.rs owns state.priority_player entirely now -
                // execute_command just reports intent (CommandOutcome::
                // priority_handoff, commands.rs) instead of writing the
                // field itself.
                if let PriorityHandoff::To(player) = outcome.priority_handoff {
                    self.state.priority_player = player;
                }

                // engine.rs owns state.consecutive_passes entirely now -
                // execute_command just reports whether its variant broke
                // the chain (CommandOutcome::broke_pass_chain, commands.rs)
                // instead of mutating the counter itself, so there's one
                // place deciding what "everyone just passed" means rather
                // than relying on every match arm in execute_command
                // remembering to reset it.
                if outcome.broke_pass_chain {
                    self.state.consecutive_passes = 0;
                    self.pipeline_exit = PipelineExit::BackToPriority;
                } else {
                    self.state.consecutive_passes += 1;
                    self.pipeline_exit = if self.state.consecutive_passes as usize
                        >= self.state.turn_order.len()
                    {
                        self.state.consecutive_passes = 0;
                        if !self.state.stack.is_empty() {
                            PipelineExit::ResolveStackTop
                        } else {
                            PipelineExit::AdvanceTurnStep
                        }
                    } else {
                        PipelineExit::BackToPriority
                    };
                }

                self.process_state =
                    ProcessState::ReplacementCheck { remaining: outcome.events, checked: Vec::new() };
                StepStatus::Progressed
            }

            ProcessState::ResolveStackTop => {
                // TODO: pop self.state.stack, look up the popped
                // StackObject's `operation` ScriptHandle, invoke it via
                // ScriptRuntime::invoke_operation - blocked on the script
                // bridge the same as every other script-shaped gap.
                // Should set self.pipeline_context (since_ledger = current
                // ledger length, caused_by_command = None since this isn't
                // triggered by a fresh player command, parent_event = None)
                // then transition to ProcessState::ReplacementCheck
                // { remaining: <produced events>, checked: Vec::new() },
                // same as PendingAction does above.
                todo!()
            }

            ProcessState::ReplacementCheck { mut remaining, mut checked } => {
                // Processes one event per step() call rather than mapping
                // the whole Vec in one shot — apply_replacements can return
                // NeedsOrderingChoice (CR 616.1) for any of them, and that
                // has to pause the whole pipeline run on a real
                // AwaitingReplacementChoice, not get silently dropped or
                // resolved without the player. `checked`/`remaining` carry
                // this run's progress across that pause.
                let Some(event) = remaining.pop() else {
                    self.process_state = ProcessState::CommitEvents { checked };
                    return StepStatus::Progressed;
                };
                match replacement::apply_replacements(&mut self.state, event) {
                    ReplacementOutcome::Resolved(event) => {
                        checked.push(event);
                        self.process_state = ProcessState::ReplacementCheck { remaining, checked };
                    }
                    ReplacementOutcome::NeedsOrderingChoice { affected_player, event, candidates } => {
                        self.process_state = ProcessState::AwaitingReplacementChoice {
                            affected_player,
                            pending_event: event,
                            candidates,
                            remaining,
                            checked,
                        };
                    }
                }
                StepStatus::Progressed
            }

            ProcessState::AwaitingReplacementChoice {
                affected_player,
                pending_event,
                candidates,
                remaining,
                checked,
            } => {
                // Nothing to do until submit_replacement_choice answers -
                // put the exact same state back and tell the caller who
                // needs to be prompted, mirroring PriorityWindow's
                // AwaitingAction pattern.
                let status = StepStatus::AwaitingReplacementChoice {
                    player: affected_player,
                    candidates: candidates.clone(),
                };
                self.process_state = ProcessState::AwaitingReplacementChoice {
                    affected_player,
                    pending_event,
                    candidates,
                    remaining,
                    checked,
                };
                status
            }

            ProcessState::CommitEvents { checked } => {
                let (caused_by_command, parent_event) = self
                    .pipeline_context
                    .as_ref()
                    .map(|ctx| (ctx.caused_by_command, ctx.parent_event))
                    .unwrap_or((None, None));

                for kind in checked {
                    let event_id = self.state.allocate_event_id();
                    let sequence_no = self.state.allocate_sequence_no();
                    self.state.ledger.push(GameEvent {
                        event_id,
                        sequence_no,
                        kind,
                        caused_by_command,
                        parent_event,
                    });
                }
                self.process_state = ProcessState::SbaLoop;
                StepStatus::Progressed
            }

            ProcessState::SbaLoop => {
                let events = sba::check_state_based_actions(&mut self.state);
                if events.is_empty() {
                    let since =
                        self.pipeline_context.as_ref().map(|ctx| ctx.since_ledger).unwrap_or(0);
                    self.process_state = ProcessState::CollectTriggers { since };
                } else {
                    self.state.ledger.extend(events);
                    // Re-enter SbaLoop next step() - CR 704.3's fixed
                    // point, not a single pass.
                    self.process_state = ProcessState::SbaLoop;
                }
                StepStatus::Progressed
            }

            ProcessState::CollectTriggers { since } => {
                // TODO: scan self.state.ledger[since..] and call
                // triggers::collect_triggers per event, accumulating
                // PendingTriggers, then triggers::group_apnap them and move
                // to ProcessState::PutTriggersApnap { ordered: Vec::new(),
                // pending_groups: <grouped> }. `since` is now correctly
                // threaded through from pipeline_context (no longer
                // hardcoded to 0) - but collect_triggers itself is still
                // todo!(), so wiring further wouldn't get past it.
                let _ = since;
                todo!()
            }

            ProcessState::PutTriggersApnap { mut ordered, mut pending_groups } => {
                // Walks pending_groups (already in APNAP order, per
                // triggers::group_apnap) one group per step() call rather
                // than ordering+pushing them all in one shot - a group with
                // more than one trigger needs its controller to choose the
                // intra-group order (CR 603.3b), which has to pause the
                // whole run on AwaitTriggerOrderChoice, not get resolved
                // without them.
                let Some((player, group)) = pending_groups.pop() else {
                    // TODO: push each trigger in `ordered` onto
                    // self.state.stack as a StackObject (needs StackObject
                    // construction from PendingTrigger - blocked on the
                    // same script-bridge gap as everywhere else
                    // triggered/activated abilities need a stack
                    // representation). Do that before consuming
                    // end_pipeline_run() below, since this is the last
                    // chance to read `ordered`.
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

            ProcessState::AwaitTriggerOrderChoice { player, group, ordered, pending_groups } => {
                // Nothing to do until submit_trigger_order answers - put
                // the exact same state back and tell the caller who needs
                // to be prompted, mirroring AwaitingReplacementChoice.
                let status =
                    StepStatus::AwaitingTriggerOrderChoice { player, group: group.clone() };
                self.process_state =
                    ProcessState::AwaitTriggerOrderChoice { player, group, ordered, pending_groups };
                status
            }

            ProcessState::AdvanceTurnStep => {
                // TODO: advance state.step/state.phase/state.turn_number
                // per CR 500-514 (the general version - the PassPriority
                // dry run only had a hardcoded Beginning-phase Untap->Upkeep
                // transition, see docs/implementation.md). Once advanced,
                // reset priority to state.active_player (CR 117.3b) and
                // transition to ProcessState::PriorityWindow.
                todo!()
            }
        }
    }
}
