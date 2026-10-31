//! Zone-move proposal and commit helpers, cost-payment application, and combat
//! damage application. See `DESIGN.md` for the rest.

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

/// CR 601.2h/616.1/514.1: a card moving to a graveyard or exile (a `Sacrifice`,
/// `Discard` or `Exile` cost payment, or CR 514.1's cleanup discard) is a
/// replaceable zone-change proposal. This builds the proposed event only, so
/// callers can replacement-check it before `commit_zone_move`. Callers are
/// `Engine::continue_cost_zone_moves` and `Engine::continue_discard_moves`; see
/// their doc comments for the pipeline. `to` is a parameter because
/// `AdditionalCost::ExileFromGraveyard` proposes Graveyard to Exile through the
/// same path.
pub(crate) fn propose_zone_move(state: &GameState, id: ObjectId, from: ZoneKind, to: ZoneKind) -> Option<GameEventKind> {
    let last_known_info = state.object(id)?.clone();
    Some(GameEventKind::ZoneChange { object: id, from, to, last_known_info: Some(last_known_info) })
}

/// CR 504.1/616.1: drawing a card is a replaceable zone-change proposal (Library
/// to Hand), split from its commit as in `propose_zone_move`.
/// `Engine::commit_zone_move` already handles the commit, because its zone-owner
/// derivation treats Library and Hand as per-player.
///
/// CR 704.5b: an empty library sets `PlayerState.drew_from_empty_library` here,
/// which is why this takes `&mut GameState` where `propose_zone_move` takes
/// `&GameState`.
///
/// CR 121.2b: `Engine::continue_draw_cards` and the turn-based `Step::Draw` both
/// go through here, so a "can't draw more than N cards each turn" limit belongs
/// here. Returning `None` once `player`'s `DrawLimitModifier` limit is met takes
/// the same no-draw path as an empty library, and `continue_draw_cards` stops
/// its per-card loop on it, which CR 121.2b allows (multi-card draws may be
/// partly carried out).
pub(crate) fn propose_draw(state: &mut GameState, player: PlayerId) -> Option<GameEventKind> {
    if let Some(limit) = crate::types::player::effective_draw_limit(&state.draw_limit_modifiers, player)
        && crate::types::player::cards_drawn_this_turn(state, player) >= limit {
            return None;
        }
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

/// Commits `event`, a `propose_zone_move` proposal already replacement-checked
/// by the caller (`to` may have been rewritten by a `ReplaceZoneDestination`).
/// CR 400.7 gives a fresh `ObjectId`, and `retire_object` purges the old id's
/// abilities and effects (no remap follows a sacrifice or discard, as with
/// SBA-driven death).
///
/// The owner for the `state.zone_mut` lookup comes from the zone kind:
/// Battlefield and Exile are shared (`None`), everything else is per-player.
/// Both sides are derived because a source zone can be shared too (Battlefield,
/// for a sacrifice).
///
/// Returns the departed `Triggered` abilities with the finished `GameEvent`
/// (CR 603.10), so `Dies` and `LeavesBattlefield` fire for a `Sacrifice` as for
/// an SBA death, and `DiscardsCard`, `EachDiscardsCard` and `Discarded` fire for
/// a `Discard`.
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
    if from == ZoneKind::Battlefield {
        new_object.reset_for_new_object();
    }

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

/// Performs the side effect of every immediate `CostPayment` in a validated
/// `PaymentPlan`. `PaymentPlan::validate` must have confirmed the plan first, and
/// this does not recheck it, as `cost::spend` does for mana. `Sacrifice` and
/// `Discard` are not handled here: they are zone-change proposals subject to
/// replacement checking (CR 601.2h/616.1), so `Engine::submit_payment` drives
/// them through `Engine::continue_cost_zone_moves`.
///
/// Returns the `GameEventKind`s for `Engine::submit_payment` to append to the
/// ledger.
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
            CostPayment::Sacrifice(_) | CostPayment::Discard(_) | CostPayment::Exile(_) => {
                // Handled by Engine::continue_cost_zone_moves instead.
            }
            CostPayment::Life(amount) => {
                if let Some(player) = state.player_mut(payer) {
                    player.lose_life(*amount as i32);
                }
                events.push(GameEventKind::LifeChanged { player: payer, delta: -(*amount as i32) });
            }
            CostPayment::Other { .. } => {
                // PaymentPlan::validate never accepts a plan with this variant (see
                // AdditionalCost::Other), so this is a no-op and not a panic if one slips
                // through.
            }
        }
    }
    events
}

