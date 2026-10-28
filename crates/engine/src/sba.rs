use std::collections::VecDeque;

use crate::commands;
use crate::replacement::{self, ReplacementOutcome};
use crate::script::ScriptRuntime;
use crate::types::ability::RegisteredAbility;
use crate::types::card::{CardTypeSet, KeywordSet, SupertypeSet};
use crate::types::counter::CounterKind;
use crate::types::event::{GameEvent, GameEventKind};
use crate::types::game::GameState;
use crate::types::ids::{ObjectId, ObjectLineage, PlayerId};
use crate::types::stack::StackEntryKind;
use crate::types::zone::ZoneKind;

/// What one `check_state_based_actions` pass produced.
pub enum SbaOutcome {
    /// Events from this pass, for the caller to append to the ledger; empty means
    /// CR 704.3's fixed point. The second element holds the dying objects'
    /// `Triggered` abilities (CR 603.10), captured by `retire_object` before it
    /// purged them, so "dies" triggers can still be found.
    Progressed(Vec<GameEvent>, Vec<RegisteredAbility>),
    /// A dying creature's zone-change replacement (CR 614.13) matched several
    /// effects and `affected_player` must choose the order (CR 616.1).
    /// `resolved_so_far` is committed immediately by the caller; `pending_deaths`
    /// resumes after `Engine::submit_replacement_choice`.
    NeedsOrderingChoice {
        affected_player: PlayerId,
        pending_event: GameEventKind,
        candidates: Vec<crate::replacement::ReplacementCandidate>,
        excluded: Vec<crate::replacement::ReplacementCandidate>,
        /// Whether `pending_event` is genuine destruction (CR 614.8); it was already popped from `pending_deaths`.
        is_destroy: bool,
        pending_deaths: VecDeque<DyingCandidate>,
        resolved_so_far: Vec<GameEvent>,
    },
    /// CR 704.5j: `player` controls two or more legendary permanents with the same
    /// name (`candidates`, sorted) and must keep one. Only the first group pauses;
    /// the caller reruns the pass after the answer to find the next.
    /// `resolved_so_far` is committed immediately.
    NeedsLegendRuleChoice { player: PlayerId, candidates: Vec<ObjectId>, resolved_so_far: Vec<GameEvent> },
    /// CR 903.9a: `commander` (owned by `player`) sits in a graveyard or exile and
    /// has not yet been offered the command-zone choice (`commander_return_offered`).
    /// Only the first by `ObjectId` pauses per pass. `resolved_so_far` is committed immediately.
    NeedsCommanderReturnChoice { player: PlayerId, commander: ObjectId, resolved_so_far: Vec<GameEvent> },
}

