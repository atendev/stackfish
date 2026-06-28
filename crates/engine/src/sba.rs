use crate::types::event::GameEvent;
use crate::types::game::GameState;

/// One pass of CR 704's state-based actions: finds every applicable SBA
/// (lethal damage, 0 toughness, player at 0 life, 10+ poison, legend rule,
/// etc.) and applies all of them simultaneously, mutating `state` and
/// returning the `GameEvent`s that resulted (destroys, life-loss-induced
/// losses, etc) for the caller to append to the ledger and feed into
/// trigger collection.
///
/// This is a single pass, not the fixed-point loop — CR 704.3 requires
/// repeating until no SBA applies. The engine loop (`engine.rs`) is
/// responsible for calling this in a loop until it returns empty, not this
/// function itself, so a caller stepping through ledger entries one at a
/// time can observe each pass separately rather than the loop being opaque.
pub fn check_state_based_actions(state: &mut GameState) -> Vec<GameEvent> {
    todo!()
}
