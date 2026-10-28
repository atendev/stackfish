//! The `ForEachPlayer` arm (CR 101.4): re-invokes a script function per player in
//! APNAP order, plus the outcome handling shared with `choice_answers`. See `DESIGN.md`.

use std::collections::VecDeque;
use std::ops::ControlFlow;

use crate::script::{OperationOutcome, ScriptZoneMoveKind};
use crate::triggers::apnap_player_order;
use crate::types::ability::AbilityEffect;
use crate::types::event::GameEventKind;
use crate::types::ids::{ObjectId, PlayerId};
use crate::types::stack::StackObject;

use super::super::{EachPlayerLoop, Engine, ProcessState, ScriptedZoneMoveKind, ScriptedZoneMoveThen, StepStatus};

impl Engine {
    /// CR 101.4: builds the APNAP player order. No players is a defensive no-op.
    /// `wrapped` is `Some(inner)` for a bare `SacrificeRandom` or `DiscardRandom`
    /// tag: each player's picks are drawn synchronously and merged into one queue
    /// before the single zone move, which is simultaneous for free. `None` is the
    /// script-function shape.
    pub(super) fn resolve_for_each_player_effect(
        &mut self,
        top: &StackObject,
        wrapped: &Option<Box<AbilityEffect>>,
        events: Vec<GameEventKind>,
    ) -> ControlFlow<StepStatus, Vec<GameEventKind>> {
        let mut order: VecDeque<PlayerId> = apnap_player_order(&self.state).into();
        let Some(first_player) = order.pop_front() else {
            return ControlFlow::Continue(events);
        };

        if let Some(inner) = wrapped {
            let mut all_players: VecDeque<PlayerId> = VecDeque::from([first_player]);
            all_players.extend(order);
            // `LoseLife` has no zone move; `Sacrifice` just drives an empty queue to the shared tail.
            let kind = match inner.as_ref() {
                AbilityEffect::SacrificeRandom { .. } | AbilityEffect::LoseLife(_) => ScriptedZoneMoveKind::Sacrifice,
                AbilityEffect::DiscardRandom { .. } => ScriptedZoneMoveKind::Discard,
                AbilityEffect::Mill(_) => ScriptedZoneMoveKind::Mill,
                _ => unreachable!(
                    "Game.Ops.ForEachPlayer's Lua binding only ever accepts SacrificeRandom/DiscardRandom/Mill/LoseLife as a bare tag"
                ),
            };
            let mut events = events;
            let mut ids: VecDeque<ObjectId> = VecDeque::new();
            for player in all_players {
                let picked = match inner.as_ref() {
                    AbilityEffect::SacrificeRandom { filter, count } => {
                        self.pick_sacrifice_random_ids(top.source, player, filter, *count)
                    }
                    AbilityEffect::DiscardRandom { filter, count } => {
                        self.pick_discard_random_ids(top.source, player, filter, *count)
                    }
                    AbilityEffect::Mill(n) => self.top_of_library_ids(player, *n),
                    AbilityEffect::LoseLife(n) => {
                        self.lose_life_now(player, *n, &mut events);
                        VecDeque::new()
                    }
                    _ => unreachable!("checked above"),
                };
                ids.extend(picked);
            }
            self.process_state = self.continue_scripted_zone_move(
                kind,
                ids,
                ScriptedZoneMoveThen::StackResolution { stack_id: top.stack_id },
                events,
            );
            return ControlFlow::Break(StepStatus::Progressed);
        }

        let each_player = EachPlayerLoop {
            source: top.source,
            operation: top.operation,
            remaining_players: order,
            sacrifice_ids: Vec::new(),
            discard_ids: Vec::new(),
        };
        let result =
            self.script_runtime.invoke_operation(each_player.operation, &mut self.state, each_player.source, first_player);
        if let Err(ref e) = result {
            self.log_events(vec![GameEventKind::ScriptError {
                message: format!("operation script error on source {:?}: {e:?}", each_player.source),
            }]);
        }
        let outcome = result.unwrap_or(OperationOutcome::Finished);
        self.process_state = self.settle_script_operation_outcome(
            outcome,
            top.stack_id,
            top.source,
            first_player,
            events,
            Some(each_player),
        );
        ControlFlow::Break(StepStatus::Progressed)
    }