/// One pass of CR 704 state-based actions: finds every applicable one, applies
/// them simultaneously (CR 704.3) and returns the resulting events. A dying
/// creature's replacement that needs an ordering choice pauses via
/// `SbaOutcome::NeedsOrderingChoice`.
///
/// Implemented: 704.5a, 704.5b, 704.5c, 704.5d, 704.5f, 704.5g, 704.5j, 704.5k,
/// 704.5m, 704.5n and CR 903.10a. Indestructible exempts 704.5g but not 704.5f
/// (CR 702.12b).
///
/// The engine loops this until it returns empty (CR 704.3), so a caller can
/// observe each pass.
pub fn check_state_based_actions(state: &mut GameState, script_runtime: &ScriptRuntime) -> SbaOutcome {
    let mut events = Vec::new();
    apply_player_loss(state, &mut events);

    // CR 704.5j: a player choice pauses this pass outright; the next pass finds the next group.
    if let Some((player, candidates)) = find_legend_rule_group(state, script_runtime) {
        return SbaOutcome::NeedsLegendRuleChoice { player, candidates, resolved_so_far: events };
    }

    // CR 903.9a: pauses like the legend rule above.
    if let Some((player, commander)) = find_commander_return_candidate(state) {
        return SbaOutcome::NeedsCommanderReturnChoice { player, commander, resolved_so_far: events };
    }

    // CR 704.5d: scanned first, against zones as they stand at the top of the pass,
    // so a token that dies this pass ceases on the next one (CR 111.7).
    let mut departed_triggered = Vec::new();
    for id in collect_ceasing_tokens(state) {
        let (event, mut departed) = commit_token_cessation(state, id);
        events.push(event);
        departed_triggered.append(&mut departed);
    }

    // CR 704.5n: an Equipment unattaches and stays on the battlefield, so it cannot
    // join the dying list. Collected before this pass mutates state.
    for id in collect_illegally_attached_equipment(state, script_runtime) {
        events.push(commit_unattach(state, id));
    }
    // CR 303.4d: an Aura that is also a creature unattaches here and dies below, in the same pass.
    let creature_auras = collect_creature_auras(state, script_runtime);
    for &id in &creature_auras {
        events.push(commit_unattach(state, id));
    }

    // CR 704.5k: deterministic, so the losers join the dying list; `resolve_deaths` skips a duplicate id.
    let mut dying = collect_dying_creatures(state, script_runtime);
    dying.extend(collect_world_rule_losers(state, script_runtime));
    // CR 704.5m: "put into" a graveyard, not destroy, so a regeneration shield cannot save it.
    dying.extend(collect_illegally_attached_auras(state, script_runtime));
    // CR 303.4d: self-enchanting Auras, and the Aura-creatures unattached above.
    dying.extend(collect_self_enchanted_auras(state));
    dying.extend(creature_auras.into_iter().map(|id| DyingCandidate { id, is_destroy: false }));

    match resolve_deaths(state, script_runtime, dying.into()) {
        DeathResolution::Done(death_events, mut death_departed) => {
            events.extend(death_events);
            departed_triggered.append(&mut death_departed);
            SbaOutcome::Progressed(events, departed_triggered)
        }
        DeathResolution::NeedsOrderingChoice {
            affected_player,
            pending_event,
            candidates,
            excluded,
            is_destroy,
            pending_deaths,
        } => {
            // TODO: carry `departed_triggered` through this pause; a token cessation's departed abilities are lost when another death needs an ordering choice
            SbaOutcome::NeedsOrderingChoice {
                affected_player,
                pending_event,
                candidates,
                excluded,
                is_destroy,
                pending_deaths,
                resolved_so_far: events,
            }
        }
    }

    // TODO: planeswalker loyalty 0 (CR 704.5i)
    // TODO: the remaining CR 704.5 state-based actions
}

/// CR 704.5a, 704.5b, 704.5c, 903.10a: a player at 0 life, who drew from an empty
/// library, with 10 poison counters, or dealt 21 combat damage by one commander
/// loses. One `PlayerEliminated` per player whatever the cause; players already
/// eliminated are skipped so a rerun emits nothing.
///
/// CR 704.5j and 704.5k have their own collectors.
fn apply_player_loss(state: &mut GameState, events: &mut Vec<GameEvent>) {
    let losing: Vec<PlayerId> = state
        .players
        .iter()
        .filter(|p| {
            !p.eliminated
                && (p.life <= 0
                    || p.drew_from_empty_library
                    || p.counters.count(&CounterKind::Poison) >= 10
                    || p.commander_damage_taken.iter().any(|&(_, damage)| damage >= 21))
        })
        .map(|p| p.player_id)
        .collect();

    for player in losing {
        if let Some(p) = state.player_mut(player) {
            p.eliminated = true;
        }
        // CR 800.4a: needs an opponent to continue against, so a single-player fixture keeps its
        // objects, as `Engine::game_over_check` does.
        if state.turn_order.len() >= 2 {
            remove_players_objects(state, player);
        }
        let event_id = state.allocate_event_id();
        let sequence_no = state.allocate_sequence_no();
        events.push(GameEvent {
            event_id,
            sequence_no,
            kind: GameEventKind::PlayerEliminated { player },
            caused_by_command: None,
            parent_event: None,
        });
    }
}

