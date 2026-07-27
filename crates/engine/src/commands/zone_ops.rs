//! Zone-move proposal/commit helpers, cost-payment application, and combat
//! damage application - split out of the original monolithic `commands.rs`.

use crate::script::ScriptRuntime;
use crate::types::ability::RegisteredAbility;
use crate::types::card::{CardTypeSet, KeywordSet};
use crate::types::cost::{self, CostPayment};
use crate::types::counter::CounterKind;
use crate::types::event::{GameEvent, GameEventKind};
use crate::types::game::GameState;
use crate::types::ids::{ObjectId, ObjectLineage, PlayerId, Target};
use crate::types::zone::ZoneKind;

use super::attach::retire_object;
use super::legality::effective_keywords;

/// CR 601.2h/616.1/514.1: a card moving to a graveyard - whether via a
/// `Sacrifice`/`Discard` cost payment or CR 514.1's cleanup hand-size
/// discard - is itself a replaceable zone-change proposal ("if a permanent
/// you control would be sacrificed, exile it instead" is a real, if
/// unusual, card pattern). This builds the *proposed* event only, mirroring
/// `sba::resolve_deaths`'/`Engine::continue_scripted_zone_move`'s own
/// "propose, then replacement-check, then commit" split rather than
/// mutating directly the way this function's predecessor
/// (`move_to_graveyard_for_cost`, folded into `commit_payment` itself) used
/// to. Two callers: `Engine::continue_cost_zone_moves` (cost payments) and
/// `Engine::continue_discard_moves` (cleanup discard, `phase5/replaceable-
/// cleanup-discard`) - see either's own doc comment for its full pipeline.
pub(crate) fn propose_zone_move(state: &GameState, id: ObjectId, from: ZoneKind) -> Option<GameEventKind> {
    let last_known_info = state.object(id)?.clone();
    Some(GameEventKind::ZoneChange { object: id, from, to: ZoneKind::Graveyard, last_known_info: Some(last_known_info) })
}

/// CR 504.1/616.1: drawing a card is itself a replaceable zone-change
/// proposal (e.g. Notion Thief-style "you draw instead" effects) - same
/// "propose, then replacement-check, then commit" split `propose_zone_move`
/// established, generalized to `to: Hand` (the other real destination a
/// zone-move proposal in this codebase starts with, alongside
/// `propose_zone_move`'s own `Graveyard`). `Engine::commit_zone_move`
/// already handles the commit side correctly with no changes needed - its
/// zone-owner derivation treats Library/Hand as per-player, same as every
/// zone besides Battlefield/Exile.
///
/// CR 704.5b: an empty library still sets `PlayerState.drew_from_empty_library`
/// here (not itself a zone-change) - the same flag the old direct-mutation
/// `Engine::draw_one_card` used to set. `Game.Ops.DrawCards(n)`'s
/// `ResolveStackTop` arm now routes through this function + `commit_zone_move`
/// (see `Engine::continue_draw_cards`). Takes `&mut GameState` rather than
/// `propose_zone_move`'s `&GameState` for exactly this one side effect -
/// nothing else about this function mutates state.
pub(crate) fn propose_draw(state: &mut GameState, player: PlayerId) -> Option<GameEventKind> {
    let card = state.zone(ZoneKind::Library, Some(player)).and_then(|library| library.contents.front().copied());
    let Some(card) = card else {
        if let Some(p) = state.player_mut(player) {
            p.drew_from_empty_library = true;
        }
        return None;
    };
    let last_known_info = state.object(card).expect("card id came from the library zone's own contents").clone();
    Some(GameEventKind::ZoneChange { object: card, from: ZoneKind::Library, to: ZoneKind::Hand, last_known_info: Some(last_known_info) })
}

