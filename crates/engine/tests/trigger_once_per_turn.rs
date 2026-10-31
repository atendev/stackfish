//! CR 603.2h: "this ability triggers only once each turn", `ConditionSpec::PrimitiveOncePerTurn` (`Game.Conditions.OncePerTurn(tag)`) enforced in `collect_triggers` against `GameState::once_per_turn_triggered`.
//! Two qualifying events in one batch (an attacker splitting damage across two blockers, CR 510.1c) must trigger once, so a match is recorded as soon as one event in the batch produces it.

use std::collections::HashMap;

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, two_player_state as base_two_player_state, zones};

fn creature_definition(name: &str, power: i32, toughness: i32) -> CardDefinition {
    CardDefinition {
        definition_id: 1,
        name: name.to_string(),
        mana_cost: ManaCost {
            symbols: Vec::new(),
        },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::CREATURE,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: Some(PTValue::Fixed(power)),
        toughness: Some(PTValue::Fixed(toughness)),
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey(name.to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn permanent(id: ObjectId, definition: CardDefinition, owner: PlayerId) -> GameObject {
    GameObject {
        tapped: true,
        ..card_object(id, definition, owner, ZoneKind::Battlefield)
    }
}

fn put_on_battlefield(state: &mut GameState, object: GameObject) {
    let id = object.object_id;
    state
        .zone_mut(ZoneKind::Battlefield, None)
        .unwrap()
        .push_bottom(id);
    state.objects.insert(id, object);
}

fn combat_damage_state(combat: CombatState) -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        phase: Phase::Combat,
        step: Step::CombatDamage,
        combat: Some(combat),
        last_active_turn_number: HashMap::from([(p0, 1), (p1, 0)]),
        ..base_two_player_state(zones(p0, p1, &[ZoneKind::Battlefield]))
    }
}

fn assign_combat_damage(
    engine: &mut Engine,
    player: PlayerId,
    assignments: Vec<(ObjectId, Vec<(ObjectId, u32)>)>,
) -> StepStatus {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::AssignCombatDamage { assignments },
        })
        .expect("turn-based action, no priority required");
    drain(engine)
}

fn pass(engine: &mut Engine, player: PlayerId) {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::PassPriority,
        })
        .unwrap();
}

// Drives real priority passes until turn_number rolls over, with a safety cap so a stuck-priority engine bug fails and does not hang.
fn pass_until_turn_advances(engine: &mut Engine, starting_turn: u32) -> StepStatus {
    let mut status = drain(engine);
    for _ in 0..64 {
        if engine.state().turn_number != starting_turn {
            break;
        }
        let StepStatus::AwaitingAction { player } = status else {
            break;
        };
        pass(engine, player);
        status = drain(engine);
    }
    status
}

fn dealsdamage_ability(source: ObjectId, once_per_turn: bool) -> RegisteredAbility {
    RegisteredAbility {
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
        id: AbilityId(0),
        source,
        kind: AbilityKind::Triggered,
        condition: Some(if once_per_turn {
            ConditionSpec::PrimitiveOncePerTurn(ConditionPrimitive::DealsDamage)
        } else {
            ConditionSpec::Primitive(ConditionPrimitive::DealsDamage)
        }),
        cost: None,
        target: None,
        operation: ScriptHandle(0),
        effect: Some(AbilityEffect::GainLife(1)),
        intervening_if: None,
    }
}

// CR 510.1c
// An attacker with 2 blockers splits its damage: one AssignCombatDamage, one CollectTriggers pass, two DamageDealt events.
#[test]
fn once_per_turn_dealsdamage_trigger_fires_only_once_from_two_events_in_the_same_batch() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let blocker1 = ObjectId(1);
    let blocker2 = ObjectId(2);
    let mut state = combat_damage_state(CombatState {
        attackers: vec![(attacker, Target::Player(p1))],
        blocks: vec![(blocker1, attacker), (blocker2, attacker)],
        declared_defenders: vec![p1],
        bands: Vec::new(),
        effect_blocked: Vec::new(),
        damage_assigned_this_step: false,
        pending_damage_splits: std::collections::HashMap::new(),
    });
    put_on_battlefield(
        &mut state,
        permanent(attacker, creature_definition("Ogre", 3, 3), p0),
    );
    put_on_battlefield(
        &mut state,
        permanent(blocker1, creature_definition("Wall", 0, 5), p1),
    );
    put_on_battlefield(
        &mut state,
        permanent(blocker2, creature_definition("Wall", 0, 5), p1),
    );
    state.abilities.push(dealsdamage_ability(attacker, true));

    let mut engine = Engine::new(state);
    let status = assign_combat_damage(
        &mut engine,
        p0,
        vec![(attacker, vec![(blocker1, 2), (blocker2, 1)])],
    );
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));

    // Both DamageDealt events really happened (not a setup mistake)...
    let damage_events: Vec<u32> = engine
        .state()
        .ledger
        .iter()
        .filter_map(|e| match e.kind {
            GameEventKind::DamageDealt { source, amount, .. } if source == attacker => Some(amount),
            _ => None,
        })
        .collect();
    assert_eq!(
        damage_events,
        vec![2, 1],
        "the attacker's split damage produced two DamageDealt events"
    );

    // ...but only one trigger was placed for them, not two.
    assert_eq!(
        engine.state().stack.len(),
        1,
        "once-per-turn capped the trigger to a single instance"
    );

    pass(&mut engine, p0);
    drain(&mut engine);
    pass(&mut engine, p1);
    drain(&mut engine);
    assert!(engine.state().stack.is_empty());
    assert_eq!(
        engine.state().player(p0).unwrap().life,
        21,
        "GainLife(1) applied exactly once"
    );
}