/// CR 800.4a: every object owned by a player who left the game leaves with them.
/// Not a zone change, so nothing reaches `CollectTriggers`: the abilities
/// `retire_object` returns are discarded because their controller left (CR 800.4d).
///
/// Per-owner zones drain outright and shared zones keep other players' objects.
/// Stack entries go by owner for spells and by controller for abilities.
fn remove_players_objects(state: &mut GameState, player: PlayerId) {
    let owned_zone_ids: Vec<ObjectId> = state
        .zones
        .iter_mut()
        .filter(|z| z.owner == Some(player))
        .flat_map(|z| std::mem::take(&mut z.contents))
        .collect();

    let shared_zone_ids: Vec<ObjectId> = state
        .zones
        .iter()
        .filter(|z| z.owner.is_none())
        .flat_map(|z| z.contents.iter().copied())
        .filter(|id| state.objects.get(id).is_some_and(|o| o.owner == player))
        .collect();
    for &id in &shared_zone_ids {
        for zone in state.zones.iter_mut().filter(|z| z.owner.is_none()) {
            zone.remove(id);
        }
    }

    for id in owned_zone_ids.into_iter().chain(shared_zone_ids) {
        commands::retire_object(state, id);
    }

    // CR 800.4a: a spell goes with its owner; stack objects not represented by cards
    // (abilities) go with their controller.
    let stack_ids: Vec<ObjectId> = state
        .stack
        .iter()
        .filter(|so| so.owner == player || (so.controller == player && !matches!(so.kind, StackEntryKind::Spell)))
        .map(|so| so.stack_id)
        .collect();
    state.stack.retain(|so| !stack_ids.contains(&so.stack_id));
    for id in stack_ids {
        commands::retire_object(state, id);
    }
}

/// One `resolve_deaths` candidate. `is_destroy` marks genuine destruction
/// (CR 614.8), the only death a regeneration shield can intercept. CR 704.5f and
/// 704.5k move to the graveyard without destroying.
#[derive(Debug)]
pub struct DyingCandidate {
    pub id: ObjectId,
    pub is_destroy: bool,
}

/// CR 704.5f, 704.5g: battlefield creatures with non-positive toughness or lethal
/// marked damage. Characteristics come through `layers::effective_characteristics`.
/// `is_destroy` is false for 704.5f and true for 704.5g.
fn collect_dying_creatures(state: &GameState, script_runtime: &ScriptRuntime) -> Vec<DyingCandidate> {
    // `state.objects` is a `HashMap`, so the order is fixed by id. Creatures
    // that die together move to the graveyard in this order and take their new
    // ids in it, so an unordered walk would give a replay different ids.
    let mut candidates: Vec<ObjectId> =
        state.objects.values().filter(|o| o.zone == ZoneKind::Battlefield).map(|o| o.object_id).collect();
    candidates.sort();

    candidates
        .into_iter()
        .filter_map(|id| {
            let effective = crate::layers::effective_characteristics(state, script_runtime, id);
            // CR 613.1d: checked against the effective type, so an animated land counts.
            if !effective.card_types.contains(CardTypeSet::CREATURE) {
                return None;
            }
            // An unresolved toughness is skipped rather than guessed.
            let toughness = effective.toughness?;
            let object = state.object(id).expect("candidate id came from state.objects itself");
            // CR 704.5f: applies even to an indestructible creature (CR 702.12b) and is not destruction.
            if toughness <= 0 {
                return Some(DyingCandidate { id, is_destroy: false });
            }
            if effective.keywords.contains(KeywordSet::INDESTRUCTIBLE) {
                return None;
            }
            // CR 704.5g, 702.2b: lethal damage, or any damage from a deathtouch source; this is destruction.
            let lethal = object.damage_marked >= toughness as u32 || (object.deathtouched && object.damage_marked > 0);
            lethal.then_some(DyingCandidate { id, is_destroy: true })
        })
        .collect()
}

