use std::ops::Range;

use crate::commands::{self, CommandError};
use crate::engine_context::{PipelineContext, PipelineExit};
use crate::replacement;
use crate::sba;
use crate::triggers::PendingTrigger;
use crate::types::command::{EngineCommand, PlayerAction};
use crate::types::event::{GameEvent, GameEventKind};
use crate::types::game::GameState;
use crate::types::ids::{CommandId, PlayerId, SequenceNo};

/// Why `submit_action` rejected a `PlayerAction` outright (wrong player has
/// priority, action doesn't match what's currently legal, etc). Distinct
/// from in-progress command execution failures, which surface through
/// `CommandError` instead once the action is accepted and being carried out.
#[derive(Debug)]
pub enum ActionError {
    NotYourTurn,
    IllegalAction(String),
}

/// What happened on one `step()` call, and what the engine needs next.
pub enum StepStatus {
    /// Made progress; safe to call `step()` again immediately.
    Progressed,
    /// Needs a `PlayerAction` from `player` before it can continue — the
    /// caller should prompt and `submit_action`, then `step()` again.
    AwaitingAction { player: PlayerId },
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
    /// 614-616) before they're allowed to commit.
    ReplacementCheck { proposed: Vec<GameEventKind> },
    /// Checked events ready to become real `GameEvent`s in the ledger.
    CommitEvents { checked: Vec<GameEventKind> },
    /// CR 704.3's fixed-point loop.
    SbaLoop,
    /// Collecting triggered abilities that matched ledger events from
    /// index `since` onward.
    CollectTriggers { since: usize },
    /// Triggers collected, waiting to be APNAP-ordered and pushed to the
    /// stack as `StackObject`s.
    PutTriggersApnap { triggers: Vec<PendingTrigger> },
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
    /// when a pipeline run begins (`PendingAction`/`ResolveStackTop`) and
    /// read by `CommitEvents`/`CollectTriggers`.
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

                if let Err(err) = commands::execute_command(&mut self.state, command) {
                    // Reset to PriorityWindow rather than leaving process_state
                    // in limbo - see StepStatus::Error's doc comment for why
                    // this doesn't attempt rollback.
                    self.process_state = ProcessState::PriorityWindow;
                    return StepStatus::Error(err);
                }

                self.pipeline_context = Some(PipelineContext {
                    since_ledger,
                    caused_by_command: Some(command_id),
                    parent_event: None,
                });

                // CONTRACT (documented on execute_command, commands.rs):
                // every successful variant other than PassPriority resets
                // consecutive_passes to 0. This is the only place that
                // decides what "everyone just passed" means; it can't tell
                // which variant ran, only what state.consecutive_passes
                // and state.stack ended up as.
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

                self.process_state = ProcessState::SbaLoop;
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
                // before producing any proposed events, same as
                // PendingAction does above.
                todo!()
            }

            ProcessState::ReplacementCheck { proposed } => {
                let checked = proposed
                    .into_iter()
                    .map(|event| replacement::apply_replacements(&mut self.state, event))
                    .collect();
                self.process_state = ProcessState::CommitEvents { checked };
                StepStatus::Progressed
            }

            ProcessState::CommitEvents { checked } => {
                let (caused_by_command, parent_event) = self
                    .pipeline_context
                    .as_ref()
                    .map(|ctx| (ctx.caused_by_command, ctx.parent_event))
                    .unwrap_or((None, None));

                for kind in checked {
                    let event_id = self.state.allocate_event_id();
                    let sequence_no = SequenceNo(self.state.ledger.len() as u64);
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
                // PendingTriggers, then move to PutTriggersApnap. `since`
                // is now correctly threaded through from pipeline_context
                // (no longer hardcoded to 0) - but collect_triggers itself
                // is still todo!(), so wiring further wouldn't get past it.
                let _ = since;
                todo!()
            }

            ProcessState::PutTriggersApnap { triggers } => {
                // TODO: triggers::order_apnap(&self.state, triggers), push
                // each as a StackObject onto self.state.stack, then consume
                // self.pipeline_exit (replacing it with BackToPriority) to
                // decide the next ProcessState:
                //   BackToPriority -> ProcessState::PriorityWindow (priority
                //     stays wherever execute_command left it - only the
                //     all-pass branches reset it, per CR 117.3b/c)
                //   ResolveStackTop -> ProcessState::ResolveStackTop
                //   AdvanceTurnStep -> ProcessState::AdvanceTurnStep
                let _ = triggers;
                todo!()
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
