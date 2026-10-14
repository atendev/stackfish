//! Builds the entries a Decision lists for the player who holds priority. Each
//! check mirrors the one `execute_cast_spell`, `execute_activate_ability` and
//! `execute_play_land` run before they change anything, so an entry is listed
//! exactly when the command would get past its timing, zone and target checks.
//! The engine does not check affordability here (ticket 003).
//!
//! TODO: a rule that changes timing or zone permission through a continuous
//! effect ("as though it had flash", "play lands from your graveyard") is not
//! represented in the commands either, so entries inherit that gap.

use std::collections::HashMap;

use crate::decision::{
    ActivateEntry, CastEntry, ColorChoice, Entry, EntryTargets, ModeRange, TargetSlot,
};
use crate::script::ScriptRuntime;
use crate::types::ability::{AbilityEffect, AbilityKind};
use crate::types::card::{CardDefinition, CardTypeSet, KeywordSet, ModeChoice};
use crate::types::cost::{AdditionalCost, AlternativeCost};
use crate::types::game::GameState;
use crate::types::ids::{ObjectId, PlayerId, Target};
use crate::types::mana::Color;
use crate::types::target::{TargetRequirement, TargetSpec};
use crate::types::zone::ZoneKind;

use super::legality::{has_summoning_sickness, is_sorcery_speed, legal_target_candidates};

/// Every entry the priority player may take, with `Pass` last.
pub(crate) fn priority_entries(state: &GameState, script_runtime: &ScriptRuntime) -> Vec<Entry> {
    let player = state.priority_player;
    let mut entries = Vec::new();
    play_land_entries(state, player, &mut entries);
    cast_entries(state, script_runtime, player, &mut entries);
    activate_entries(state, script_runtime, player, &mut entries);
    entries.push(Entry::Pass);
    entries
}

fn play_land_entries(state: &GameState, player: PlayerId, entries: &mut Vec<Entry>) {
    if !is_sorcery_speed(state, player) {
        return;
    }
    let played = *state.lands_played_this_turn.get(&player).unwrap_or(&0);
    if played >= crate::types::player::effective_land_play_limit(&state.land_play_modifiers, player) {
        return;
    }
    let Some(hand) = state.zone(ZoneKind::Hand, Some(player)) else { return };
    for &id in hand.contents.iter() {
        let Some(object) = state.object(id) else { continue };
        if object.controller == player && object.definition.card_types.contains(CardTypeSet::LAND) {
            entries.push(Entry::PlayLand { card: id });
        }
    }
}

fn cast_entries(state: &GameState, script_runtime: &ScriptRuntime, player: PlayerId, entries: &mut Vec<Entry>) {
    let mut ids: Vec<ObjectId> = state.objects.keys().copied().collect();
    ids.sort();
    for id in ids {
        let object = &state.objects[&id];
        let definition = &object.definition;
        if definition.card_types.contains(CardTypeSet::LAND) {
            continue;
        }
        // CR 903.8: a commander may be cast from the command zone by its
        // owner. Every other normal cast comes from the hand.
        let normal = (object.zone == ZoneKind::Hand && object.controller == player)
            || (object.is_commander && object.zone == ZoneKind::Command && object.owner == player);
        // CR 601.2b: an alternative cost also grants the zone it casts from.
        let alternative_costs: Vec<usize> = if object.controller == player {
            definition
                .alternative_costs
                .iter()
                .enumerate()
                .filter(|(_, alternative)| match alternative {
                    AlternativeCost::Replacement { from_zone, .. } => *from_zone == object.zone,
                })
                .map(|(index, _)| index)
                .collect()
        } else {
            Vec::new()
        };
        if !normal && alternative_costs.is_empty() {
            continue;
        }
        // CR 117.1a/702.8b: instants and flash are castable whenever the
        // player holds priority. Everything else needs the sorcery window.
        let instant_speed = definition.card_types.contains(CardTypeSet::INSTANT)
            || definition.keywords.contains(KeywordSet::FLASH);
        let timing_ok = if instant_speed { state.priority_player == player } else { is_sorcery_speed(state, player) };
        if !timing_ok {
            continue;
        }
        let Some((modes, targets)) = cast_modes_and_targets(state, script_runtime, player, id, definition) else {
            continue;
        };
        entries.push(Entry::Cast(CastEntry {
            card: id,
            modes,
            targets,
            x: definition.mana_cost.has_x(),
            normal,
            alternative_costs,
            buyback: definition.buyback_cost.is_some(),
        }));
    }
}

/// The mode range and target slots a cast announces, or `None` when no
/// legal announcement exists (a required target has no candidate).
fn cast_modes_and_targets(
    state: &GameState,
    script_runtime: &ScriptRuntime,
    player: PlayerId,
    card: ObjectId,
    definition: &CardDefinition,
) -> Option<(Option<ModeRange>, EntryTargets)> {
    if definition.modes.is_empty() {
        let slots = target_slots(state, script_runtime, definition.target_requirement.as_ref(), player, card);
        return slots_are_satisfiable(&slots).then_some((None, EntryTargets::Fixed(slots)));
    }
    let per_mode: Vec<Vec<TargetSlot>> = definition
        .modes
        .iter()
        .map(|mode| target_slots(state, script_runtime, mode.target_requirement.as_ref(), player, card))
        .collect();
    let range = mode_range(definition);
    // TODO: this counts modes that can be chosen on their own. Two modes
    // that each have a target can still fail together if
    // `combine_chosen_modes` cannot combine them, and the engine rejects
    // that pair when the Answer arrives.
    let usable = per_mode.iter().filter(|slots| slots_are_satisfiable(slots)).count() as u32;
    let needed = if range.repeatable { range.min.min(1) } else { range.min };
    (usable >= needed).then_some((Some(range), EntryTargets::PerMode(per_mode)))
}