/// CR 704.5j: the first group of two or more legendary permanents sharing a
/// controller and effective name, for `Engine::submit_legend_rule_choice`. Applies
/// to any permanent type. Candidates and groups are sorted by id because
/// `state.objects` is a `HashMap`.
fn find_legend_rule_group(state: &GameState, script_runtime: &ScriptRuntime) -> Option<(PlayerId, Vec<ObjectId>)> {
    let mut groups: std::collections::BTreeMap<(PlayerId, String), Vec<ObjectId>> = std::collections::BTreeMap::new();
    for object in state.objects.values().filter(|o| o.zone == ZoneKind::Battlefield) {
        let effective = crate::layers::effective_characteristics(state, script_runtime, object.object_id);
        if !effective.supertypes.contains(SupertypeSet::LEGENDARY) {
            continue;
        }
        // CR 704.5j: groups by current controller.
        let controller = crate::layers::effective_controller(state, script_runtime, object.object_id);
        groups.entry((controller, effective.name)).or_default().push(object.object_id);
    }
    let mut violating: Vec<(PlayerId, Vec<ObjectId>)> = groups
        .into_iter()
        .filter(|(_, ids)| ids.len() >= 2)
        .map(|((player, _), mut ids)| {
            ids.sort();
            (player, ids)
        })
        .collect();
    violating.sort_by_key(|(_, ids)| ids[0]);
    violating.into_iter().next()
}

/// CR 903.9a: the first commander by `ObjectId` in a graveyard or exile that has
/// not been offered the command-zone choice this arrival. Uses the owner (CR 903.8).
fn find_commander_return_candidate(state: &GameState) -> Option<(PlayerId, ObjectId)> {
    let mut candidates: Vec<ObjectId> = state
        .objects
        .values()
        .filter(|o| {
            o.is_commander
                && matches!(o.zone, ZoneKind::Graveyard | ZoneKind::Exile)
                && !state.commander_return_offered.contains(&o.object_id)
        })
        .map(|o| o.object_id)
        .collect();
    candidates.sort();
    let commander = candidates.into_iter().next()?;
    Some((state.object(commander)?.owner, commander))
}

/// CR 704.5k: the World permanents that die: all but the one with the highest
/// timestamp, or all of them on a tie. Any permanent type. Deterministic, so the
/// losers join the dying list.
fn collect_world_rule_losers(state: &GameState, script_runtime: &ScriptRuntime) -> Vec<DyingCandidate> {
    let mut world_objects: Vec<(ObjectId, crate::types::ids::SequenceNo)> = state
        .objects
        .values()
        .filter(|o| o.zone == ZoneKind::Battlefield)
        .filter_map(|o| {
            let effective = crate::layers::effective_characteristics(state, script_runtime, o.object_id);
            effective.supertypes.contains(SupertypeSet::WORLD).then_some((o.object_id, o.timestamp))
        })
        .collect();
    if world_objects.len() < 2 {
        return Vec::new();
    }
    // `state.objects` is a `HashMap`, so sort for a replay-stable death order.
    world_objects.sort_by_key(|(id, _)| *id);
    let max_timestamp = world_objects.iter().map(|(_, ts)| *ts).max().expect("checked len >= 2 above");
    let survivor_count = world_objects.iter().filter(|(_, ts)| *ts == max_timestamp).count();
    // CR 704.5k: "put into" a graveyard, not destroy.
    if survivor_count == 1 {
        world_objects
            .into_iter()
            .filter(|(_, ts)| *ts != max_timestamp)
            .map(|(id, _)| DyingCandidate { id, is_destroy: false })
            .collect()
    } else {
        // CR 704.5k: a tie leaves no survivor.
        world_objects.into_iter().map(|(id, _)| DyingCandidate { id, is_destroy: false }).collect()
    }
}