/// Commits `event` (a `propose_zone_move` proposal, already
/// replacement-checked by the caller — `to` may have been rewritten by a
/// `ReplaceZoneDestination` payload) — CR 400.7 fresh `ObjectId`, same
/// convention every other zone move in this codebase uses, and purges any
/// ability/continuous-effect the old id left behind (`retire_object` - no
/// remap follows a sacrifice/discard, same as a SBA-driven death).
///
/// `from`/`to`'s owner for the `state.zone_mut` lookup is derived from the
/// zone kind, not carried separately - Battlefield/Exile are shared zones
/// (owner: `None`), everything else (Hand, Graveyard, ...) is per-player -
/// same convention `Engine::commit_scripted_zone_move`'s own Exile arm
/// already established for its destination-side lookup, mirrored here for
/// both sides since a source zone can be shared too (Battlefield, for a
/// sacrifice).
///
/// Returns the departed `Triggered`-kind abilities alongside the finished
/// `GameEvent` (CR 603.10) — for a `Sacrifice` (Battlefield→Graveyard, the
/// common case), this is what lets `Dies`/`LeavesBattlefield` fire the same
/// way they do off an SBA-driven death. For a `Discard` (Hand→Graveyard),
/// the departed abilities let `DiscardsCard`/`EachDiscardsCard`
/// (player-relative) and `Discarded` (self-referential,
/// `phase5/discarded-trigger`) fire the same way.
pub(crate) fn commit_zone_move(state: &mut GameState, event: GameEventKind) -> (GameEvent, Vec<RegisteredAbility>) {
    let GameEventKind::ZoneChange { object: old_id, from, to, last_known_info } = event else {
        unreachable!("propose_zone_move only ever proposes a ZoneChange");
    };
    let last_known_info = last_known_info.expect("propose_zone_move always sets last_known_info");
    let owner = last_known_info.owner;

    let new_id = state.allocate_object_id();
    let mut new_object = last_known_info.clone();
    new_object.object_id = new_id;
    new_object.lineage = ObjectLineage { previous: Some(old_id) };
    new_object.zone = to;
    new_object.timestamp = state.allocate_sequence_no();

    let from_owner = match from {
        ZoneKind::Battlefield | ZoneKind::Exile => None,
        _ => Some(owner),
    };
    if let Some(zone) = state.zone_mut(from, from_owner) {
        zone.remove(old_id);
    }
    let departed_triggered = retire_object(state, old_id);
    let to_owner = match to {
        ZoneKind::Battlefield | ZoneKind::Exile => None,
        _ => Some(owner),
    };
    if let Some(zone) = state.zone_mut(to, to_owner) {
        zone.push_bottom(new_id);
    }
    state.objects.insert(new_id, new_object);

    let event_id = state.allocate_event_id();
    let sequence_no = state.allocate_sequence_no();
    let committed = GameEvent {
        event_id,
        sequence_no,
        kind: GameEventKind::ZoneChange { object: new_id, from, to, last_known_info: Some(last_known_info) },
        caused_by_command: None,
        parent_event: None,
    };
    (committed, departed_triggered)
}

/// Performs the real side effect of every *immediate* `CostPayment` in an
/// already-validated `PaymentPlan` (`PaymentPlan::validate` must have
/// confirmed the whole plan first — same "trusts validate, doesn't recheck"
/// contract `cost::spend` documents for mana). `Sacrifice`/`Discard` are
/// deliberately not handled here — unlike mana/tap/life, they're zone-change
/// proposals, themselves subject to CR 601.2h/616.1 replacement checking
/// (see `propose_zone_move`'s own doc comment) — `Engine::submit_payment`
/// collects them separately and drives them through `Engine::
/// continue_cost_zone_moves` instead of this function.
///
/// Returns the `GameEventKind`s for the caller (`Engine::submit_payment`) to
/// append to the ledger — same "commands decide, engine.rs writes the
/// ledger" split every other command path in this file follows.
pub(crate) fn commit_payment(state: &mut GameState, payer: PlayerId, payments: &[CostPayment]) -> Vec<GameEventKind> {
    let mut events = Vec::new();
    for payment in payments {
        match payment {
            CostPayment::Mana { units, phyrexian_life } => {
                if let Some(player) = state.player_mut(payer) {
                    cost::spend(units, &mut player.mana_pool);
                    if *phyrexian_life > 0 {
                        player.lose_life(*phyrexian_life as i32);
                    }
                }
                if *phyrexian_life > 0 {
                    events.push(GameEventKind::LifeChanged {
                        player: payer,
                        delta: -(*phyrexian_life as i32),
                    });
                }
            }
            CostPayment::Tap(id) => {
                if let Some(obj) = state.object_mut(*id) {
                    obj.tapped = true;
                }
                events.push(GameEventKind::Tapped { object: *id });
            }
            CostPayment::Sacrifice(_) | CostPayment::Discard(_) => {
                // Handled by Engine::continue_cost_zone_moves instead - see
                // this function's own doc comment.
            }
            CostPayment::Life(amount) => {
                if let Some(player) = state.player_mut(payer) {
                    player.lose_life(*amount as i32);
                }
                events.push(GameEventKind::LifeChanged { player: payer, delta: -(*amount as i32) });
            }
            CostPayment::Other { .. } => {
                // PaymentPlan::validate never accepts a plan containing
                // this variant (see AdditionalCost::Other's doc comment) -
                // unreachable in practice, no-op rather than panicking if
                // it somehow got here anyway.
            }
        }
    }
    events
}

/// `PendingZoneMove` (`Engine::continue_cost_zone_moves`'s own queue
/// entry) - collects every `Sacrifice`/`Discard` payment in `payments` into
/// a `(id, source zone)` pair, ready for `propose_zone_move`. Kept as a
/// standalone pass (not folded into `commit_payment` above) since it must
/// run *before* any replacement-checked commit happens - `PaymentPlan::
/// validate` has already confirmed every id here really is in the zone it
/// claims to be.
pub(crate) fn collect_cost_zone_moves(payments: &[CostPayment]) -> Vec<(ObjectId, ZoneKind)> {
    payments
        .iter()
        .filter_map(|payment| match payment {
            CostPayment::Sacrifice(id) => Some((*id, ZoneKind::Battlefield)),
            CostPayment::Discard(id) => Some((*id, ZoneKind::Hand)),
            _ => None,
        })
        .collect()
}

