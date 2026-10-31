use crate::script::{ScriptContext, ScriptRuntime};
use crate::types::ability::{
    AbilityKind, ConditionPrimitive, ConditionSpec, InterveningIfSpec, RegisteredAbility,
};
use crate::types::event::{GameEvent, GameEventKind};
use crate::types::game::GameState;
use crate::types::ids::{AbilityId, ObjectId, PlayerId};

/// A triggered ability that matched an event and awaits placement on the
/// stack. Transient pipeline state, not part of `GameState`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PendingTrigger {
    pub source: ObjectId,
    pub controller: PlayerId,
    /// Stable id, because the match is consulted after a possible trigger-order
    /// pause, by which time a position index could be stale.
    pub ability: AbilityId,
    /// CR 603.2h: set when the match came from a once-per-turn condition, so
    /// collection records it in `once_per_turn_triggered`.
    pub once_per_turn: bool,
    /// CR 702.21a: the stack object that caused the trigger (`BecameTarget::by`).
    /// Ward's counter targets it.
    pub triggering_stack: Option<ObjectId>,
}

/// The stack object an event is "caused by" for `PendingTrigger::
/// triggering_stack`, if it names one.
fn triggering_stack_of(kind: &GameEventKind) -> Option<ObjectId> {
    match kind {
        GameEventKind::BecameTarget { by, .. } => Some(*by),
        _ => None,
    }
}

/// Finds every triggered ability that matches `event` and may trigger now
/// (CR 603.2, 603.3). The result is not yet in APNAP order; see `group_apnap`.
/// Script errors come back as messages because only `&GameState` is held, so
/// the caller logs them.
///
/// CR 603.10: `departed` holds triggered abilities captured off objects that
/// left this run. They match against the event's last-known info instead of a
/// live lookup.
///
/// CR 601.2i: a "whenever you cast" condition matches `SpellCast`, not
/// `StackObjectPushed`, which is only the proposal.
pub fn collect_triggers(
    state: &GameState,
    event: &GameEvent,
    runtime: &ScriptRuntime,
    departed: &[RegisteredAbility],
) -> (Vec<PendingTrigger>, Vec<String>) {
    if !state.abilities.iter().any(|a| a.kind == AbilityKind::Triggered) && departed.is_empty() {
        // No triggered ability exists, so there is nothing to match.
        return (Vec::new(), Vec::new());
    }

    let mut pending = Vec::new();
    // Collected rather than logged: only `&GameState` is available here.
    let mut errors: Vec<String> = Vec::new();
    for ability in state.abilities.iter() {
        if ability.kind != AbilityKind::Triggered {
            continue;
        }
        let Some(source_object) = state.object(ability.source) else {
            // Defensive: retirement purges an ability in the same step its source
            // leaves, so this should not occur.
            continue;
        };
        // CR 707.9g: skip an ability granted by a copy effect that a later copy
        // effect on the same object has superseded.
        if let Some(&granted_generation) = state.linked_ability_generation.get(&ability.id)
            && source_object.copy_generation != granted_generation {
                continue;
            }
        // CR 113.6
        // Defensive: the ability is also deregistered when its source leaves the zone.
        if state.ability_function_zones.get(&ability.id).is_some_and(|z| *z != source_object.zone) {
            continue;
        }
        let controller = source_object.controller;

        let (matched, once_per_turn) = match &ability.condition {
            None => (true, false),
            Some(ConditionSpec::Primitive(primitive)) => {
                (primitive.matches(ability.source, controller, state.active_player, &event.kind), false)
            }
            // CR 603.2h
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
        // CR 603.4: checked at trigger time, and again at resolution (CR 608.2a).
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
            pending.push(PendingTrigger {
                source: ability.source,
                controller,
                ability: ability.id,
                once_per_turn,
                triggering_stack: triggering_stack_of(&event.kind),
            });
        }
    }

    // CR 603.10: departed abilities were purged from `state.abilities`, so the
    // loop above never sees them.
    for ability in departed {
        // Only conditions that read last-known info apply to a source that no longer resolves.
        // TODO: filtered departed conditions such as "whenever a creature you control dies"
        let (primitive, once_per_turn) = match &ability.condition {
            Some(ConditionSpec::Primitive(
                primitive @ (ConditionPrimitive::Dies
                | ConditionPrimitive::LeavesBattlefield
                | ConditionPrimitive::Discarded
                | ConditionPrimitive::Cycled
                | ConditionPrimitive::MadnessExiled),
            )) => (primitive, false),
            // CR 603.2h
            Some(ConditionSpec::PrimitiveOncePerTurn(
                primitive @ (ConditionPrimitive::Dies
                | ConditionPrimitive::LeavesBattlefield
                | ConditionPrimitive::Discarded
                | ConditionPrimitive::Cycled
                | ConditionPrimitive::MadnessExiled),
            )) => (primitive, true),
            _ => continue,
        };
        // Each variant carries its last-known info in a different event shape.
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
        // CR 603.4
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
                triggering_stack: triggering_stack_of(&event.kind),
            });
        }
    }

    (pending, errors)
}

/// CR 603.3b: groups `triggers` by controller in APNAP order. Order inside a
/// group is the controller's choice, made later at the trigger-order pause.
pub fn group_apnap(
    state: &GameState,
    triggers: Vec<PendingTrigger>,
) -> Vec<(PlayerId, Vec<PendingTrigger>)> {
    if triggers.is_empty() {
        return Vec::new();
    }
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

/// CR 101.4: `state.turn_order` rotated to start at the active player.
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

/// CR 603.10: re-derives a departed source's last-known info from the ledger
/// at stack-push time. `PendingTrigger` is `Copy`, so it cannot carry the snapshot.
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
