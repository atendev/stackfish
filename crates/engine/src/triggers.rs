use crate::types::event::GameEvent;
use crate::types::game::GameState;
use crate::types::ids::{ObjectId, PlayerId};

/// A triggered ability that matched an event and is waiting to be placed on
/// the stack — not yet a `StackObject` (no chosen targets/modes/costs yet,
/// those come from the controller once it's actually being put on the
/// stack), and not part of `GameState` (it's transient process state
/// between "trigger matched" and "pushed to stack", not part of the
/// authoritative game state model).
#[derive(Clone, Copy)]
pub struct PendingTrigger {
    pub source: ObjectId,
    pub controller: PlayerId,
    pub ability_index: u32,
}

/// Finds every triggered ability that matches `event` and is currently
/// legal to trigger (timing, once-per-turn, range, etc — CR 603.2-603.3).
/// Order in the returned `Vec` is not yet APNAP order; see `group_apnap`.
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

/// Groups `triggers` by controller in APNAP order (CR 603.3b): the active
/// player's triggers first, then each other player's in turn order,
/// generalized over `state.turn_order` rather than hardcoded to two
/// players. Each group is still in `collect_triggers`'s original (arbitrary)
/// order — within one controller's own triggers, CR 603.3b leaves the
/// order up to that player to choose, which this function can't resolve on
/// its own. `engine.rs`'s `ProcessState::PutTriggersApnap` walks the
/// returned groups in order, pausing on `AwaitTriggerOrderChoice` for any
/// group with more than one trigger so the controller can choose.
pub fn group_apnap(
    state: &GameState,
    triggers: Vec<PendingTrigger>,
) -> Vec<(PlayerId, Vec<PendingTrigger>)> {
    // TODO: partition triggers by controller, ordered by state.turn_order
    // starting at state.active_player (active player's triggers first).
    // Do NOT attempt to resolve intra-group order here - that's
    // engine.rs/AwaitTriggerOrderChoice's job.
    todo!()
}