/// Performs the real effect of one already-committed `GameEventKind::
/// DamageDealt` event (CR 510.2/119/702.15/702.80/702.90) — mark damage on
/// a creature or lose life on a player, folding in Lifelink/Wither/Infect/
/// Deathtouch off the source's effective keywords (CR 613 layer 6, so a
/// granted keyword applies here too, not just the printed `KeywordSet`).
///
/// Called from `engine.rs`'s `CommitEvents` once `event` is in its final,
/// post-replacement form — CR 614-616 says damage is itself a replaceable/
/// preventable event, so this can't run any earlier than that (`amount`
/// might have been reduced to 0, or `target` redirected, by a
/// `ReplacementEffect` in between the moment `AssignCombatDamage` proposed
/// it and the moment it's actually committed). `AssignCombatDamage` itself
/// (this file) only ever *proposes* `DamageDealt` events now — it must not
/// also mutate `damage_marked`/life directly, or a prevention/redirection
/// effect would rewrite the ledger's record of what happened without
/// stopping the mutation that already happened.
pub(crate) fn apply_damage_event(
    state: &mut GameState,
    script_runtime: &ScriptRuntime,
    source: ObjectId,
    target: Target,
    amount: u32,
    is_combat_damage: bool,
) {
    // CR 702.15/702.80/702.90: looked up once, up front, so the borrow ends
    // before the target mutations below (which may need a mutable borrow of
    // the same GameState).
    let source_keywords = effective_keywords(state, script_runtime, source);
    let has_lifelink = source_keywords.contains(KeywordSet::LIFELINK);
    let replaces_with_counters = source_keywords.intersects(KeywordSet::WITHER | KeywordSet::INFECT);
    let is_infect = source_keywords.contains(KeywordSet::INFECT);
    let has_deathtouch = source_keywords.contains(KeywordSet::DEATHTOUCH);
    // CR 702.15b: lifelink's life gain goes to whoever *currently* controls
    // the damage source (layers::effective_controller), not whoever
    // controlled it when the damage was dealt if a GainControl effect has
    // since changed hands - though in practice these are the same instant.
    let source_controller = state
        .object(source)
        .map(|_| crate::layers::effective_controller(state, script_runtime, source));
    let source_is_commander = state.object(source).map(|o| o.is_commander).unwrap_or(false);

    match target {
        Target::Player(pid) => {
            // CR 903.10a: a commander's *combat* damage to a player
            // accumulates regardless of how that damage otherwise manifests
            // (life loss below, or poison via infect) - a parallel tracking
            // system, not a redirect of the life-loss/poison branching, so
            // this runs unconditionally before that branch rather than
            // inside either arm of it. TODO: keyed by this specific
            // ObjectId, which changes across a CR 903.9a return
            // (cast/die/return/recast) - a recast commander's damage count
            // resets to 0 under this key instead of accumulating against
            // "the same commander" the way CR 903.10a's own text means it.
            // Pre-existing field (item 10's slice), not changed here - see
            // docs/implementation.md's phase5/commander-damage slice notes
            // for why this was deliberately left as-is rather than widened
            // to name-keying like PlayerState::commander_casts.
            if is_combat_damage && source_is_commander && amount > 0 {
                if let Some(player) = state.player_mut(pid) {
                    match player.commander_damage_taken.iter_mut().find(|(s, _)| *s == source) {
                        Some((_, total)) => *total += amount,
                        None => player.commander_damage_taken.push((source, amount)),
                    }
                }
            }
            // CR 702.90b: Infect damage to a player becomes poison
            // counters, not life loss.
            if is_infect {
                let timestamp = state.allocate_sequence_no();
                if let Some(player) = state.player_mut(pid) {
                    player.counters.add(CounterKind::Poison, amount as i32, timestamp);
                }
            } else if let Some(player) = state.player_mut(pid) {
                player.lose_life(amount as i32);
            }
        }
        Target::Object(id) => {
            // TODO(planeswalker/battle damage, CR 120.3c/d): damage to
            // those marks loyalty/defense loss, not damage_marked -
            // GameObject has no loyalty/defense-damage path wired yet, so
            // only creature targets are functional this slice.
            let timestamp = state.allocate_sequence_no();
            if let Some(object) = state.object_mut(id) {
                if object.definition.card_types.contains(CardTypeSet::CREATURE) {
                    // CR 702.80a/702.90c: Wither/Infect damage to a
                    // creature becomes -1/-1 counters instead of marked
                    // damage.
                    if replaces_with_counters {
                        object.counters.add(CounterKind::MinusOneMinusOne, amount as i32, timestamp);
                    } else {
                        object.damage_marked += amount;
                        if has_deathtouch && amount > 0 {
                            object.deathtouched = true;
                        }
                    }
                }
            }
        }
    }

    // CR 702.15b: Lifelink's life gain happens "in addition to any other
    // results that damage causes" - applies to damage dealt to a player or
    // an object alike.
    if has_lifelink && amount > 0 {
        if let Some(controller) = source_controller {
            if let Some(player) = state.player_mut(controller) {
                player.lose_life(-(amount as i32));
            }
        }
    }
}