/// CR 704.5m, 303.4c: battlefield Auras attached to nothing, or whose attachment
/// is no longer a legal target. The filter is rechecked through
/// `commands::attachment_target_is_legal`, which skips hexproof and shroud because
/// an Aura permanent does not target (CR 115.1b) but still applies protection
/// (CR 702.16). An Aura on a player is also illegal once that player is eliminated.
/// Without a `target_requirement`, only existence is checked.
///
/// Runs before `resolve_deaths`, so a target that dies this pass still counts as
/// legal and the Aura falls off one pass later (CR 304.3).
fn collect_illegally_attached_auras(state: &GameState, script_runtime: &ScriptRuntime) -> Vec<DyingCandidate> {
    let mut found: Vec<DyingCandidate> = state
        .objects
        .values()
        .filter(|o| o.zone == ZoneKind::Battlefield && commands::is_aura(&o.definition))
        .filter_map(|o| {
            let illegal = match (o.attached_to, o.attached_to_player) {
                (None, None) => true,
                (Some(target_id), None) => match &o.definition.target_requirement {
                    Some(crate::types::target::TargetSpec::Primitive(req)) => !commands::attachment_target_is_legal(
                        state,
                        script_runtime,
                        crate::types::ids::Target::Object(target_id),
                        &req.filter,
                        o.controller,
                        o.object_id,
                        req.exclude_source,
                        // No X context outside a cast or resolution.
                        None,
                    ),
                    _ => state.object(target_id).is_none(),
                },
                (None, Some(player)) => {
                    let eliminated = state.player(player).map(|p| p.eliminated).unwrap_or(true);
                    eliminated
                        || match &o.definition.target_requirement {
                            Some(crate::types::target::TargetSpec::Primitive(req)) => !commands::attachment_target_is_legal(
                                state,
                                script_runtime,
                                crate::types::ids::Target::Player(player),
                                &req.filter,
                                o.controller,
                                o.object_id,
                                req.exclude_source,
                                None,
                            ),
                            _ => false,
                        }
                }
                // Defensive: `attach_permanent` and its player variant each clear the other.
                (Some(_), Some(_)) => true,
            };
            illegal.then_some(DyingCandidate { id: o.object_id, is_destroy: false })
        })
        .collect();
    found.sort_by_key(|candidate| candidate.id);
    found
}

/// CR 303.4d, 704.5m: an Aura attached to itself is illegal even if it passes its
/// own enchant filter. Reachable today only through a directly constructed
/// fixture; kept as an always-on check.
fn collect_self_enchanted_auras(state: &GameState) -> Vec<DyingCandidate> {
    let mut found: Vec<DyingCandidate> = state
        .objects
        .values()
        .filter(|o| o.zone == ZoneKind::Battlefield && commands::is_aura(&o.definition) && o.attached_to == Some(o.object_id))
        .map(|o| DyingCandidate { id: o.object_id, is_destroy: false })
        .collect();
    found.sort_by_key(|candidate| candidate.id);
    found
}

/// CR 303.4d: battlefield Auras that are attached and whose effective types
/// include Creature. The caller unattaches each (`commit_unattach`) and also adds
/// it to the dying list in the same pass.
fn collect_creature_auras(state: &GameState, script_runtime: &ScriptRuntime) -> Vec<ObjectId> {
    let mut found: Vec<ObjectId> = state
        .objects
        .values()
        .filter(|o| {
            o.zone == ZoneKind::Battlefield
                && commands::is_aura(&o.definition)
                && o.attached_to.is_some()
                && crate::layers::effective_characteristics(state, script_runtime, o.object_id)
                    .card_types
                    .contains(CardTypeSet::CREATURE)
        })
        .map(|o| o.object_id)
        .collect();
    found.sort();
    found
}

/// CR 704.5n, 301.5: battlefield Equipment attached to something that is no longer
/// a legal creature to carry it: gone, no longer a creature, or protected from the
/// Equipment. The equip ability's own filter is not reused (CR 702.6c), so a control
/// change of the equipped creature does not unattach it. Unattached Equipment is
/// legal (CR 301.5b).
fn collect_illegally_attached_equipment(state: &GameState, script_runtime: &ScriptRuntime) -> Vec<ObjectId> {
    let creature_filter = crate::types::target::TargetFilter::Object {
        card_types: Some(CardTypeSet::CREATURE),
        excluded_card_types: None,
        subtypes: None,
        excluded_subtypes: None,
        supertypes: None,
        excluded_supertypes: None,
        controller: None,
        zone: None,
        mana_value: None,
    };
    let mut found: Vec<ObjectId> = state
        .objects
        .values()
        .filter(|o| o.zone == ZoneKind::Battlefield && commands::is_equipment(&o.definition))
        .filter_map(|o| {
            let target_id = o.attached_to?;
            let illegal = !commands::attachment_target_is_legal(
                state,
                script_runtime,
                crate::types::ids::Target::Object(target_id),
                &creature_filter,
                o.controller,
                o.object_id,
                // Synthetic filter, not a `TargetRequirement`, so "another" has no meaning.
                false,
                None,
            );
            illegal.then_some(o.object_id)
        })
        .collect();
    found.sort();
    found
}

