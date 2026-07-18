use std::collections::HashMap;

use crate::script::{ScriptContext, ScriptRuntime};
use crate::types::ability::{
    AbilityKind, ConditionPrimitive, ConditionSpec, InterveningIfSpec, RegisteredAbility,
};
use crate::types::event::{GameEvent, GameEventKind};
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
    /// CR 603.2h: whether this match came from a `ConditionSpec::
    /// PrimitiveOncePerTurn` condition — `Engine`'s `CollectTriggers` stage
    /// reads this to know which `(source, ability_index)` pairs to record
    /// into `GameState::once_per_turn_triggered` once this trigger is
    /// actually collected, so a second qualifying event later in the same
    /// turn (even within the same batch) is correctly excluded.
    pub once_per_turn: bool,
}

/// Finds every triggered ability that matches `event` and is currently
/// legal to trigger (timing, once-per-turn, range, etc — CR 603.2-603.3).
/// Order in the returned `Vec` is not yet APNAP order; see `group_apnap`.
/// The second return value is every Script condition/intervening-if call
/// that errored during this pass, as plain messages — this function only
/// has `&GameState`, not `&mut Engine`, so it can't log a real
/// `GameEventKind::ScriptError` itself; the caller (`Engine`'s
/// `CollectTriggers` stage) does that once the loop over `state.ledger`
/// finishes (`phase5/script-error-surfacing`). A script error still falls
/// back to the same conservative "didn't match"/"doesn't hold" default as
/// before this slice — only the *visibility* changed.
///
/// `runtime` invokes each ability's `condition` callback (`None` means "no
/// filter, always matches" per `RegisteredAbility::condition`'s doc).
/// CR 603.2h ("this ability triggers only once each turn",
/// `ConditionSpec::PrimitiveOncePerTurn`) is enforced here, against
/// `state.once_per_turn_triggered` — see that field's own doc comment for
/// who populates it. Range-of-influence legality (irrelevant outside
/// multiplayer, backlog item 11) and once-per-turn *cost* restrictions on
/// activated abilities (CR 602.5b, already handled by `commands.rs` at
/// activation time, not a triggered-ability concern) remain out of scope —
/// see the IMPORTANT notes below, carried over unchanged from before this
/// function had a real body.
///
/// IMPORTANT (CR 603.10): a condition checking "did this object leave a
/// zone" consults event.kind's ZoneChange::last_known_info (if Some)
/// instead of looking `object` up live via state.object() - by the time
/// this runs (post-CommitEvents/SbaLoop), a live lookup can miss
/// characteristics the object only had at the moment it left (e.g. an
/// ability granted by an aura that left in the same batch), or the id may
/// not resolve to anything at all if the move minted a new ObjectId. See
/// event.rs's doc comment on ZoneChange for the full case. An ability whose
/// source no longer resolves via `state.object()` (e.g. it just left the
/// battlefield) is skipped by the main loop below - `departed` is the CR
/// 603.10 fallback for exactly that case: `Engine`'s stash of `Triggered`
/// abilities captured (by `commands::retire_object`, via `sba::
/// commit_death`) off objects that died during this pipeline run, *before*
/// they were purged from `state.abilities`. Only `Dies`/`LeavesBattlefield`/
/// `Discarded` conditions are evaluated against it (see the second loop
/// below) - a
/// filtered condition ("whenever a creature you control dies") isn't
/// representable yet, and every other condition shape assumes a source that
/// still resolves live, which a departed object's never will again.
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
    departed: &[RegisteredAbility],
) -> (Vec<PendingTrigger>, Vec<String>) {
    if !state.abilities.iter().any(|a| a.kind == AbilityKind::Triggered) && departed.is_empty() {
        // Genuinely nothing to match against - safe to no-op rather than
        // hit the invocation logic below, which only matters once a
        // *triggered* RegisteredAbility actually exists. Activated/static
        // abilities (e.g. CR 605 mana abilities) don't fire off events at
        // all, so their presence alone shouldn't force this function past
        // its no-op path.
        return (Vec::new(), Vec::new());
    }

    let mut pending = Vec::new();
    // Collected rather than logged directly - this function only has
    // `&GameState`, not `&mut Engine`, so the caller (`Engine`'s
    // `CollectTriggers` stage) is the one that actually turns these into
    // real `GameEventKind::ScriptError` events (`phase5/script-error-
    // surfacing`). A script error still falls back to the same
    // conservative "didn't match"/"doesn't hold" default as before.
    let mut errors: Vec<String> = Vec::new();
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
            // Source no longer resolves live, yet this ability is still
            // sitting in state.abilities - shouldn't happen via any real
            // zone-change path today (every one either calls remap_abilities
            // or retire_object, which purges the ability in the same step
            // the object leaves state.objects - see retire_object's own doc
            // comment), so this is defensive, not the CR 603.10 case: a
            // Dies/LeavesBattlefield ability reaches the `departed` loop
            // below instead, since retire_object already removed it from
            // this very list before collect_triggers ever ran.
            continue;
        };
        let controller = source_object.controller;

        let (matched, once_per_turn) = match &ability.condition {
            None => (true, false),
            // Rust-native fast path - zero Lua VM calls, see
            // ConditionPrimitive::matches's own doc comment.
            Some(ConditionSpec::Primitive(primitive)) => {
                (primitive.matches(ability.source, controller, state.active_player, &event.kind), false)
            }
            // CR 603.2h: same matching as Primitive above, but excluded if
            // this (source, ability_index) already triggered once this turn
            // - see GameState::once_per_turn_triggered's own doc comment.
            Some(ConditionSpec::PrimitiveOncePerTurn(primitive)) => {
                let already = state.once_per_turn_triggered.contains(&(ability.source, this_index));
                (
                    !already && primitive.matches(ability.source, controller, state.active_player, &event.kind),
                    true,
                )
            }
            Some(ConditionSpec::Script(handle)) => {
                let ctx = ScriptContext {
                    state,
                    source: ability.source,
                    controller,
                    event: Some(event),
                    chosen_targets: Vec::new(),
                };
                let matched = match runtime.invoke_condition(*handle, &ctx) {
                    Ok(v) => v,
                    Err(e) => {
                        errors.push(format!(
                            "condition script error on source {:?}: {e:?}",
                            ability.source
                        ));
                        false
                    }
                };
                (matched, false)
            }
        };
        // CR 603.4: the intervening-if clause is checked at trigger time too
        // - a false intervening-if means the ability never triggers at all,
        // same outcome as the trigger-event condition itself not matching.
        // Orthogonal to (and checked in addition to) `matched` above - see
        // `InterveningIfSpec`'s own doc comment. Rechecked again at
        // resolution by `Engine`'s `ResolveStackTop` (CR 608.2a), against
        // whatever `PutTriggersApnap` bakes onto the `StackObject`.
        let intervening_if_holds = match &ability.intervening_if {
            None => true,
            Some(InterveningIfSpec::Primitive(p)) => p.holds(state, controller),
            Some(InterveningIfSpec::Script(handle)) => {
                match runtime.invoke_intervening_if(*handle, state, ability.source, controller) {
                    Ok(v) => v,
                    Err(e) => {
                        errors.push(format!(
                            "intervening-if script error on source {:?}: {e:?}",
                            ability.source
                        ));
                        false
                    }
                }
            }
        };
        if matched && intervening_if_holds {
            pending.push(PendingTrigger { source: ability.source, controller, ability_index: this_index, once_per_turn });
        }
    }

    // CR 603.10 last-known-information fallback (see this function's own
    // doc comment above) - `departed` abilities can never appear in the
    // loop above (retire_object already purged them from state.abilities by
    // the time this runs), so this is a genuinely separate source, not a
    // duplicate scan. `ability_index` here counts "the nth departed ability
    // for this source in `departed`'s own order" - the same convention
    // `PutTriggersApnap`'s fallback lookup (engine.rs) walks `departed` with
    // to resolve a PendingTrigger back into a RegisteredAbility.
    let mut departed_count: HashMap<ObjectId, u32> = HashMap::new();
    for ability in departed {
        let count = departed_count.entry(ability.source).or_insert(0);
        let this_index = *count;
        *count += 1;

        // Only Dies/LeavesBattlefield/Discarded are meaningful for a source
        // that no longer resolves live - every other condition shape (None/
        // Always/DealsDamage/a Script condition/...) either assumes a live
        // source or isn't scoped to this slice (see the TODO on
        // ConditionPrimitive itself). Discarded joined this list in
        // `phase5/discarded-trigger` - a discarded card's own abilities are
        // purged from `state.abilities` by `retire_object` exactly like a
        // battlefield departure's, so it needs the same LKI fallback.
        let (primitive, once_per_turn) = match &ability.condition {
            Some(ConditionSpec::Primitive(
                primitive @ (ConditionPrimitive::Dies
                | ConditionPrimitive::LeavesBattlefield
                | ConditionPrimitive::Discarded),
            )) => (primitive, false),
            // CR 603.2h applies here too - a departed object's own Dies/
            // LeavesBattlefield/Discarded trigger is just as capable of
            // carrying "This ability triggers only once each turn" as a live
            // one's.
            Some(ConditionSpec::PrimitiveOncePerTurn(
                primitive @ (ConditionPrimitive::Dies
                | ConditionPrimitive::LeavesBattlefield
                | ConditionPrimitive::Discarded),
            )) => (primitive, true),
            _ => continue,
        };
        let GameEventKind::ZoneChange { last_known_info: Some(lki), .. } = &event.kind else {
            continue;
        };
        if lki.object_id != ability.source {
            continue;
        }
        if once_per_turn && state.once_per_turn_triggered.contains(&(ability.source, this_index)) {
            continue;
        }
        // CR 603.4 applies here too - see the main loop's own comment above.
        let intervening_if_holds = match &ability.intervening_if {
            None => true,
            Some(InterveningIfSpec::Primitive(p)) => p.holds(state, lki.controller),
            Some(InterveningIfSpec::Script(handle)) => {
                match runtime.invoke_intervening_if(*handle, state, ability.source, lki.controller) {
                    Ok(v) => v,
                    Err(e) => {
                        errors.push(format!(
                            "intervening-if script error on departed source {:?}: {e:?}",
                            ability.source
                        ));
                        false
                    }
                }
            }
        };
        if intervening_if_holds && primitive.matches(ability.source, lki.controller, state.active_player, &event.kind) {
            pending.push(PendingTrigger {
                source: ability.source,
                controller: lki.controller,
                ability_index: this_index,
                once_per_turn,
            });
        }
    }

    (pending, errors)
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
