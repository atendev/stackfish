//! `resolve_ability_effect`'s `ForEachPlayer` arm (`phase5/mass-choice-
//! effects`, CR 101.4) — re-invokes a script function once per player in
//! APNAP order, and the shared "what do we do with one invocation's
//! `OperationOutcome`" machinery `choice_answers::submit_script_choice`/
//! `submit_script_may_choice` also call once the *current* player's own
//! `Game.SelectTargets`/`Game.ChooseN`/`Game.MayChoose` ask is answered.
//! See `EachPlayerLoop`'s own doc comment (`process_state.rs`) for what
//! `remaining_players`/`sacrifice_ids`/`discard_ids` carry across a pause.

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
    /// CR 101.4: builds this loop's own APNAP player order (`active_player`
    /// first, then each other player in turn order — `triggers::
    /// apnap_player_order`, the same rotation `group_apnap` uses for trigger
    /// placement). No players (shouldn't be reachable - every real game has
    /// at least the active player) is a silent no-op, same defensive stance
    /// every other "should always have at least one candidate" arm in this
    /// dispatcher takes.
    ///
    /// `wrapped` (`phase5/mass-choice-effects`) is `AbilityEffect::
    /// ForEachPlayer`'s own field — `Some(inner)` for a bare `Game.Ops.
    /// SacrificeRandom`/`DiscardRandom` tag (no script involved at all: each
    /// player's own random picks are drawn directly via `pick_sacrifice_
    /// random_ids`/`pick_discard_random_ids`, pure synchronous Rust that
    /// can't pause mid-computation, then merged into one queue before the
    /// single real `continue_scripted_zone_move` call - already CR 101.4-
    /// "simultaneous" for free, no `EachPlayerLoop` pause-tracking needed
    /// since there's nothing that can pause until that one final call).
    /// `None` is the pre-existing script-function shape below.
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
            // LoseLife has no zone move: it applies per player below and the
            // `Sacrifice` kind here only drives an empty queue to the shared
            // `StackResolution` tail.
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

    /// What one `ScriptRuntime::invoke_operation`/`resume_operation`/
    /// `resume_operation_bool` call produced, folded into the next
    /// `ProcessState` — shared by `resolve_for_each_player_effect` above and
    /// `choice_answers::submit_script_choice`/`submit_script_may_choice`
    /// (a plain, non-`ForEachPlayer` script pause also funnels through
    /// here with `each_player: None`, so this replaces what used to be a
    /// two-way `match outcome { Finished => .., NeedsChoice => .. }` inlined
    /// in each of those methods). `controller` is whichever player this
    /// particular `outcome` belongs to — the loop's *current* player, not
    /// necessarily `top.controller`/the spell's original caster.
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
            // `deal_damage`/`add_counters` - no pause of its own (see
            // `OperationOutcome::PendingEvents`'s own doc comment), so this
            // works the same whether or not we're inside a `ForEachPlayer`
            // loop: fold the constructed events into the running list and
            // continue exactly as if this player's own invocation had
            // returned `Finished`.
            OperationOutcome::PendingEvents(new_events) => {
                let mut events_so_far = events_so_far;
                events_so_far.extend(new_events);
                self.advance_or_finish_each_player(stack_id, events_so_far, each_player)
            }
            // `draw_cards` - TODO: not `ForEachPlayer`-aware, same narrowing
            // `PendingCast` below already takes and for the same reason
            // (`continue_draw_cards` finishes the *whole* resolution when
            // its own queue drains, wrong if this is only one player's turn
            // in a loop with others still remaining) - no real card needs
            // "each player draws a card via a custom function" today.
            OperationOutcome::PendingDraw { amount } => match each_player {
                None => self.continue_draw_cards(amount, controller, stack_id, events_so_far),
                Some(_) => self.advance_or_finish_each_player(stack_id, events_so_far, each_player),
            },
            // CR 101.4: a mid-loop player's sacrifice_chosen/discard_chosen
            // answer is folded into the loop's own running totals, not
            // moved yet - see EachPlayerLoop's own doc comment for why.
            // Outside a ForEachPlayer loop (each_player: None) there's no
            // batching to do, so it moves immediately, same as before this
            // slice existed.
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
            // `Game.Ops.CastSpell`'s own result reached after a resumed
            // coroutine stops yielding (e.g. `Game.SelectTargets` first,
            // then `Game.Ops.CastSpell(...)` once resumed) - see
            // `resolve_pending_cast`'s own doc comment (`misc_effects.rs`)
            // for the shared execute_command-based logic.
            //
            // TODO: not `ForEachPlayer`-aware - a per-player cast request
            // mid-loop (`each_player: Some(_)`) isn't a supported combination
            // yet (no real card needs "each player may cast a card" today,
            // same "named, not silently wrong" stance `PendingZoneMoves`'
            // own batching support didn't need until a real card forced it);
            // falls through to `advance_or_finish_each_player` as if this
            // player's own invocation had returned `Finished` instead,
            // silently skipping the requested cast rather than performing it.
            OperationOutcome::PendingCast { card, modes, targets, x_value, free } => match each_player {
                None => self.resolve_pending_cast(card, modes, targets, x_value, free, stack_id, controller, events_so_far),
                Some(_) => self.advance_or_finish_each_player(stack_id, events_so_far, each_player),
            },
        }
    }

    /// The current player's own invocation is done (`Finished`, or a
    /// `PendingZoneMoves` already folded into `each_player`'s totals) —
    /// advance to whichever player is next in `each_player.remaining_
    /// players`, or (outside a loop, or once the loop's queue is empty)
    /// settle for real. CR 101.4's "then the actions happen simultaneously":
    /// only the empty-queue branch below ever calls `chain_pending_zone_
    /// moves` with real ids - every earlier player's own answer only ever
    /// grew `sacrifice_ids`/`discard_ids`, never moved anything.
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