/// CR 704.5n: clears `id`'s `attached_to` and its former target's `attachments`
/// entry. No zone change or new `ObjectId`, since `id` stays on the battlefield.
fn commit_unattach(state: &mut GameState, id: ObjectId) -> GameEvent {
    let was_attached_to = state.object_mut(id).and_then(|o| o.attached_to.take());
    if let Some(target_id) = was_attached_to {
        if let Some(target) = state.object_mut(target_id) {
            target.attachments.retain(|&a| a != id);
        }
    }
    let event_id = state.allocate_event_id();
    let sequence_no = state.allocate_sequence_no();
    GameEvent {
        event_id,
        sequence_no,
        kind: GameEventKind::Unattached {
            object: id,
            was_attached_to: was_attached_to
                .expect("caller only ever passes ids with a real attached_to"),
        },
        caused_by_command: None,
        parent_event: None,
    }
}

/// CR 704.5d: every token in a zone other than the battlefield. Scanned before
/// this pass's deaths so a token that just died ceases a pass later (CR 111.7).
/// The stack is not a `Zone`.
fn collect_ceasing_tokens(state: &GameState) -> Vec<ObjectId> {
    state
        .zones
        .iter()
        .filter(|z| z.kind != ZoneKind::Battlefield)
        .flat_map(|z| z.contents.iter().copied())
        .filter(|id| state.object(*id).is_some_and(|obj| obj.is_token))
        .collect()
}

/// Removes `id` from its zone and purges it. No new `ObjectId`: a ceasing token
/// has nowhere to move (CR 400.7). Returns its `Triggered` abilities as
/// `commit_death` does.
fn commit_token_cessation(state: &mut GameState, id: ObjectId) -> (GameEvent, Vec<RegisteredAbility>) {
    for zone in &mut state.zones {
        zone.remove(id);
    }
    let departed = commands::retire_object(state, id);
    let event_id = state.allocate_event_id();
    let sequence_no = state.allocate_sequence_no();
    let event = GameEvent {
        event_id,
        sequence_no,
        kind: GameEventKind::TokenCeased { object: id },
        caused_by_command: None,
        parent_event: None,
    };
    (event, departed)
}

/// What one `resolve_deaths` call produced.
pub(crate) enum DeathResolution {
    /// Every pending death is moved or gone. Holds the committed events and the dead
    /// objects' `Triggered` abilities (see `SbaOutcome::Progressed`).
    Done(Vec<GameEvent>, Vec<RegisteredAbility>),
    /// The next death's replacement matched several effects (CR 616.1).
    /// `pending_deaths` is what remains, excluding the one that paused.
    NeedsOrderingChoice {
        affected_player: PlayerId,
        pending_event: GameEventKind,
        candidates: Vec<crate::replacement::ReplacementCandidate>,
        excluded: Vec<crate::replacement::ReplacementCandidate>,
        /// Whether `pending_event` is genuine destruction; the remaining entries carry their own flag.
        is_destroy: bool,
        pending_deaths: VecDeque<DyingCandidate>,
    },
}

