//! The rules about a whole combat declaration: "attacks each combat if able"
//! (CR 508.1d) and menace (CR 509.1b, 702.111b). `validate_declare_attackers`,
//! `validate_declare_blockers`, the pass-priority check and the entry builders
//! all read them from here, so the entries and the commands cannot drift apart
//! (024).
//!
//! An entry is a per-creature list, so it can only carry the part of a
//! whole-declaration rule that one creature decides alone. Menace shows up in
//! `legal_blockers` as "an attacker is offered only if two creatures could block
//! it". Whether the blockers a player picked add up to two, and whether two
//! menace attackers fight over the same two blockers, is left to
//! `block_declaration_is_legal` when the Answer arrives.
//!
//! TODO(026-followup): "blocks if able" (CR 509.1c) and the maximum-requirements
//! search that comes with it. A menace attacker plus a "blocks if able" creature
//! forces every legal blocker to block, which needs the search.
//! TODO(026-followup): "attacks a specific player if able".
//! TODO(026-followup): costs to attack (CR 508.1g to 508.1j). A creature that
//! can't attack unless its controller pays is treated as able, and nobody is
//! forced to pay.
//! TODO(026-followup): restrictions on the whole declaration ("can't attack
//! alone", "no more than one creature can attack"). Nothing in the engine
//! produces one yet, so the maximum-requirements count is just "every able
//! must-attack creature attacks".

use std::collections::BTreeMap;

use crate::script::ScriptRuntime;
use crate::types::card::KeywordSet;
use crate::types::combat::CombatState;
use crate::types::game::GameState;
use crate::types::ids::{ObjectId, PlayerId, Target};
use crate::types::turn::{Phase, Step};

use super::super::attach::non_eliminated_players;
use super::super::entries::target_order;
use super::super::legality::effective_keywords;
use super::super::Rejected;
use super::declare::{attack_target_is_legal, attacker_can_attack, block_is_legal};

/// Every object id in the game, ascending. The entries list in this order.
fn sorted_ids(state: &GameState) -> Vec<ObjectId> {
    let mut ids: Vec<ObjectId> = state.objects.keys().copied().collect();
    ids.sort();
    ids
}

/// What `actor` may attack: opponents first, then the planeswalkers and
/// battles they control (CR 506.4, 508.1b).
pub(crate) fn legal_defenders(state: &GameState, script_runtime: &ScriptRuntime, actor: PlayerId) -> Vec<Target> {
    let mut possible: Vec<Target> =
        non_eliminated_players(state).into_iter().filter(|&p| p != actor).map(Target::Player).collect();
    possible.extend(sorted_ids(state).into_iter().map(Target::Object));
    possible.sort_by_key(target_order);
    possible.into_iter().filter(|target| attack_target_is_legal(state, script_runtime, actor, target).is_ok()).collect()
}

/// CR 508.1d: the creatures `actor` must declare as attackers. A creature that
/// "attacks each combat if able" is required only when it is able: it passes
/// `attacker_can_attack` and there is something to attack.
pub(crate) fn required_attackers(state: &GameState, script_runtime: &ScriptRuntime, actor: PlayerId) -> Vec<ObjectId> {
    if legal_defenders(state, script_runtime, actor).is_empty() {
        return Vec::new();
    }
    sorted_ids(state)
        .into_iter()
        .filter(|&id| attacker_can_attack(state, script_runtime, actor, id).is_ok())
        .filter(|&id| effective_keywords(state, script_runtime, id).contains(KeywordSet::ATTACKS_EACH_COMBAT))
        .collect()
}

/// CR 508.1d: the declaration is illegal if it leaves a required attacker
/// home. Each requirement is independent today, so the maximum number of
/// requirements that can be obeyed is all of them.
pub(crate) fn attack_declaration_is_legal(
    state: &GameState,
    script_runtime: &ScriptRuntime,
    actor: PlayerId,
    attackers: &[(ObjectId, Target)],
) -> Result<(), Rejected> {
    let left_home = required_attackers(state, script_runtime, actor)
        .into_iter()
        .any(|required| !attackers.iter().any(|(id, _)| *id == required));
    if left_home {
        return Err(Rejected::IllegalAction(
            "a creature that attacks each combat if able must be declared as an attacker".to_string(),
        ));
    }
    Ok(())
}

/// CR 508.1d: the active player can't pass priority in the declare attackers
/// step while a required attacker is still undeclared. Passing would skip the
/// declaration, and an empty declaration is illegal for the same reason.
pub(crate) fn pass_would_skip_a_required_attack(
    state: &GameState,
    script_runtime: &ScriptRuntime,
    actor: PlayerId,
) -> bool {
    actor == state.active_player
        && state.phase == Phase::Combat
        && state.step == Step::DeclareAttackers
        && state.combat.is_none()
        && !required_attackers(state, script_runtime, actor).is_empty()
}

/// CR 702.111b: how many creatures must block `attacker` once any block it.
/// Two for menace, one otherwise (CR 702.111c: repeated menace changes nothing).
pub(crate) fn blockers_needed(state: &GameState, script_runtime: &ScriptRuntime, attacker: ObjectId) -> usize {
    if effective_keywords(state, script_runtime, attacker).contains(KeywordSet::MENACE) {
        2
    } else {
        1
    }
}

/// CR 509.1b: a blocked attacker needs `blockers_needed` of the declared
/// blockers. This counts the direct blocks of one submission and runs before
/// banding adds its propagated blocks (CR 702.22h bypasses it).
pub(crate) fn block_declaration_is_legal(
    state: &GameState,
    script_runtime: &ScriptRuntime,
    blocks: &[(ObjectId, ObjectId)],
) -> Result<(), Rejected> {
    let mut blocker_counts: BTreeMap<ObjectId, usize> = BTreeMap::new();
    for (_, attacker) in blocks {
        *blocker_counts.entry(*attacker).or_insert(0) += 1;
    }
    for (attacker, count) in blocker_counts {
        if count < blockers_needed(state, script_runtime, attacker) {
            return Err(Rejected::IllegalAction(
                "an attacker with menace can't be blocked except by two or more creatures".to_string(),
            ));
        }
    }
    Ok(())
}

/// The attackers `player` could block, each with the creatures that could
/// legally block it. An attacker with fewer legal blockers than
/// `blockers_needed` is left out, since no declaration can block it.
pub(crate) fn blockable_attackers(
    state: &GameState,
    script_runtime: &ScriptRuntime,
    player: PlayerId,
    combat: &CombatState,
) -> Vec<(ObjectId, Vec<ObjectId>)> {
    let ids = sorted_ids(state);
    combat
        .attackers
        .iter()
        .map(|(attacker, _)| {
            let blockers: Vec<ObjectId> = ids
                .iter()
                .copied()
                .filter(|&blocker| block_is_legal(state, script_runtime, player, combat, blocker, *attacker).is_ok())
                .collect();
            (*attacker, blockers)
        })
        .filter(|(attacker, blockers)| {
            !blockers.is_empty() && blockers.len() >= blockers_needed(state, script_runtime, *attacker)
        })
        .collect()
}
