use std::collections::HashMap;

use crate::script::{ScriptContext, ScriptRuntime};
use crate::types::ability::{AbilityKind, ConditionSpec};
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
///
/// `runtime` invokes each ability's `condition` callback (`None` means "no
/// filter, always matches" per `RegisteredAbility::condition`'s doc). Once-
/// per-turn/range/timing legality (CR 603.2-603.3) beyond the condition
/// callback itself is still out of scope — see the IMPORTANT notes below,
/// carried over unchanged from before this function had a real body.
///
/// IMPORTANT (CR 603.10): a condition checking "did this object leave a
/// zone" must consult event.kind's ZoneChange::last_known_info (if
/// Some) instead of looking `object` up live via state.object() - by the
/// time this runs (post-CommitEvents/SbaLoop), a live lookup can miss
/// characteristics the object only had at the moment it left (e.g. an
/// ability granted by an aura that left in the same batch), or the id
/// may not resolve to anything at all if the move minted a new
/// ObjectId. See event.rs's doc comment on ZoneChange for the full case.
/// Not handled this slice: an ability whose source no longer resolves via
/// `state.object()` (e.g. it just left the battlefield) is skipped rather
/// than evaluated against last-known information - only ETB-shaped
/// conditions (source still on the battlefield when checked) are exercised.
///
/// IMPORTANT (CR 601.2i): a "whenever you cast a spell" condition must
/// NOT match on GameEventKind::StackObjectPushed alone - that event
/// represents CR 601.2a's *proposal* (card moved to stack), not the
/// spell actually becoming cast, which only happens once its cost is
/// paid (601.2i). There's no dedicated "spell was cast" event yet -
/// either add one (e.g. emitted alongside/after GameEventKind::CostPaid
/// in engine.rs's submit_payment, only for StackEntryKind::Spell), or
/// have this condition check require both StackObjectPushed AND a
/// matching CostPaid for the same stack_id before it's considered a
/// completed cast. Timing happens to work out correctly today regardless
/// (this function isn't invoked until CollectTriggers, which never runs
/// until after the ProcessState::AwaitingPayment pause resolves - see
/// engine.rs) - but matching the wrong event would still be semantically
/// wrong (e.g. would also incorrectly fire for a spell whose cost was
/// never actually paid, if 601.6's rollback gap ever gets fixed and a
/// proposed-but-uncast spell becomes representable).
pub fn collect_triggers(
    state: &GameState,
    event: &GameEvent,
    runtime: &ScriptRuntime,
) -> Vec<PendingTrigger> {
    if !state.abilities.iter().any(|a| a.kind == AbilityKind::Triggered) {
        // Genuinely nothing to match against - safe to no-op rather than
        // hit the invocation logic below, which only matters once a
        // *triggered* RegisteredAbility actually exists. Activated/static
        // abilities (e.g. CR 605 mana abilities) don't fire off events at
        // all, so their presence alone shouldn't force this function past
        // its no-op path.
        return Vec::new();
    }

    let mut pending = Vec::new();
    // `ability_index` (PendingTrigger's field) is "the nth ability
    // registered to this source, any kind, in registration order" - same
    // convention `commands::find_ability` uses for ActivateAbility. Tracked
    // per-source as we walk state.abilities in order, rather than a global
    // index, since that's the contract find_ability's lookup relies on.
    let mut per_source_count: HashMap<ObjectId, u32> = HashMap::new();
    for ability in state.abilities.iter() {
        let count = per_source_count.entry(ability.source).or_insert(0);
        let this_index = *count;
        *count += 1;

        if ability.kind != AbilityKind::Triggered {
            continue;
        }
        let Some(source_object) = state.object(ability.source) else {
            // Source no longer resolves (e.g. it left the battlefield) -
            // see the CR 603.10 note above for why this is a real gap, not
            // fixed here.
            continue;
        };
        let controller = source_object.controller;

        let matched = match &ability.condition {
            None => true,
            // Rust-native fast path - zero Lua VM calls, see
            // ConditionPrimitive::matches's own doc comment.
            Some(ConditionSpec::Primitive(primitive)) => {
                primitive.matches(ability.source, &event.kind)
            }
            Some(ConditionSpec::Script(handle)) => {
                let ctx = ScriptContext {
                    state,
                    source: ability.source,
                    controller,
                    event: Some(event),
                    chosen_targets: Vec::new(),
                };
                // TODO: a script error here silently becomes "didn't
                // match" rather than being surfaced to the caller - no
                // error-reporting path exists from CollectTriggers back to
                // a StepStatus yet.
                runtime.invoke_condition(*handle, &ctx).unwrap_or(false)
            }
        };
        if matched {
            pending.push(PendingTrigger { source: ability.source, controller, ability_index: this_index });
        }
    }
    pending
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