/// Resolves the dying list one object at a time (CR 704.5f, 704.5g). Each
/// Battlefield to Graveyard `ZoneChange` is proposed under its live id and run
/// through `replacement::apply_replacements` before moving, so an effect can
/// substitute the destination (CR 614.13).
///
/// Pauses on `DeathResolution::NeedsOrderingChoice` (CR 616.1);
/// `Engine::submit_replacement_choice` resumes by calling this again with the
/// same `pending_deaths`.
pub(crate) fn resolve_deaths(
    state: &mut GameState,
    script_runtime: &ScriptRuntime,
    mut pending_deaths: VecDeque<DyingCandidate>,
) -> DeathResolution {
    let mut resolved = Vec::new();
    let mut departed_triggered = Vec::new();
    while let Some(DyingCandidate { id: object_id, is_destroy }) = pending_deaths.pop_front() {
        let Some(object) = state.object(object_id) else {
            // Already moved earlier in the pass; nothing to do.
            continue;
        };
        let last_known_info = object.clone();
        let proposed = GameEventKind::ZoneChange {
            object: object_id,
            from: ZoneKind::Battlefield,
            to: ZoneKind::Graveyard,
            last_known_info: Some(last_known_info),
        };
        match replacement::apply_replacements(state, script_runtime, proposed, is_destroy) {
            ReplacementOutcome::Resolved(final_event) => {
                for (event, mut abilities) in commit_death_or_regeneration(state, final_event) {
                    resolved.push(event);
                    departed_triggered.append(&mut abilities);
                }
            }
            ReplacementOutcome::NeedsOrderingChoice { affected_player, event, candidates, excluded, .. } => {
                // TODO: thread earlier iterations' events and departed abilities into this pause; they are dropped when a later death needs an ordering choice
                return DeathResolution::NeedsOrderingChoice {
                    affected_player,
                    pending_event: event,
                    candidates,
                    excluded,
                    is_destroy,
                    pending_deaths,
                };
            }
        }
    }
    DeathResolution::Done(resolved, departed_triggered)
}

/// Performs the zone move for one replacement-checked death: mints a new
/// `ObjectId` (CR 400.7), purges dangling abilities and effects with
/// `retire_object`, and builds the ledger event. `to` is `Graveyard` unless a
/// replacement substituted it.
///
/// Returns the object's `Triggered` abilities (CR 603.10), because `retire_object`
/// purges them before `collect_triggers` runs.
pub(crate) fn commit_death(state: &mut GameState, event: GameEventKind) -> (GameEvent, Vec<RegisteredAbility>) {
    let GameEventKind::ZoneChange { object: old_id, to, last_known_info, .. } = event else {
        // Every caller goes through `commit_death_or_regeneration`, which forwards only a `ZoneChange` here.
        unreachable!("commit_death is only ever called with a ZoneChange event");
    };
    let last_known_info = last_known_info.expect("resolve_deaths always sets last_known_info");
    let owner = last_known_info.owner;

    let new_id = state.allocate_object_id();
    let mut new_object = last_known_info.clone();
    new_object.object_id = new_id;
    new_object.lineage = ObjectLineage { previous: Some(old_id) };
    new_object.zone = to;
    new_object.timestamp = state.allocate_sequence_no();
    new_object.reset_for_new_object();

    if let Some(zone) = state.zone_mut(ZoneKind::Battlefield, None) {
        zone.remove(old_id);
    }
    // CR 400.7: the object is gone for good, so purge abilities and effects pointing at it.
    let departed_triggered = commands::retire_object(state, old_id);
    // Exile is a shared zone; a wrong-owner lookup would find no zone and drop the object.
    let destination_owner = match to {
        ZoneKind::Battlefield | ZoneKind::Exile => None,
        _ => Some(owner),
    };
    if let Some(zone) = state.zone_mut(to, destination_owner) {
        zone.push_bottom(new_id);
        state.objects.insert(new_id, new_object);
    }
    // No such zone registered: defensive no-op.

    let event_id = state.allocate_event_id();
    let sequence_no = state.allocate_sequence_no();
    let event = GameEvent {
        event_id,
        sequence_no,
        kind: GameEventKind::ZoneChange {
            object: new_id,
            from: ZoneKind::Battlefield,
            to,
            last_known_info: Some(last_known_info),
        },
        caused_by_command: None,
        parent_event: None,
    };
    (event, departed_triggered)
}

/// CR 614.8: commits a `Regenerated` event: removes marked damage and the
/// deathtouch taint, taps the permanent and removes it from combat (CR 506.4).
/// The `ObjectId` is unchanged and nothing is retired, so the second element is empty.
pub(crate) fn commit_regeneration(state: &mut GameState, object: ObjectId) -> (GameEvent, Vec<RegisteredAbility>) {
    if let Some(obj) = state.object_mut(object) {
        obj.damage_marked = 0;
        obj.deathtouched = false;
        obj.tapped = true;
    }
    if let Some(combat) = state.combat.as_mut() {
        combat.attackers.retain(|(id, _)| *id != object);
        combat.blocks.retain(|(blocker, attacker)| *blocker != object && *attacker != object);
        combat.effect_blocked.retain(|id| *id != object);
    }
    let event_id = state.allocate_event_id();
    let sequence_no = state.allocate_sequence_no();
    let event = GameEvent {
        event_id,
        sequence_no,
        kind: GameEventKind::Regenerated { object },
        caused_by_command: None,
        parent_event: None,
    };
    (event, Vec::new())
}

