//! The options a combat Decision lists. Each builder asks the same checks that
//! `DeclareAttackers`, `DeclareBlockers` and `AssignCombatDamage` run, so a
//! listed attack, block or split is one the command accepts on its own. The
//! rules about the whole declaration (menace, "must attack", banding) stay
//! with the command and are checked when the Answer arrives.
//!
//! TODO: `DeclareBands` (CR 702.22) is not offered. It stays a direct
//! `submit_action`.

use std::collections::HashSet;

use crate::decision::{AttackerEntry, BlockerEntry, DamageSplitEntry};
use crate::script::ScriptRuntime;
use crate::types::card::KeywordSet;
use crate::types::game::GameState;
use crate::types::ids::{ObjectId, PlayerId, Target};
use crate::types::turn::{Phase, Step};

use super::super::attach::non_eliminated_players;
use super::super::entries::target_order;
use super::super::legality::{effective_keywords, effective_power};
use super::damage::plan_damage_splits;
use super::declare::{attack_target_is_legal, attacker_can_attack, block_is_legal, is_defending_player};

/// The active player's attackers while attackers can still be declared, or
/// an empty list. An empty list means no Decision is needed.
pub(crate) fn attacker_entries(state: &GameState, script_runtime: &ScriptRuntime) -> Vec<AttackerEntry> {
    if state.phase != Phase::Combat || state.step != Step::DeclareAttackers || state.combat.is_some() {
        return Vec::new();
    }
    let actor = state.active_player;
    let mut ids: Vec<ObjectId> = state.objects.keys().copied().collect();
    ids.sort();
    // Opponents first, then the planeswalkers and battles they control.
    let mut possible_defenders: Vec<Target> =
        non_eliminated_players(state).into_iter().filter(|&p| p != actor).map(Target::Player).collect();
    possible_defenders.extend(ids.iter().map(|&id| Target::Object(id)));
    possible_defenders.sort_by_key(target_order);
    let defenders: Vec<Target> = possible_defenders
        .into_iter()
        .filter(|target| attack_target_is_legal(state, script_runtime, actor, target).is_ok())
        .collect();
    if defenders.is_empty() {
        return Vec::new();
    }
    ids.into_iter()
        .filter(|&id| attacker_can_attack(state, script_runtime, actor, id).is_ok())
        .map(|attacker| AttackerEntry { attacker, defenders: defenders.clone() })
        .collect()
}

/// `player`'s blockers while they still owe a declaration, or an empty list.
pub(crate) fn blocker_entries(state: &GameState, script_runtime: &ScriptRuntime, player: PlayerId) -> Vec<BlockerEntry> {
    if state.phase != Phase::Combat || state.step != Step::DeclareBlockers {
        return Vec::new();
    }
    let Some(combat) = state.combat.as_ref() else { return Vec::new() };
    if combat.declared_defenders.contains(&player) || !is_defending_player(state, script_runtime, combat, player) {
        return Vec::new();
    }
    let mut ids: Vec<ObjectId> = state.objects.keys().copied().collect();
    ids.sort();
    ids.into_iter()
        .filter_map(|blocker| {
            let attackers: Vec<ObjectId> = combat
                .attackers
                .iter()
                .map(|(attacker, _)| *attacker)
                .filter(|&attacker| block_is_legal(state, script_runtime, player, combat, blocker, attacker).is_ok())
                .collect();
            (!attackers.is_empty()).then_some(BlockerEntry { blocker, attackers })
        })
        .collect()
}

/// The first defending player, in turn order from the active player, who still
/// owes a declaration and has a creature that may block.
pub(crate) fn next_blocking_player(state: &GameState, script_runtime: &ScriptRuntime) -> Option<PlayerId> {
    let start = state.turn_order.iter().position(|&p| p == state.active_player)?;
    let (after, before) = state.turn_order.split_at(start);
    before
        .iter()
        .chain(after.iter())
        .copied()
        .find(|&player| !blocker_entries(state, script_runtime, player).is_empty())
}

/// The splits still missing in this combat damage step for the player who
/// chooses first, or `None` when nothing is waiting on a player. Only the
/// splits that one player chooses are listed. Another player's share is
/// listed once they hold the Decision.
pub(crate) fn damage_split_entries(
    state: &GameState,
    script_runtime: &ScriptRuntime,
) -> Option<(PlayerId, Vec<DamageSplitEntry>)> {
    let plan = plan_damage_splits(state, script_runtime).ok().flatten()?;
    let submitted: HashSet<ObjectId> = plan.combat.pending_damage_splits.keys().copied().collect();
    let mut missing: Vec<ObjectId> =
        plan.requires_assignment.iter().copied().filter(|id| !submitted.contains(id)).collect();
    missing.sort();
    let chooser = *plan.required_actor_by_id.get(missing.first()?)?;
    let entries = missing
        .into_iter()
        .filter(|id| plan.required_actor_by_id.get(id) == Some(&chooser))
        .map(|source| {
            let power = effective_power(state, script_runtime, source).map_or(0, |p| p.max(0) as u32);
            match plan.blockers_by_attacker.get(&source) {
                Some(blockers) => DamageSplitEntry {
                    source,
                    power,
                    recipients: blockers.clone(),
                    spill: effective_keywords(state, script_runtime, source)
                        .contains(KeywordSet::TRAMPLE)
                        .then(|| plan.combat.attackers.iter().find(|(id, _)| *id == source).map(|(_, target)| *target))
                        .flatten(),
                },
                None => DamageSplitEntry {
                    source,
                    power,
                    recipients: plan.attackers_by_blocker.get(&source).cloned().unwrap_or_default(),
                    spill: None,
                },
            }
        })
        .collect();
    Some((chooser, entries))
}