/// The `PendingZoneMove` entries (`Engine::continue_cost_zone_moves`'s queue):
/// every `Sacrifice`, `Discard` and `Exile` payment in `payments` as an
/// `(id, source zone, destination zone)` triple for `propose_zone_move`. A
/// separate pass from `commit_payment` because it must run before any
/// replacement-checked commit; `PaymentPlan::validate` already confirmed each id
/// is in the zone it claims.
pub(crate) fn collect_cost_zone_moves(payments: &[CostPayment]) -> Vec<(ObjectId, ZoneKind, ZoneKind)> {
    payments
        .iter()
        .filter_map(|payment| match payment {
            CostPayment::Sacrifice(id) => Some((*id, ZoneKind::Battlefield, ZoneKind::Graveyard)),
            CostPayment::Discard(id) => Some((*id, ZoneKind::Hand, ZoneKind::Graveyard)),
            CostPayment::Exile(id) => Some((*id, ZoneKind::Graveyard, ZoneKind::Exile)),
            _ => None,
        })
        .collect()
}

/// Performs the effect of one committed `GameEventKind::DamageDealt`
/// (CR 510.2/119/702.15/702.80/702.90): marks damage on a creature or makes a
/// player lose life, applying Lifelink, Wither, Infect and Deathtouch from the
/// source's effective keywords (CR 613 layer 6, so granted keywords count).
///
/// `CommitEvents` calls it once `event` is in its final post-replacement form
/// (CR 614-616), because `amount` may have been reduced to 0 or `target`
/// redirected in between. `AssignCombatDamage` only proposes `DamageDealt` events
/// and must not mutate `damage_marked` or life itself, or a prevention or
/// redirection would rewrite the ledger without stopping the mutation.
pub(crate) fn apply_damage_event(
    state: &mut GameState,
    script_runtime: &ScriptRuntime,
    source: ObjectId,
    target: Target,
    amount: u32,
    is_combat_damage: bool,
) {
    // CR 702.15/702.80/702.90
    // Looked up once up front so the borrow ends before the target mutations below.
    let source_keywords = effective_keywords(state, script_runtime, source);
    let has_lifelink = source_keywords.contains(KeywordSet::LIFELINK);
    let replaces_with_counters = source_keywords.intersects(KeywordSet::WITHER | KeywordSet::INFECT);
    let is_infect = source_keywords.contains(KeywordSet::INFECT);
    let has_deathtouch = source_keywords.contains(KeywordSet::DEATHTOUCH);
    // CR 702.15b
    // Lifelink gains life for whoever controls the source now
    // (layers::effective_controller), not whoever controlled it when the damage was
    // dealt.
    let source_controller = state
        .object(source)
        .map(|_| crate::layers::effective_controller(state, script_runtime, source));
    let source_is_commander = state.object(source).map(|o| o.is_commander).unwrap_or(false);

    match target {
        Target::Player(pid) => {
            // CR 903.10a
            // Commander combat damage to a player accumulates whether it becomes life loss
            // or, with infect, poison, so this runs before that branch and not inside it.
            // Keyed by name and not ObjectId (see PlayerState::commander_damage_taken). The
            // name is read before source_is_commander's borrow of `state` ends, so it
            // survives into the `player_mut` borrow below.
            if is_combat_damage && source_is_commander && amount > 0
                && let Some(name) = state.object(source).map(|o| o.definition.name.clone())
                    && let Some(player) = state.player_mut(pid) {
                        match player.commander_damage_taken.iter_mut().find(|(s, _)| *s == name) {
                            Some((_, total)) => *total += amount,
                            None => player.commander_damage_taken.push((name, amount)),
                        }
                    }
            // CR 702.90b
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
            // TODO: damage to planeswalkers and battles marks loyalty or defense loss and not damage_marked, and GameObject has no loyalty or defense damage path, so only creature targets work (CR 120.3c/d)
            let timestamp = state.allocate_sequence_no();
            if let Some(object) = state.object_mut(id)
                && object.definition.card_types.contains(CardTypeSet::CREATURE) {
                    // CR 702.80a/702.90c
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
        // CR 702/119: damage is dealt to a permanent or a player, never to an ability.
        Target::Ability(_) => unreachable!("damage is never dealt to an ability"),
    }

    // CR 702.15b
    // Lifelink's gain is in addition to any other result of the damage, to a player
    // or an object alike.
    if has_lifelink && amount > 0
        && let Some(controller) = source_controller
            && let Some(player) = state.player_mut(controller) {
                player.lose_life(-(amount as i32));
            }
}