/// CR 702.89a (Umbra armor): clears damage on `enchanted` and destroys `aura`
/// through `commit_death`, a real zone move. Returns two committed events, which
/// is why `commit_death_or_regeneration` returns a `Vec`.
// TODO: run the Aura's departure through replacement checking (CR 616)
fn commit_umbra_armor(state: &mut GameState, enchanted: ObjectId, aura: ObjectId) -> Vec<(GameEvent, Vec<RegisteredAbility>)> {
    if let Some(obj) = state.object_mut(enchanted) {
        obj.damage_marked = 0;
    }
    let event_id = state.allocate_event_id();
    let sequence_no = state.allocate_sequence_no();
    let consumed_event = GameEvent {
        event_id,
        sequence_no,
        kind: GameEventKind::UmbraArmorConsumed { enchanted, aura },
        caused_by_command: None,
        parent_event: None,
    };
    let mut committed = vec![(consumed_event, Vec::new())];
    // `aura` may already be gone (two Umbra armors matching one destroy, or it left earlier this pass); skip.
    if let Some(aura_obj) = state.object(aura).cloned() {
        let proposed = GameEventKind::ZoneChange {
            object: aura,
            from: ZoneKind::Battlefield,
            to: ZoneKind::Graveyard,
            last_known_info: Some(aura_obj),
        };
        committed.push(commit_death(state, proposed));
    }
    committed
}

/// Dispatches a replacement-checked destroy's final event to `commit_death`,
/// `commit_regeneration` or `commit_umbra_armor`. Returns a `Vec` because an
/// `UmbraArmorConsumed` commits two events.
pub(crate) fn commit_death_or_regeneration(
    state: &mut GameState,
    event: GameEventKind,
) -> Vec<(GameEvent, Vec<RegisteredAbility>)> {
    match event {
        GameEventKind::Regenerated { object } => vec![commit_regeneration(state, object)],
        GameEventKind::UmbraArmorConsumed { enchanted, aura } => commit_umbra_armor(state, enchanted, aura),
        other => vec![commit_death(state, other)],
    }
}

/// CR 903.9a: performs the accepted return of a commander from a graveyard or
/// exile to the command zone, with a new `ObjectId` (CR 400.7) and `is_commander`
/// and `owner` preserved. `None` if `old_id` no longer resolves.
// TODO: run this move through replacement checking, for CR 903.9b's redirect to the command zone
pub(crate) fn move_commander_to_command_zone(state: &mut GameState, old_id: ObjectId) -> Option<GameEvent> {
    let object = state.object(old_id)?;
    let from = object.zone;
    let owner = object.owner;
    let last_known_info = object.clone();

    let new_id = state.allocate_object_id();
    let mut new_object = last_known_info.clone();
    new_object.object_id = new_id;
    new_object.lineage = ObjectLineage { previous: Some(old_id) };
    new_object.zone = ZoneKind::Command;
    new_object.timestamp = state.allocate_sequence_no();

    // Graveyard is per-owner and exile is shared.
    let from_zone_owner = if from == ZoneKind::Exile { None } else { Some(owner) };
    if let Some(zone) = state.zone_mut(from, from_zone_owner) {
        zone.remove(old_id);
    }
    commands::retire_object(state, old_id);
    if let Some(zone) = state.zone_mut(ZoneKind::Command, Some(owner)) {
        zone.push_bottom(new_id);
        state.objects.insert(new_id, new_object);
    }

    let event_id = state.allocate_event_id();
    let sequence_no = state.allocate_sequence_no();
    Some(GameEvent {
        event_id,
        sequence_no,
        kind: GameEventKind::ZoneChange {
            object: new_id,
            from,
            to: ZoneKind::Command,
            last_known_info: Some(last_known_info),
        },
        caused_by_command: None,
        parent_event: None,
    })
}