// Without the once-per-turn cap the same two events produce two triggers, so the capped test is not passing for an unrelated reason.
#[test]
fn without_the_cap_the_same_two_events_fire_the_trigger_twice() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let blocker1 = ObjectId(1);
    let blocker2 = ObjectId(2);
    let mut state = combat_damage_state(CombatState {
        attackers: vec![(attacker, Target::Player(p1))],
        blocks: vec![(blocker1, attacker), (blocker2, attacker)],
        declared_defenders: vec![p1],
        bands: Vec::new(),
        effect_blocked: Vec::new(),
        damage_assigned_this_step: false,
        pending_damage_splits: std::collections::HashMap::new(),
    });
    put_on_battlefield(
        &mut state,
        permanent(attacker, creature_definition("Ogre", 3, 3), p0),
    );
    put_on_battlefield(
        &mut state,
        permanent(blocker1, creature_definition("Wall", 0, 5), p1),
    );
    put_on_battlefield(
        &mut state,
        permanent(blocker2, creature_definition("Wall", 0, 5), p1),
    );
    state.abilities.push(dealsdamage_ability(attacker, false));

    let mut engine = Engine::new(state);
    let status = assign_combat_damage(
        &mut engine,
        p0,
        vec![(attacker, vec![(blocker1, 2), (blocker2, 1)])],
    );

    // Both triggers share a controller (p0), so CR 603.3b pauses for p0 to order them, and the group size proves two triggers matched.
    match status {
        StepStatus::AwaitingTriggerOrderChoice { player, group } => {
            assert_eq!(player, p0);
            assert_eq!(
                group.len(),
                2,
                "uncapped: both DamageDealt events triggered their own instance"
            );
        }
        other => panic!(
            "expected AwaitingTriggerOrderChoice for two same-controller triggers, got {other:?}"
        ),
    }
}

// CR 603.2h
// The cap resets with the turn.
#[test]
fn once_per_turn_trigger_resets_after_a_new_turn_begins() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let blocker1 = ObjectId(1);
    let blocker2 = ObjectId(2);
    let mut state = combat_damage_state(CombatState {
        attackers: vec![(attacker, Target::Player(p1))],
        blocks: vec![(blocker1, attacker), (blocker2, attacker)],
        declared_defenders: vec![p1],
        bands: Vec::new(),
        effect_blocked: Vec::new(),
        damage_assigned_this_step: false,
        pending_damage_splits: std::collections::HashMap::new(),
    });
    put_on_battlefield(
        &mut state,
        permanent(attacker, creature_definition("Ogre", 3, 3), p0),
    );
    put_on_battlefield(
        &mut state,
        permanent(blocker1, creature_definition("Wall", 0, 5), p1),
    );
    put_on_battlefield(
        &mut state,
        permanent(blocker2, creature_definition("Wall", 0, 5), p1),
    );
    state.abilities.push(dealsdamage_ability(attacker, true));
    // Simulates "already triggered this turn" directly, since this test only checks whether a fresh turn clears the record.
    state.once_per_turn_triggered.insert(AbilityId(0));

    let mut engine = Engine::new(state);
    let status = assign_combat_damage(
        &mut engine,
        p0,
        vec![(attacker, vec![(blocker1, 2), (blocker2, 1)])],
    );
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert!(
        engine.state().stack.is_empty(),
        "already-triggered-this-turn correctly suppressed it"
    );

    // Walk the real turn structure until the turn rolls over.
    let status = pass_until_turn_advances(&mut engine, 1);
    assert!(
        matches!(status, StepStatus::AwaitingAction { player } if player == p1),
        "turn rotated, got {status:?}"
    );
    assert_eq!(engine.state().turn_number, 2);
    assert!(
        !engine
            .state()
            .once_per_turn_triggered
            .contains(&AbilityId(0)),
        "the cap reset with the new turn"
    );
}
