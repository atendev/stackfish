use crate::types::event::GameEvent;
use crate::types::game::GameState;
use crate::types::ids::{ObjectId, PlayerId};

/// A triggered ability that matched an event and is waiting to be placed on
/// the stack — not yet a `StackObject` (no chosen targets/modes/costs yet,
/// those come from the controller once it's actually being put on the
/// stack), and not part of `GameState` (it's transient process state
/// between "trigger matched" and "pushed to stack", not part of the
/// authoritative game state model).
pub struct PendingTrigger {
    pub source: ObjectId,
    pub controller: PlayerId,
    pub ability_index: u32,
}

/// Finds every triggered ability that matches `event` and is currently
/// legal to trigger (timing, once-per-turn, range, etc — CR 603.2-603.3).
/// Order in the returned `Vec` is not yet APNAP order; see `order_apnap`.
pub fn collect_triggers(state: &GameState, event: &GameEvent) -> Vec<PendingTrigger> {
    // TODO: scan state.abilities for RegisteredAbility entries where kind
    // == AbilityKind::Triggered and condition (if Some) evaluates true
    // against event (needs ScriptRuntime to invoke the condition
    // ScriptHandle - signature will need one threaded through, same
    // pattern as every other script-condition gap in this codebase).
    // Also needs once-per-turn/range/timing legality checks (CR 603.2-603.3)
    // that aren't designed yet beyond the condition callback itself.
    todo!()
}

/// Reorders `triggers` into APNAP order (CR 603.3b): the active player's
/// triggers first, in their own chosen order, then each other player's in
/// turn order, generalized over `state.turn_order` rather than hardcoded
/// to two players.
pub fn order_apnap(state: &GameState, triggers: Vec<PendingTrigger>) -> Vec<PendingTrigger> {
    // TODO: partition triggers by controller, ordered by state.turn_order
    // starting at state.active_player (active player's triggers first).
    // Within one controller's own triggers, CR 603.3b leaves the order up
    // to that player to choose - this function can't make that choice
    // itself; either needs to return groups for the engine to ask about,
    // or take an already-player-chosen intra-group order as input. Not
    // fully resolvable with this signature alone.
    todo!()
}