fn mode_range(definition: &CardDefinition) -> ModeRange {
    let mode_count = definition.modes.len();
    let (min, max, repeatable) = definition.mode_choice.bounds();
    match definition.mode_choice {
        ModeChoice::ExactlyChosenByOpponent(_) | ModeChoice::UpToChosenByOpponent(_) => ModeRange {
            mode_count,
            min: 0,
            max: 0,
            repeatable: false,
            chosen_by_opponent: true,
            pawprint_weights: None,
        },
        ModeChoice::PawprintBudget(_) => ModeRange {
            mode_count,
            min,
            max,
            repeatable: false,
            chosen_by_opponent: false,
            pawprint_weights: Some(definition.modes.iter().map(|mode| mode.pawprint_weight).collect()),
        },
        _ => ModeRange { mode_count, min, max, repeatable, chosen_by_opponent: false, pawprint_weights: None },
    }
}

fn activate_entries(state: &GameState, script_runtime: &ScriptRuntime, player: PlayerId, entries: &mut Vec<Entry>) {
    // `ability_index` counts every ability registered to a source, not only
    // the activated ones (`find_ability`).
    let mut seen: HashMap<ObjectId, u32> = HashMap::new();
    for ability in &state.abilities {
        let counter = seen.entry(ability.source).or_insert(0);
        let ability_index = *counter;
        *counter += 1;
        if ability.kind != AbilityKind::Activated {
            continue;
        }
        let Some(source) = state.object(ability.source) else { continue };
        if crate::layers::effective_controller(state, script_runtime, ability.source) != player {
            continue;
        }

        // CR 602.1/112.6/113.6: Battlefield unless Cycling's discard cost or a
        // script's `SetFunctionsFrom` says otherwise.
        let cost = ability.cost.as_ref();
        let requires_hand = cost.is_some_and(|c| c.additional.iter().any(|a| matches!(a, AdditionalCost::DiscardSelf)));
        let required_zone = state
            .ability_function_zones
            .get(&ability.id)
            .copied()
            .unwrap_or(if requires_hand { ZoneKind::Hand } else { ZoneKind::Battlefield });
        if source.zone != required_zone {
            continue;
        }
        // CR 702.6a: Equip is sorcery-speed.
        if matches!(ability.effect, Some(AbilityEffect::Attach)) && !is_sorcery_speed(state, player) {
            continue;
        }

        let mana_ability = matches!(ability.effect, Some(AbilityEffect::AddMana(_) | AbilityEffect::AddManaChoice { .. }));
        let has_real_cost = cost.is_some_and(|c| c.mana.is_some() || !c.additional.is_empty());
        let taps_source = if mana_ability && !has_real_cost {
            true
        } else {
            cost.is_some_and(|c| c.additional.iter().any(|a| matches!(a, AdditionalCost::TapSelf)))
        };
        // CR 602.5a/302.6: a {T} cost needs an untapped source, and a creature
        // also needs to be free of summoning sickness.
        if taps_source
            && (source.tapped
                || (source.definition.card_types.contains(CardTypeSet::CREATURE)
                    && has_summoning_sickness(state, script_runtime, source)))
        {
            continue;
        }

        let targets = if mana_ability {
            Vec::new()
        } else {
            target_slots(state, script_runtime, ability.target.as_ref(), player, ability.source)
        };
        if !slots_are_satisfiable(&targets) {
            continue;
        }
        let color_choice = match &ability.effect {
            Some(AbilityEffect::AddManaChoice { count, choices }) => Some(ColorChoice {
                count: *count,
                colors: [Color::White, Color::Blue, Color::Black, Color::Red, Color::Green]
                    .into_iter()
                    .filter(|&color| choices.contains_color(color))
                    .collect(),
            }),
            _ => None,
        };
        entries.push(Entry::Activate(ActivateEntry {
            source: ability.source,
            ability_index,
            targets,
            x: cost.and_then(|c| c.mana.as_ref()).is_some_and(|m| m.has_x()),
            mana_ability,
            color_choice,
        }));
    }
}

/// One slot per target word of `spec`, with each word's legal candidates in a
/// stable order.
fn target_slots(
    state: &GameState,
    script_runtime: &ScriptRuntime,
    spec: Option<&TargetSpec>,
    player: PlayerId,
    source: ObjectId,
) -> Vec<TargetSlot> {
    let requirements: Vec<&TargetRequirement> = match spec {
        None => Vec::new(),
        Some(TargetSpec::Primitive(requirement)) => vec![requirement],
        Some(TargetSpec::Multiple(requirements)) => requirements.iter().collect(),
    };
    requirements
        .into_iter()
        .map(|requirement| {
            let mut candidates = legal_target_candidates(state, script_runtime, requirement, player, source);
            candidates.sort_by_key(target_order);
            TargetSlot {
                candidates,
                min: if requirement.up_to { 0 } else { requirement.count },
                max: if requirement.at_least { None } else { Some(requirement.count) },
            }
        })
        .collect()
}

/// Objects by id, then players, then abilities on the stack.
pub(crate) fn target_order(target: &Target) -> (u8, u64) {
    match target {
        Target::Object(id) => (0, id.0),
        Target::Player(player) => (1, u64::from(player.0)),
        Target::Ability(id) => (2, id.0),
    }
}

/// Every word has at least as many candidates as it must name. A word cannot
/// name the same target twice (CR 601.2c).
fn slots_are_satisfiable(slots: &[TargetSlot]) -> bool {
    slots.iter().all(|slot| slot.candidates.len() as u32 >= slot.min)
}