    /// Folds one script operation outcome into the next `ProcessState`. Shared with
    /// `submit_script_choice` and `submit_script_may_choice`; a plain script pause
    /// arrives with `each_player: None`. `controller` is the loop's current player.
    pub(in crate::engine) fn settle_script_operation_outcome(
        &mut self,
        outcome: OperationOutcome,
        stack_id: ObjectId,
        source: ObjectId,
        controller: PlayerId,
        events_so_far: Vec<GameEventKind>,
        mut each_player: Option<EachPlayerLoop>,
    ) -> ProcessState {
        match outcome {
            OperationOutcome::NeedsChoice { thread, request } => {
                ProcessState::AwaitingScriptChoice { thread, request, stack_id, source, controller, events_so_far, each_player }
            }
            OperationOutcome::Finished => self.advance_or_finish_each_player(stack_id, events_so_far, each_player),
            // No pause of its own, so it works inside or outside a `ForEachPlayer` loop.
            OperationOutcome::PendingEvents(new_events) => {
                let mut events_so_far = events_so_far;
                events_so_far.extend(new_events);
                self.advance_or_finish_each_player(stack_id, events_so_far, each_player)
            }
            // `draw_cards`
            // TODO: not `ForEachPlayer`-aware; `continue_draw_cards` finishes the whole resolution when its queue drains
            OperationOutcome::PendingDraw { amount } => match each_player {
                None => self.continue_draw_cards(amount, controller, stack_id, events_so_far),
                Some(_) => self.advance_or_finish_each_player(stack_id, events_so_far, each_player),
            },
            // CR 101.4: mid-loop answers fold into the running totals instead of moving.
            // Outside a loop there is nothing to batch, so they move at once.
            OperationOutcome::PendingZoneMoves(moves) => match &mut each_player {
                None => self.chain_pending_zone_moves(moves, ScriptedZoneMoveThen::StackResolution { stack_id }, events_so_far),
                Some(loop_state) => {
                    for (kind, ids) in moves {
                        match kind {
                            ScriptZoneMoveKind::Sacrifice => loop_state.sacrifice_ids.extend(ids),
                            ScriptZoneMoveKind::Discard => loop_state.discard_ids.extend(ids),
                        }
                    }
                    self.advance_or_finish_each_player(stack_id, events_so_far, each_player)
                }
            },
            // A `Game.Ops.CastSpell` result reached after a resumed coroutine; see
            // `resolve_pending_cast`.
            // TODO: a per-player cast request mid-loop is skipped as if the player returned `Finished`
            OperationOutcome::PendingCast { card, modes, targets, x_value, free } => match each_player {
                None => self.resolve_pending_cast(card, modes, targets, x_value, free, stack_id, controller, events_so_far),
                Some(_) => self.advance_or_finish_each_player(stack_id, events_so_far, each_player),
            },
        }
    }

    /// Advances to the next player, or settles once the queue is empty. Only the
    /// empty-queue branch moves cards, so the actions are simultaneous (CR 101.4).
    fn advance_or_finish_each_player(
        &mut self,
        stack_id: ObjectId,
        events_so_far: Vec<GameEventKind>,
        each_player: Option<EachPlayerLoop>,
    ) -> ProcessState {
        let Some(mut loop_state) = each_player else {
            let mut final_events: VecDeque<GameEventKind> = events_so_far.into();
            final_events.push_back(GameEventKind::StackObjectResolved { stack_id });
            return ProcessState::ReplacementCheck { remaining: final_events, checked: Vec::new() };
        };
        let Some(next_player) = loop_state.remaining_players.pop_front() else {
            let moves = vec![
                (ScriptZoneMoveKind::Sacrifice, loop_state.sacrifice_ids),
                (ScriptZoneMoveKind::Discard, loop_state.discard_ids),
            ];
            return self.chain_pending_zone_moves(moves, ScriptedZoneMoveThen::StackResolution { stack_id }, events_so_far);
        };
        let result =
            self.script_runtime.invoke_operation(loop_state.operation, &mut self.state, loop_state.source, next_player);
        if let Err(ref e) = result {
            self.log_events(vec![GameEventKind::ScriptError {
                message: format!("operation script error on source {:?}: {e:?}", loop_state.source),
            }]);
        }
        let outcome = result.unwrap_or(OperationOutcome::Finished);
        let source = loop_state.source;
        self.settle_script_operation_outcome(outcome, stack_id, source, next_player, events_so_far, Some(loop_state))
    }
}
