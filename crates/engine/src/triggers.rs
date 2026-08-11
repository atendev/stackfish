use crate::script::{ScriptContext, ScriptRuntime};
use crate::types::ability::{
    AbilityKind, ConditionPrimitive, ConditionSpec, InterveningIfSpec, RegisteredAbility,
};
use crate::types::event::{GameEvent, GameEventKind};
use crate::types::game::GameState;
use crate::types::ids::{AbilityId, ObjectId, PlayerId};

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
    /// The triggered `RegisteredAbility`'s own stable `AbilityId` — replaces
    /// the old positional "the nth ability registered to this source"
    /// indexing (`phase5/ability-stable-id`). Needed because this value is
    /// computed here at collection time but consulted later, after an
    /// `AwaitingTriggerOrderChoice` pause and potentially other intervening
    /// game actions, by which point a position could have gone stale in a
    /// way an id can't (see `AbilityId`'s own doc comment).
    pub ability: AbilityId,
    /// CR 603.2h: whether this match came from a `ConditionSpec::
    /// PrimitiveOncePerTurn` condition — `Engine`'s `CollectTriggers` stage
    /// reads this to know which `AbilityId`s to record into
    /// `GameState::once_per_turn_triggered` once this trigger is actually
    /// collected, so a second qualifying event later in the same turn (even
    /// within the same batch) is correctly excluded.
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
    for ability in state.abilities.iter() {
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
        // CR 707.9g: an ability granted by a copy effect that's since been
        // superseded by *another* copy effect on the same object no longer
        // triggers - see GameState::linked_ability_generation's own doc
        // comment. No producer tags an ability yet, so this is a no-op for
        // every real card today; the check exists so it's correct the
        // moment one does.
        if let Some(&granted_generation) = state.linked_ability_generation.get(&ability.id) {
            if source_object.copy_generation != granted_generation {
                continue;
            }
        }
        let controller = source_object.controller;

        let (matched, once_per_turn) = match &ability.condition {
            None => (true, false),
            // Rust-native fast path - zero Lua VM calls, see
            // ConditionPrimitive::matches's own doc comment.
            Some(ConditionSpec::Primitive(primitive)) => {
                (primitive.matches(ability.source, controller, state.active_player, &event.kind), false)
            }
            // CR 603.2h: same matching as Primitive above, but excluded if
            // this ability already triggered once this turn - see
            // GameState::once_per_turn_triggered's own doc comment.
            Some(ConditionSpec::PrimitiveOncePerTurn(primitive)) => {
                let already = state.once_per_turn_triggered.contains(&ability.id);
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
            Some(InterveningIfSpec::Primitive(p)) => p.holds(state, controller, ability.source, None),
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
            pending.push(PendingTrigger { source: ability.source, controller, ability: ability.id, once_per_turn });
        }
    }

    // CR 603.10 last-known-information fallback (see this function's own
    // doc comment above) - `departed` abilities can never appear in the
    // loop above (retire_object already purged them from state.abilities by
    // the time this runs), so this is a genuinely separate source, not a
    // duplicate scan. `ability.id` is the same stable id it always had while
    // still live - `PutTriggersApnap`'s fallback lookup (engine.rs) walks
    // `departed` matching on that same id to resolve a `PendingTrigger` back
    // into a `RegisteredAbility`.
    for ability in departed {
        // Only Dies/LeavesBattlefield/Discarded/Cycled are meaningful for a
        // source that no longer resolves live - every other condition shape
        // (None/Always/DealsDamage/a Script condition/...) either assumes a
        // live source or isn't scoped to this slice (see the TODO on
        // ConditionPrimitive itself). Discarded joined this list in
        // `phase5/discarded-trigger` - a discarded card's own abilities are
        // purged from `state.abilities` by `retire_object` exactly like a
        // battlefield departure's, so it needs the same LKI fallback. Cycled
        // joined it in `phase5/cycling-triggers` for the exact same reason -
        // a cycled card's own "when you cycle this card" ability is purged
        // the moment it leaves hand too.
        let (primitive, once_per_turn) = match &ability.condition {
            Some(ConditionSpec::Primitive(
                primitive @ (ConditionPrimitive::Dies
                | ConditionPrimitive::LeavesBattlefield
                | ConditionPrimitive::Discarded
                | ConditionPrimitive::Cycled
                | ConditionPrimitive::MadnessExiled),
            )) => (primitive, false),
            // CR 603.2h applies here too - a departed object's own Dies/
            // LeavesBattlefield/Discarded/Cycled/MadnessExiled trigger is
            // just as capable of carrying "This ability triggers only once
            // each turn" as a live one's.
            Some(ConditionSpec::PrimitiveOncePerTurn(
                primitive @ (ConditionPrimitive::Dies
                | ConditionPrimitive::LeavesBattlefield
                | ConditionPrimitive::Discarded
                | ConditionPrimitive::Cycled
                | ConditionPrimitive::MadnessExiled),
            )) => (primitive, true),
            _ => continue,
        };
        // Dies/LeavesBattlefield/Discarded all read their LKI off
        // ZoneChange; Cycled/MadnessExiled are separate marker events with
        // their own last_known_info field (same shape, different variant) -
        // extracted generically here so the rest of this loop (controller/
        // intervening-if/PendingTrigger) doesn't need to care which.
        let lki = match &event.kind {
            GameEventKind::ZoneChange { last_known_info: Some(lki), .. } => lki,
            GameEventKind::Cycled { last_known_info, .. } => last_known_info,
            GameEventKind::MadnessExiled { last_known_info, .. } => last_known_info,
            _ => continue,
        };
        if lki.object_id != ability.source {
            continue;
        }
        if once_per_turn && state.once_per_turn_triggered.contains(&ability.id) {
            continue;
        }
        // CR 603.4 applies here too - see the main loop's own comment above.
        let intervening_if_holds = match &ability.intervening_if {
            None => true,
            Some(InterveningIfSpec::Primitive(p)) => p.holds(state, lki.controller, ability.source, Some(lki)),
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
                ability: ability.id,
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
    // Partition triggers by controller in apnap_player_order's order.
    // Intra-group order is deliberately left as collect_triggers produced it
    // - that's the controller's choice (CR 603.3b), resolved by
    // engine.rs/AwaitTriggerOrderChoice, not here.
    apnap_player_order(state)
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

/// CR 101.4's APNAP order as a bare player list: `state.turn_order` rotated
/// to start at `state.active_player`, then every other player in turn order.
/// Factored out of `group_apnap` (`phase5/mass-choice-effects`) so `engine::
/// resolve::each_player::resolve_for_each_player_effect`'s own CR 101.4
/// "each player ..." loop can reuse the identical rotation instead of a
/// second copy — `group_apnap` itself now just partitions triggers by
/// controller in this order.
pub fn apnap_player_order(state: &GameState) -> Vec<PlayerId> {
    let mut order: Vec<PlayerId> = Vec::new();
    match state.turn_order.iter().position(|p| *p == state.active_player) {
        Some(start) => {
            order.extend(state.turn_order[start..].iter().copied());
            order.extend(state.turn_order[..start].iter().copied());
        }
        None => order.extend(state.turn_order.iter().copied()),
    }
    order
}

/// CR 603.4/608.2a: re-derives a departed source's own last-known-info at
/// `push_trigger_stack_object` time (later than `collect_triggers` itself,
/// possibly after an `AwaitingTriggerOrderChoice` pause), for
/// `StackObject::source_lki` — needed so `InterveningIfPrimitive::
/// HadNoCountersOfKind`'s own CR 608.2a resolution recheck has something to
/// read (a `Dies`-sourced `source` no longer resolves live by then, same
/// CR 603.10 problem the collection-time check already has to solve).
/// Deliberately not a new `PendingTrigger`/`Engine` field carrying the
/// snapshot forward - `PendingTrigger` derives `Copy`/`PartialEq`
/// (`is_permutation`/`group_apnap` both rely on it), and a `GameObject`
/// can't cheaply support either; the exact same event this trigger was
/// collected from (its `last_known_info`, keyed by the now-departed
/// `source` id, which is never reused per CR 400.7) is already sitting in
/// `state.ledger` by this point, so a reverse scan reusing `collect_triggers`'
/// own departed-loop match shape (`ZoneChange`/`Cycled`, matched by
/// `object_id`) finds it directly instead.
pub fn find_departed_lki(state: &GameState, source: ObjectId) -> Option<crate::types::object::GameObject> {
    state.ledger.iter().rev().find_map(|event| match &event.kind {
        GameEventKind::ZoneChange { last_known_info: Some(lki), .. } if lki.object_id == source => {
            Some(lki.clone())
        }
        GameEventKind::Cycled { last_known_info, .. } if last_known_info.object_id == source => {
            Some(last_known_info.clone())
        }
        GameEventKind::MadnessExiled { last_known_info, .. } if last_known_info.object_id == source => {
            Some(last_known_info.clone())
        }
        _ => None,
    })
}
