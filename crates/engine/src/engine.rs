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
        todo!()
    }

    pub fn state(&self) -> &GameState {
        todo!()
    }

    /// Validates `action` is legal right now (right player, matches what's
    /// actually available) and queues it for the next `step()` to act on.
    /// Does not itself advance the ledger — `step()` does that.
    pub fn submit_action(&mut self, action: PlayerAction) -> Result<(), ActionError> {
        todo!()
    }

    /// Advances by one ledger entry's worth of work and returns. Callers
    /// that don't care about pausing (live play) just call this in a loop
    /// until `AwaitingAction`/`GameOver`; a replay viewer calls it once per
    /// UI "step".
    pub fn step(&mut self) -> StepResult {
        todo!()
    }
}
