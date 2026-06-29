use std::ops::Range;

use crate::types::command::PlayerAction;
use crate::types::game::GameState;
use crate::types::ids::PlayerId;

/// Why `submit_action` rejected a `PlayerAction` outright (wrong player has
/// priority, action doesn't match what's currently legal, etc). Distinct
/// from in-progress command execution failures, which surface through
/// `CommandError` instead once the action is accepted and being carried out.
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

/// The step-able rules processor (DESIGN.md's "step-able core"). Wraps
/// `GameState` (pure data) with the actual turn/phase/priority/stack
/// advancement logic. Live play drives `step()` in a tight loop between
/// player decisions; a replay viewer re-feeds the same seed + action stream
/// into a fresh `Engine` and calls `step()` one at a time to pause/scrub.
pub struct Engine {
    state: GameState,
}

impl Engine {
    pub fn new(state: GameState) -> Self {
        // Implemented ahead of phase order, same reason as CounterSet::new
        // (counter.rs) - `state` is Engine's only field and it's private,
        // so the Phase 4 scenario tooling has no way to construct an
        // Engine at all without this existing for real.
        Engine { state }
    }

    pub fn state(&self) -> &GameState {
        &self.state
    }

    /// Validates `action` is legal right now (right player, matches what's
    /// actually available) and queues it for the next `step()` to act on.
    /// Does not itself advance the ledger — `step()` does that.
    pub fn submit_action(&mut self, action: PlayerAction) -> Result<(), ActionError> {
        // TODO: needs an internal "pending action" slot on Engine that
        // doesn't exist yet (this struct currently only holds `state`) -
        // check action.player matches whoever GameState says currently has
        // priority/is expected to act, check action.kind is one of the
        // currently-legal options, then store it for step() to consume.
        // Validation itself depends on the legality logic this whole crate
        // is building toward (cost-affordability, target legality, etc) -
        // not implementable until those pieces exist.
        todo!()
    }

    /// Advances by one ledger entry's worth of work and returns. Callers
    /// that don't care about pausing (live play) just call this in a loop
    /// until `AwaitingAction`/`GameOver`; a replay viewer calls it once per
    /// UI "step".
    pub fn step(&mut self) -> StepResult {
        // TODO: this is the orchestration point nothing currently wires
        // together. Rough shape per DESIGN.md/adaptation doc's pipeline:
        //   1. If a pending action is queued (from submit_action), turn it
        //      into an EngineCommand and run commands::execute_command.
        //   2. Run replacement::apply_replacements on any newly-proposed
        //      event before it commits (CR 614-616).
        //   3. Loop sba::check_state_based_actions until it returns empty
        //      (CR 704.3's fixed-point requirement - check_state_based_actions
        //      itself is one pass only, see sba.rs).
        //   4. For each event produced, triggers::collect_triggers +
        //      triggers::order_apnap, then push resulting PendingTriggers
        //      onto state.stack as StackObjects.
        //   5. Advance turn/phase/step/priority per CR 500-514, 117 once
        //      the stack is empty and all players have passed.
        //   6. Return StepResult with the right events_added range and
        //      StepStatus (Progressed / AwaitingAction / GameOver).
        // None of steps 1-5 have a home yet beyond their own modules' single
        // entry points - this function is where they need to be sequenced.
        todo!()
    }
}
