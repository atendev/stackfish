use crate::types::ability::AbilityKind;
use crate::types::event::GameEvent;
use crate::types::game::GameState;
use crate::types::ids::{ObjectId, PlayerId};

/// A triggered ability that matched an event and is waiting to be placed on
/// the stack — not yet a `StackObject` (no chosen targets/modes/costs yet,
/// those come from the controller once it's actually being put on the
/// stack), and not part of `GameState` (it's transient process state
/// between "trigger matched" and "pushed to stack", not part of the
/// authoritative game state model).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PendingTrigger {
    pub source: ObjectId,
    pub controller: PlayerId,
    pub ability_index: u32,
}

/// Finds every triggered ability that matches `event` and is currently
/// legal to trigger (timing, once-per-turn, range, etc — CR 603.2-603.3).
/// Order in the returned `Vec` is not yet APNAP order; see `group_apnap`.
pub fn collect_triggers(state: &GameState, event: &GameEvent) -> Vec<PendingTrigger> {
    let _ = event;
    if !state.abilities.iter().any(|a| a.kind == AbilityKind::Triggered) {
        // Genuinely nothing to match against - safe to no-op rather than
        // hit the todo!() below, which only bites once a *triggered*
        // RegisteredAbility actually exists. Activated/static abilities
        // (e.g. CR 605 mana abilities, added in the activate-ability-mana
        // slice) don't fire off events at all, so their presence alone
        // shouldn't force this function past its no-op path.
        return Vec::new();
    }
    // TODO: scan state.abilities for RegisteredAbility entries where kind
    // == AbilityKind::Triggered and condition (if Some) evaluates true
    // against event (needs ScriptRuntime to invoke the condition
    // ScriptHandle - signature will need one threaded through, same
    // pattern as every other script-condition gap in this codebase).
    // Also needs once-per-turn/range/timing legality checks (CR 603.2-603.3)
    // that aren't designed yet beyond the condition callback itself.
    //
    // IMPORTANT (CR 603.10): a condition checking "did this object leave a
    // zone" must consult event.kind's ZoneChange::last_known_info (if
    // Some) instead of looking `object` up live via state.object() - by the
    // time this runs (post-CommitEvents/SbaLoop), a live lookup can miss
    // characteristics the object only had at the moment it left (e.g. an
    // ability granted by an aura that left in the same batch), or the id
    // may not resolve to anything at all if the move minted a new
    // ObjectId. See event.rs's doc comment on ZoneChange for the full case.
    //
    // IMPORTANT (CR 601.2i): a "whenever you cast a spell" condition must
    // NOT match on GameEventKind::StackObjectPushed alone - that event
    // represents CR 601.2a's *proposal* (card moved to stack), not the
    // spell actually becoming cast, which only happens once its cost is
    // paid (601.2i). There's no dedicated "spell was cast" event yet -
    // either add one (e.g. emitted alongside/after GameEventKind::CostPaid
    // in engine.rs's submit_payment, only for StackEntryKind::Spell), or
    // have this condition check require both StackObjectPushed AND a
    // matching CostPaid for the same stack_id before it's considered a
    // completed cast. Timing happens to work out correctly today regardless
    // (this function isn't invoked until CollectTriggers, which never runs
    // until after the ProcessState::AwaitingPayment pause resolves - see
    // engine.rs) - but matching the wrong event would still be semantically
    // wrong (e.g. would also incorrectly fire for a spell whose cost was
    // never actually paid, if 601.6's rollback gap ever gets fixed and a
    // proposed-but-uncast spell becomes representable).
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
    if triggers.is_empty() {
        return Vec::new();
    }
    // Rotate turn_order to start at active_player (APNAP order), then
    // partition triggers by controller in that order. Intra-group order is
    // deliberately left as collect_triggers produced it - that's the
    // controller's choice (CR 603.3b), resolved by
    // engine.rs/AwaitTriggerOrderChoice, not here.
    let mut order: Vec<PlayerId> = Vec::new();
    match state.turn_order.iter().position(|p| *p == state.active_player) {
        Some(start) => {
            order.extend(state.turn_order[start..].iter().copied());
            order.extend(state.turn_order[..start].iter().copied());
        }
        None => order.extend(state.turn_order.iter().copied()),
    }
    order
        .into_iter()
        .filter_map(|pid| {
            let group: Vec<PendingTrigger> =
                triggers.iter().copied().filter(|t| t.controller == pid).collect();
            if group.is_empty() {
                None
            } else {
                Some((pid, group))
            }
        })
        .collect()
}
