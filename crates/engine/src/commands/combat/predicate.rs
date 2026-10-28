//! The rules about a whole combat declaration: must-attack (CR 508.1d) and
//! menace (CR 509.1b, 702.111b). The commands and the entry builders both read
//! them from here, so the two cannot drift apart.

// TODO: "blocks if able" and the maximum-requirements search that comes with it, since a menace attacker plus a "blocks if able" creature forces every legal blocker to block (CR 509.1c)
// TODO: "attacks a specific player if able"
// TODO: costs to attack are not charged, so a creature that can't attack unless its controller pays is treated as able and nobody is forced to pay (CR 508.1g to 508.1j)
// TODO: restrictions on the whole declaration ("can't attack alone"); nothing produces one yet, so the maximum-requirements count is every able must-attack creature

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
/// home. Each requirement is independent, so the maximum number of requirements
/// that can be obeyed is all of them.
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
/// banding adds its propagated blocks (CR 702.22h bypasses it). The entries
/// cannot check it because they list one creature at a time.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::script::ScriptRuntime;
    use crate::types::card::{CardDefinition, CardTypeSet, KeywordSet, ModeChoice, PTValue, ScriptReference, SupertypeSet as SupertypeSetT};
    use crate::types::combat::CombatState;
    use crate::types::counter::CounterSet;
    use crate::types::game::{GameState, RngState};
    use crate::types::ids::{ObjectId, PlayerId, SequenceNo, Target};
    use crate::types::mana::{ManaCost, ManaPool};
    use crate::types::object::{GameObject, VisibilityPolicy};
    use crate::types::player::PlayerState;
    use crate::types::turn::{Phase, Step};
    use crate::types::zone::ZoneKind;
    use std::collections::HashMap;

    fn player(id: PlayerId) -> PlayerState {
        PlayerState {
            player_id: id,
            life: 20,
            mana_pool: ManaPool::default(),
            counters: CounterSet::new(),
            emblems: Vec::new(),
            commander_damage_taken: Vec::new(),
            eliminated: false,
            drew_from_empty_library: false,
            mulligans_taken: 0,
            hand_kept: false,
            commander_casts: HashMap::new(),
        }
    }

    fn object(id: ObjectId, controller: PlayerId, card_types: CardTypeSet, zone: ZoneKind) -> GameObject {
        GameObject {
            object_id: id,
            lineage: crate::types::ids::ObjectLineage { previous: None },
            definition: CardDefinition {
                definition_id: 1,
                name: "Test Object".to_string(),
                mana_cost: ManaCost { symbols: Vec::new() },
                color_indicator: None,
                supertypes: SupertypeSetT::empty(),
                card_types,
                subtypes: Vec::new(),
                oracle_text: String::new(),
                power: Some(PTValue::Fixed(1)),
                toughness: Some(PTValue::Fixed(1)),
                loyalty: None,
                defense: None,
                script_ref: ScriptReference::ByKey("test".to_string()),
                keywords: KeywordSet::empty(),
                target_requirement: None,
                alternative_costs: Vec::new(),
                buyback_cost: None,
                effect: None,
                modes: Vec::new(),
                mode_choice: ModeChoice::Exactly(1),
            },
            copiable_override: None,
            owner: controller,
            controller,
            zone,
            tapped: false,
            flipped: false,
            face_down: false,
            phased_out: false,
            damage_marked: 0,
            deathtouched: false,
            counters: CounterSet::new(),
            attachments: Vec::new(),
            attached_to: None,
            attached_to_player: None,
            timestamp: SequenceNo(0),
            controlled_since_turn: 0,
            is_commander: false,
            is_token: false,
            visibility: VisibilityPolicy { public: true, visible_to: Vec::new() },
            copy_generation: 0,
            entered_via_escape: false,
        }
    }

    fn creature(id: ObjectId, controller: PlayerId) -> GameObject {
        let mut obj = object(id, controller, CardTypeSet::CREATURE, ZoneKind::Battlefield);
        obj.definition.power = Some(PTValue::Fixed(2));
        obj.definition.toughness = Some(PTValue::Fixed(2));
        obj
    }

    fn state_with(objects: Vec<GameObject>) -> GameState {
        let p0 = PlayerId(0);
        let p1 = PlayerId(1);
        GameState {
            players: vec![player(p0), player(p1)],
            turn_order: vec![p0, p1],
            active_player: p0,
            turn_number: 1,
            phase: Phase::Combat,
            step: Step::DeclareAttackers,
            priority_player: p0,
            consecutive_passes: 0,
            lands_played_this_turn: HashMap::new(),
            combat: None,
            last_active_turn_number: HashMap::from([(p0, 1), (p1, 0)]),
            objects: objects.into_iter().map(|o| (o.object_id, o)).collect(),
            zones: Vec::new(),
            stack: Vec::new(),
            continuous_effects: Vec::new(),
            replacement_effects: Vec::new(),
            cost_modifiers: Vec::new(),
            hand_size_modifiers: Vec::new(),
            hand_reveal_modifiers: Vec::new(),
            draw_limit_modifiers: Vec::new(),
            land_play_modifiers: Vec::new(),
            abilities: Vec::new(),
            ledger: Vec::new(),
            rng: RngState { seed: 1, draws_consumed: 0 },
            next_event_id: 0,
            next_sequence_no: 10,
            next_object_id: 1000,
            pending_step_skips: Vec::new(),
            once_per_turn_triggered: std::collections::HashSet::new(),
            commander_return_offered: std::collections::HashSet::new(),
            linked_ability_generation: std::collections::HashMap::new(),
            script_ability_ids: std::collections::HashSet::new(),
            ability_function_zones: std::collections::HashMap::new(),
            off_battlefield_scripts_run: std::collections::HashSet::new(),
            linked_exiles: Vec::new(),
            pending_timing_bypass: None,
            pending_free_cast: None,
        }
    }

    fn combat_with(attackers: Vec<(ObjectId, Target)>) -> CombatState {
        CombatState {
            attackers,
            blocks: Vec::new(),
            declared_defenders: Vec::new(),
            bands: Vec::new(),
            effect_blocked: Vec::new(),
            damage_assigned_this_step: false,
            pending_damage_splits: HashMap::new(),
        }
    }

    #[test]
    fn required_attackers_includes_only_able_creatures_with_attacks_each_combat() {
        let p0 = PlayerId(0);
        let mut required = creature(ObjectId(1), p0);
        required.definition.keywords = KeywordSet::ATTACKS_EACH_COMBAT;
        let tapped = {
            let mut c = creature(ObjectId(2), p0);
            c.definition.keywords = KeywordSet::ATTACKS_EACH_COMBAT;
            c.tapped = true;
            c
        };
        let normal = creature(ObjectId(3), p0);
        let state = state_with(vec![required, tapped, normal]);
        let ids = required_attackers(&state, &ScriptRuntime::new(), p0);
        assert_eq!(ids, vec![ObjectId(1)]);
    }

    #[test]
    fn required_attackers_empty_when_no_legal_defenders() {
        let p0 = PlayerId(0);
        let mut c = creature(ObjectId(1), p0);
        c.definition.keywords = KeywordSet::ATTACKS_EACH_COMBAT;
        let mut state = state_with(vec![c]);
        state.turn_order = vec![p0];
        let ids = required_attackers(&state, &ScriptRuntime::new(), p0);
        assert!(ids.is_empty());
    }

    #[test]
    fn attack_declaration_is_legal_when_required_attackers_attack() {
        let p0 = PlayerId(0);
        let p1 = PlayerId(1);
        let mut required = creature(ObjectId(1), p0);
        required.definition.keywords = KeywordSet::ATTACKS_EACH_COMBAT;
        let state = state_with(vec![required]);
        let attackers = vec![(ObjectId(1), Target::Player(p1))];
        assert!(attack_declaration_is_legal(&state, &ScriptRuntime::new(), p0, &attackers).is_ok());
    }

    #[test]
    fn attack_declaration_is_illegal_when_required_attacker_stays_home() {
        let p0 = PlayerId(0);
        let p1 = PlayerId(1);
        let mut required = creature(ObjectId(1), p0);
        required.definition.keywords = KeywordSet::ATTACKS_EACH_COMBAT;
        let normal = creature(ObjectId(2), p0);
        let state = state_with(vec![required, normal]);
        let attackers = vec![(ObjectId(2), Target::Player(p1))];
        assert!(attack_declaration_is_legal(&state, &ScriptRuntime::new(), p0, &attackers).is_err());
    }

    #[test]
    fn passing_would_skip_required_attack_in_declare_attackers() {
        let p0 = PlayerId(0);
        let mut c = creature(ObjectId(1), p0);
        c.definition.keywords = KeywordSet::ATTACKS_EACH_COMBAT;
        let state = state_with(vec![c]);
        assert!(pass_would_skip_a_required_attack(&state, &ScriptRuntime::new(), p0));
    }

    #[test]
    fn passing_does_not_skip_required_attack_outside_declare_attackers() {
        let p0 = PlayerId(0);
        let mut c = creature(ObjectId(1), p0);
        c.definition.keywords = KeywordSet::ATTACKS_EACH_COMBAT;
        let mut state = state_with(vec![c]);
        state.step = Step::DeclareBlockers;
        state.combat = Some(combat_with(Vec::new()));
        assert!(!pass_would_skip_a_required_attack(&state, &ScriptRuntime::new(), p0));
    }

    #[test]
    fn blockers_needed_is_two_for_menace_one_otherwise() {
        let p0 = PlayerId(0);
        let mut menace = creature(ObjectId(1), p0);
        menace.definition.keywords = KeywordSet::MENACE;
        let normal = creature(ObjectId(2), p0);
        let state = state_with(vec![menace, normal]);
        assert_eq!(blockers_needed(&state, &ScriptRuntime::new(), ObjectId(1)), 2);
        assert_eq!(blockers_needed(&state, &ScriptRuntime::new(), ObjectId(2)), 1);
    }

    #[test]
    fn block_declaration_legal_with_enough_blockers_per_attacker() {
        let p0 = PlayerId(0);
        let p1 = PlayerId(1);
        let attacker = creature(ObjectId(1), p1);
        let blocker1 = creature(ObjectId(2), p0);
        let blocker2 = creature(ObjectId(3), p0);
        let state = state_with(vec![attacker, blocker1, blocker2]);
        let blocks = vec![(ObjectId(2), ObjectId(1)), (ObjectId(3), ObjectId(1))];
        assert!(block_declaration_is_legal(&state, &ScriptRuntime::new(), &blocks).is_ok());
    }

    #[test]
    fn block_declaration_illegal_for_menace_with_single_blocker() {
        let p0 = PlayerId(0);
        let p1 = PlayerId(1);
        let mut attacker = creature(ObjectId(1), p1);
        attacker.definition.keywords = KeywordSet::MENACE;
        let blocker = creature(ObjectId(2), p0);
        let state = state_with(vec![attacker, blocker]);
        let blocks = vec![(ObjectId(2), ObjectId(1))];
        assert!(block_declaration_is_legal(&state, &ScriptRuntime::new(), &blocks).is_err());
    }

    #[test]
    fn blockable_attackers_lists_only_attackers_with_enough_legal_blockers() {
        let p0 = PlayerId(0);
        let p1 = PlayerId(1);
        let attacker1 = creature(ObjectId(1), p1);
        let mut menace = creature(ObjectId(2), p1);
        menace.definition.keywords = KeywordSet::MENACE;
        let blocker = creature(ObjectId(3), p0);
        let state = state_with(vec![attacker1, menace, blocker]);
        let combat = combat_with(vec![(ObjectId(1), Target::Player(p0)), (ObjectId(2), Target::Player(p0))]);
        let result = blockable_attackers(&state, &ScriptRuntime::new(), p0, &combat);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, ObjectId(1));
    }
}
