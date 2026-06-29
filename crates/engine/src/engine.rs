use std::collections::VecDeque;
use std::ops::Range;

use crate::commands::{self, CommandError, PriorityHandoff};
use crate::engine_context::{PipelineContext, PipelineExit};
use crate::replacement::{self, ReplacementOutcome};
use crate::sba;
use crate::triggers::{self, PendingTrigger};
use crate::types::card::CardTypeSet;
use crate::types::command::{EngineCommand, PlayerAction};
use crate::types::event::{GameEvent, GameEventKind};
use crate::types::game::GameState;
use crate::types::ids::{CommandId, ObjectId, ObjectLineage, PlayerId};
use crate::types::stack::StackEntryKind;
use crate::types::turn::{Phase, Step};
use crate::types::zone::ZoneKind;

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
    /// Same length as the offered group, but not the same multiset of
    /// `PendingTrigger`s — length matching alone doesn't rule out the caller
    /// substituting/dropping/duplicating a trigger while keeping the count
    /// right, so this is checked separately (see `is_permutation`).
    NotAPermutation,
}

/// Whether `chosen` is a reordering of `group` — same `PendingTrigger`s
/// (by value), each used exactly once, any order. Used by
/// `submit_trigger_order` to reject a same-length answer that isn't
/// actually a permutation (swapped-in trigger, duplicate, etc).
fn is_permutation(chosen: &[PendingTrigger], group: &[PendingTrigger]) -> bool {
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

/// What happened on one `step()` call, and what the engine needs next.
#[derive(Debug)]
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
    /// `ReplacementCheck` run; `remaining` is what's still unprocessed — a
    /// `VecDeque` so it can be drained front-to-back (`pop_front`) in the
    /// order it was built, rather than a `Vec`'s `pop()` silently reversing
    /// processing order.
    ReplacementCheck { remaining: VecDeque<GameEventKind>, checked: Vec<GameEventKind> },
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
        remaining: VecDeque<GameEventKind>,
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
    /// `pending_groups` is a `VecDeque` for the same reason
    /// `ReplacementCheck::remaining` is: drained front-to-back in APNAP
    /// order, not reversed by a `Vec::pop()`.
    PutTriggersApnap {
        ordered: Vec<PendingTrigger>,
        pending_groups: VecDeque<(PlayerId, Vec<PendingTrigger>)>,
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
        pending_groups: VecDeque<(PlayerId, Vec<PendingTrigger>)>,
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

    /// Diagnostic for tests/replay tooling: whether `pipeline_context` is
    /// currently set. Should be `false` whenever `process_state` is
    /// `PriorityWindow` - `end_pipeline_run` is the only place that clears
    /// it, and `PriorityWindow` is only ever reached through there (or
    /// `AdvanceTurnStep`, which also doesn't set it).
    pub fn pipeline_context_active(&self) -> bool {
        self.pipeline_context.is_some()
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

                self.process_state = ProcessState::ReplacementCheck {
                    remaining: outcome.events.into(),
                    checked: Vec::new(),
                };
                StepStatus::Progressed
            }

            ProcessState::ResolveStackTop => {
                let Some(top) = self.state.stack.pop() else {
                    // Shouldn't happen - pipeline_exit only routes here when
                    // the all-pass check saw a non-empty stack. Defensive
                    // fallback rather than a panic, since nothing downstream
                    // depends on this path being unreachable yet.
                    self.process_state = ProcessState::PriorityWindow;
                    return StepStatus::Progressed;
                };
                // CR 117.3b: once the stack object finishes resolving (after
                // SBA/triggers settle below), priority returns to the active
                // player. Set directly now rather than threading it through
                // PipelineExit - simpler for this minimal slice, and nothing
                // between here and PriorityWindow needs a different value.
                self.state.priority_player = self.state.active_player;
                let since_ledger = self.state.ledger.len();
                self.pipeline_context =
                    Some(PipelineContext { since_ledger, caused_by_command: None, parent_event: None });
                self.pipeline_exit = PipelineExit::BackToPriority;
                // TODO: real resolution should look up top.operation
                // (ScriptHandle) and invoke it via
                // ScriptRuntime::invoke_operation to actually carry out
                // whatever the spell/ability says - blocked on the script
                // bridge like every other script-shaped gap. What follows is
                // only CR 608.2m's zone-change half of resolution (a spell's
                // card moves off the stack once it resolves) - it does not
                // run any spell effect.
                let mut events = Vec::new();
                if matches!(top.kind, StackEntryKind::Spell) {
                    // Defensive: the spell's GameObject should always still
                    // be at top.stack_id (nothing else removes it while on
                    // the stack) - None here would mean some other path
                    // already mutated it unexpectedly. Skip the zone move
                    // rather than panic, same defensive stance as the empty-
                    // stack branch above.
                    if let Some(spell_object) = self.state.object(top.stack_id) {
                        let last_known_info = spell_object.clone();
                        // CR 608.2m: a permanent type goes to the
                        // battlefield as a permanent; an instant or sorcery
                        // (the only other types castable as a spell - see
                        // CastSpell's land rejection in commands.rs) goes to
                        // its owner's graveyard instead.
                        let is_permanent = !last_known_info
                            .definition
                            .card_types
                            .intersects(CardTypeSet::INSTANT | CardTypeSet::SORCERY);
                        let destination = if is_permanent {
                            ZoneKind::Battlefield
                        } else {
                            ZoneKind::Graveyard
                        };
                        let destination_owner =
                            if is_permanent { None } else { Some(last_known_info.owner) };

                        // CR 400.7: another zone change, another new id -
                        // same convention as PlayLand/CastSpell's hand moves.
                        let new_id = self.state.allocate_object_id();
                        let mut new_object = last_known_info.clone();
                        new_object.object_id = new_id;
                        new_object.lineage = ObjectLineage { previous: Some(top.stack_id) };
                        new_object.zone = destination;
                        new_object.timestamp = self.state.allocate_sequence_no();

                        self.state.objects.remove(&top.stack_id);
                        if let Some(zone) = self.state.zone_mut(destination, destination_owner) {
                            zone.push_bottom(new_id);
                            self.state.objects.insert(new_id, new_object);
                        }
                        // else: no such zone registered - defensive no-op,
                        // same stance as PlayLand's zone_mut lookups, just
                        // without a CommandError to return from here.

                        events.push(GameEventKind::ZoneChange {
                            object: new_id,
                            from: ZoneKind::Stack,
                            to: destination,
                            last_known_info: Some(last_known_info),
                        });
                    }
                }
                // ActivatedAbility/TriggeredAbility/Copy: nothing on a zone
                // moves when these resolve (CR 608's ability-resolution path
                // doesn't involve a card) - only the Spell branch above
                // produces a ZoneChange.
                events.push(GameEventKind::StackObjectResolved { stack_id: top.stack_id });
                self.process_state =
                    ProcessState::ReplacementCheck { remaining: events.into(), checked: Vec::new() };
                StepStatus::Progressed
            }

            ProcessState::ReplacementCheck { mut remaining, mut checked } => {
                // Processes one event per step() call rather than mapping
                // the whole Vec in one shot — apply_replacements can return
                // NeedsOrderingChoice (CR 616.1) for any of them, and that
                // has to pause the whole pipeline run on a real
                // AwaitingReplacementChoice, not get silently dropped or
                // resolved without the player. `checked`/`remaining` carry
                // this run's progress across that pause.
                let Some(event) = remaining.pop_front() else {
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
                let mut pending = Vec::new();
                for event in &self.state.ledger[since..] {
                    pending.extend(triggers::collect_triggers(&self.state, event));
                }
                let groups = triggers::group_apnap(&self.state, pending);
                self.process_state =
                    ProcessState::PutTriggersApnap { ordered: Vec::new(), pending_groups: groups.into() };
                StepStatus::Progressed
            }

            ProcessState::PutTriggersApnap { mut ordered, mut pending_groups } => {
                // Walks pending_groups (already in APNAP order, per
                // triggers::group_apnap) one group per step() call rather
                // than ordering+pushing them all in one shot - a group with
                // more than one trigger needs its controller to choose the
                // intra-group order (CR 603.3b), which has to pause the
                // whole run on AwaitTriggerOrderChoice, not get resolved
                // without them.
                let Some((player, group)) = pending_groups.pop_front() else {
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
                let events = self.advance_turn_step();
                for kind in events {
                    let event_id = self.state.allocate_event_id();
                    let sequence_no = self.state.allocate_sequence_no();
                    self.state.ledger.push(GameEvent {
                        event_id,
                        sequence_no,
                        kind,
                        caused_by_command: None,
                        parent_event: None,
                    });
                }
                // CR 117.3b: priority opens to the active player at the
                // start of each step/phase.
                self.state.priority_player = self.state.active_player;
                self.process_state = ProcessState::PriorityWindow;
                StepStatus::Progressed
            }
        }
    }

    /// Deterministic CR 500-514 phase/step transition table for this slice:
    /// advances `state.phase`/`step`/`turn_number`/`active_player`/
    /// `postcombat_main` and returns the log events for it. Does NOT carry
    /// out any of the actual rules work a step triggers (untapping
    /// permanents, drawing a card, etc.) - that's separate, larger work
    /// still gated on other todo!()s (layers.rs, zone manipulation); this
    /// only answers "what step comes next."
    ///
    /// `postcombat_main` resolves CR 505.1's "Main happens twice" ambiguity
    /// that `Phase` alone can't (see turn.rs's `Phase` doc) - added to
    /// `GameState` for this slice.
    fn advance_turn_step(&mut self) -> Vec<GameEventKind> {
        let starting_new_turn = match (self.state.phase, self.state.step) {
            (Phase::PreGame, _) => {
                self.state.phase = Phase::Beginning;
                self.state.step = Step::Untap;
                true
            }
            (Phase::Beginning, Step::Untap) => {
                self.state.step = Step::Upkeep;
                false
            }
            (Phase::Beginning, Step::Upkeep) => {
                self.state.step = Step::Draw;
                false
            }
            (Phase::Beginning, Step::Draw) => {
                self.state.phase = Phase::Main;
                self.state.postcombat_main = false;
                false
            }
            (Phase::Main, _) if !self.state.postcombat_main => {
                self.state.phase = Phase::Combat;
                self.state.step = Step::BeginningOfCombat;
                false
            }
            (Phase::Combat, Step::BeginningOfCombat) => {
                self.state.step = Step::DeclareAttackers;
                false
            }
            (Phase::Combat, Step::DeclareAttackers) => {
                self.state.step = Step::DeclareBlockers;
                false
            }
            (Phase::Combat, Step::DeclareBlockers) => {
                self.state.step = Step::CombatDamage;
                false
            }
            (Phase::Combat, Step::CombatDamage) => {
                self.state.step = Step::EndOfCombat;
                false
            }
            (Phase::Combat, Step::EndOfCombat) => {
                self.state.phase = Phase::Main;
                self.state.postcombat_main = true;
                false
            }
            // postcombat_main == true here, since the guarded Main arm above
            // already claimed the precombat case.
            (Phase::Main, _) => {
                self.state.phase = Phase::Ending;
                self.state.step = Step::End;
                false
            }
            (Phase::Ending, Step::End) => {
                self.state.step = Step::Cleanup;
                false
            }
            (Phase::Ending, Step::Cleanup) => {
                let current_idx = self
                    .state
                    .turn_order
                    .iter()
                    .position(|p| *p == self.state.active_player)
                    .unwrap_or(0);
                let next_idx = (current_idx + 1) % self.state.turn_order.len();
                self.state.active_player = self.state.turn_order[next_idx];
                self.state.turn_number += 1;
                self.state.phase = Phase::Beginning;
                self.state.step = Step::Untap;
                self.state.postcombat_main = false;
                // CR 305.2's per-turn land-drop count resets with the turn,
                // same trigger as postcombat_main above.
                self.state.lands_played_this_turn.clear();
                true
            }
            (phase, step) => unreachable!(
                "invalid Phase/Step combination {phase:?}/{step:?} - Step values are only \
                 meaningful within the Phase they belong to (CR 500-514); reaching this means \
                 state held a combination that doesn't correspond to any real turn position"
            ),
        };

        let mut events =
            vec![GameEventKind::PhaseStarted { phase: self.state.phase }, GameEventKind::StepStarted {
                step: self.state.step,
            }];
        if starting_new_turn {
            events.push(GameEventKind::TurnStarted {
                player: self.state.active_player,
                turn_number: self.state.turn_number,
            });
        }
        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::counter::CounterSet;
    use crate::types::mana::ManaPool;
    use crate::types::player::PlayerState;
    use std::collections::HashMap;

    fn minimal_state() -> GameState {
        let p0 = PlayerId(0);
        GameState {
            players: vec![PlayerState {
                player_id: p0,
                life: 20,
                mana_pool: ManaPool { white: 0, blue: 0, black: 0, red: 0, green: 0, colorless: 0 },
                counters: CounterSet::new(),
                emblems: Vec::new(),
                commander_damage_taken: Vec::new(),
                eliminated: false,
            }],
            turn_order: vec![p0],
            active_player: p0,
            turn_number: 1,
            phase: Phase::PreGame,
            step: Step::Untap,
            priority_player: p0,
            consecutive_passes: 0,
            postcombat_main: false,
            lands_played_this_turn: HashMap::new(),
            objects: HashMap::new(),
            zones: Vec::new(),
            stack: Vec::new(),
            continuous_effects: Vec::new(),
            replacement_effects: Vec::new(),
            abilities: Vec::new(),
            ledger: Vec::new(),
            rng: crate::types::game::RngState { seed: 1, draws_consumed: 0 },
            next_event_id: 0,
            next_sequence_no: 0,
            next_object_id: 0,
        }
    }

    fn trigger(id: u64, controller: PlayerId) -> PendingTrigger {
        PendingTrigger { source: ObjectId(id), controller, ability_index: 0 }
    }

    #[test]
    fn is_permutation_accepts_reorderings_and_rejects_substitutions() {
        let group = vec![trigger(1, PlayerId(0)), trigger(2, PlayerId(0))];
        // A true reordering (same elements, different sequence) must pass.
        assert!(is_permutation(&[group[1], group[0]], &group));
        // Same length but a substituted element must be rejected - length
        // alone can't catch this.
        assert!(!is_permutation(&[group[0], trigger(3, PlayerId(0))], &group));
        // A duplicate of one element instead of using both must be rejected.
        assert!(!is_permutation(&[group[0], group[0]], &group));
        // Wrong length, caught the same way as before.
        assert!(!is_permutation(&[group[0]], &group));
    }

    #[test]
    fn submit_trigger_order_rejects_non_permutation_and_accepts_real_reordering() {
        let player = PlayerId(0);
        let group = vec![trigger(1, player), trigger(2, player)];

        let mut engine = Engine::new(minimal_state());
        engine.process_state = ProcessState::AwaitTriggerOrderChoice {
            player,
            group: group.clone(),
            ordered: Vec::new(),
            pending_groups: VecDeque::new(),
        };
        let substituted = vec![group[0], trigger(99, player)];
        let err = engine.submit_trigger_order(substituted).unwrap_err();
        assert!(matches!(err, TriggerOrderError::NotAPermutation));

        // Engine must still be sitting in AwaitTriggerOrderChoice - a
        // rejected answer must not have consumed the pending state.
        assert!(matches!(engine.process_state, ProcessState::AwaitTriggerOrderChoice { .. }));

        let reordered = vec![group[1], group[0]];
        engine.submit_trigger_order(reordered).expect("a true reordering must be accepted");
        assert!(matches!(engine.process_state, ProcessState::PutTriggersApnap { .. }));
    }
}
