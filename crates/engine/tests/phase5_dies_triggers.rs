//! CR 603.10: "dies" and leaves-the-battlefield conditions via last-known information.
//! `retire_object` purges a departing object's triggered abilities, so `Engine` stashes them in `departed_triggered_abilities` for `Dies` and `LeavesBattlefield` to match against `ZoneChange::last_known_info`.
// TODO: sacrifice, discard-as-cost and a resolving spell's other departures discard `retire_object`'s return value
// TODO: filtered conditions such as "whenever a creature you control dies"

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::*;
use stackfish_testutil::{
    battlefield_graveyard_exile_zones, card_object, drain, minimal_card_definition, two_player_state as base_two_player_state,
};

fn creature_definition(name: &str, power: i32, toughness: i32) -> CardDefinition {
    CardDefinition {
        power: Some(PTValue::Fixed(power)),
        toughness: Some(PTValue::Fixed(toughness)),
        ..minimal_card_definition(1, name, name, ManaCost { symbols: Vec::new() }, CardTypeSet::CREATURE)
    }
}

fn creature(id: ObjectId, owner: PlayerId, toughness: i32, damage_marked: u32) -> GameObject {
    GameObject {
        damage_marked,
        ..card_object(id, creature_definition("Test Creature", 2, toughness), owner, ZoneKind::Battlefield)
    }
}

fn base_state() -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        step: Step::PreCombatMain,
        last_active_turn_number: std::collections::HashMap::from([(p0, 1), (p1, 1)]),
        ..base_two_player_state(battlefield_graveyard_exile_zones(p0, p1))
    }
}

fn combat_damage_state(combat: CombatState) -> GameState {
    let mut state = base_state();
    state.phase = Phase::Combat;
    state.step = Step::CombatDamage;
    state.combat = Some(combat);
    state
}

fn put_on_battlefield(state: &mut GameState, object: GameObject) {
    let id = object.object_id;
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(id);
    state.objects.insert(id, object);
}

fn dies_ability(source: ObjectId) -> RegisteredAbility {
    RegisteredAbility {
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
        id: AbilityId(0),
        source,
        kind: AbilityKind::Triggered,
        condition: Some(ConditionSpec::Primitive(ConditionPrimitive::Dies)),
        cost: None,
        target: None,
        operation: ScriptHandle(0),
        effect: Some(AbilityEffect::GainLife(1)),
        intervening_if: None,
    }
}

fn leaves_battlefield_ability(source: ObjectId) -> RegisteredAbility {
    RegisteredAbility {
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
        id: AbilityId(1),
        source,
        kind: AbilityKind::Triggered,
        condition: Some(ConditionSpec::Primitive(ConditionPrimitive::LeavesBattlefield)),
        cost: None,
        target: None,
        operation: ScriptHandle(0),
        effect: Some(AbilityEffect::GainLife(1)),
        intervening_if: None,
    }
}

fn would_die_exile_effect(source: ObjectId) -> ReplacementEffect {
    ReplacementEffect {
        source,
        spec: ReplacementSpec::Primitive {
            filter: EventFilter::WouldDie {
                affected: EffectFilter {  card_types: Some(CardTypeSet::CREATURE), controller: None, self_only: false, subtypes: None, excluded_subtypes: None, supertypes: None, excluded_supertypes: None, },
            },
            payload: ReplacementPayload::ReplaceZoneDestination(ZoneKind::Exile),
        },
        consumption: ConsumptionPolicy::Repeatable,
    }
}

fn pass(engine: &mut Engine, player: PlayerId) {
    engine.submit_action(PlayerAction { player, kind: PlayerActionKind::PassPriority }).unwrap();
}

#[test]
fn dies_trigger_fires_and_resolves_after_real_lethal_damage() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let bear = ObjectId(0);
    let mut state = base_state();
    // Lethal damage is already marked, so PassPriority cascades into SbaLoop and CR 704.5g kills the creature.
    put_on_battlefield(&mut state, creature(bear, p0, 2, 2));
    state.abilities.push(dies_ability(bear));

    let mut engine = Engine::new(state);
    // A bare PassPriority hands priority to the next player in turn order (CR 117.3c), so it lands on p1 even though p0's Dies trigger fired.
    pass(&mut engine, p0);
    let status = drain(&mut engine);

    assert!(
        matches!(status, StepStatus::AwaitingAction { player } if player == p1),
        "the Dies trigger must have been collected and pushed to the stack: {status:?}"
    );
    assert_eq!(engine.state().stack.len(), 1, "the Dies trigger was collected and pushed");
    assert!(engine.state().object(bear).is_none(), "the bear itself is gone - died to the graveyard");

    // Both players passed with the trigger on the stack, so it resolves and priority returns to the active player (CR 117.3b).
    pass(&mut engine, p1);
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert!(engine.state().stack.is_empty(), "the triggered ability resolved");
    assert_eq!(engine.state().player(p0).unwrap().life, 21, "GainLife(1) applied to the bear's own controller");
}

#[test]
fn ability_on_a_creature_that_does_not_die_never_fires() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let bear = ObjectId(0);
    let mut state = base_state();
    // Toughness 2, no damage marked - survives, never produces a ZoneChange.
    put_on_battlefield(&mut state, creature(bear, p0, 2, 0));
    state.abilities.push(dies_ability(bear));

    let mut engine = Engine::new(state);
    pass(&mut engine, p0);
    let status = drain(&mut engine);

    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p1));
    assert!(engine.state().stack.is_empty(), "no death occurred, so the Dies trigger never matched");
    assert!(engine.state().object(bear).is_some(), "the bear is still alive");
}

#[test]
fn leaves_battlefield_trigger_fires_when_a_replacement_exiles_instead_of_killing() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let bear = ObjectId(0);
    let mut state = base_state();
    put_on_battlefield(&mut state, creature(bear, p0, 2, 2));
    state.abilities.push(leaves_battlefield_ability(bear));
    state.replacement_effects.push(would_die_exile_effect(bear));

    let mut engine = Engine::new(state);
    pass(&mut engine, p0);
    let status = drain(&mut engine);

    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p1));
    assert_eq!(engine.state().stack.len(), 1, "LeavesBattlefield must fire on the exile, not just a death");
    assert!(
        engine.state().zone(ZoneKind::Exile, None).unwrap().contents.len() == 1,
        "the bear actually went to exile, not the graveyard"
    );

    pass(&mut engine, p1);
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert!(engine.state().stack.is_empty(), "the triggered ability resolved");
    assert_eq!(engine.state().player(p0).unwrap().life, 21, "GainLife(1) applied");
}

#[test]
fn dies_does_not_fire_when_a_replacement_exiles_instead_of_killing() {
    // Dies is battlefield to graveyard, so a departure redirected to exile does not satisfy it.
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let bear = ObjectId(0);
    let mut state = base_state();
    put_on_battlefield(&mut state, creature(bear, p0, 2, 2));
    state.abilities.push(dies_ability(bear));
    state.replacement_effects.push(would_die_exile_effect(bear));

    let mut engine = Engine::new(state);
    pass(&mut engine, p0);
    let status = drain(&mut engine);

    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p1));
    assert!(engine.state().stack.is_empty(), "Dies must not match a redirected-to-exile departure");
}

#[test]
fn deals_damage_and_dies_triggers_both_fire_from_the_same_combat_damage_step() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let blocker = ObjectId(1);
    let mut state = combat_damage_state(CombatState {
        attackers: vec![(attacker, Target::Player(p1))],
        blocks: vec![(blocker, attacker)],
        declared_defenders: vec![p1],
        bands: Vec::new(), effect_blocked: Vec::new(), damage_assigned_this_step: false, pending_damage_splits: std::collections::HashMap::new(),
    });
    put_on_battlefield(&mut state, creature(attacker, p0, 5, 0));
    put_on_battlefield(&mut state, creature(blocker, p1, 1, 0)); // dies to 3 damage
    state.abilities.push(RegisteredAbility {
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
        id: AbilityId(2),
        source: attacker,
        kind: AbilityKind::Triggered,
        condition: Some(ConditionSpec::Primitive(ConditionPrimitive::DealsDamage)),
        cost: None,
        target: None,
        operation: ScriptHandle(0),
        effect: Some(AbilityEffect::GainLife(1)),
        intervening_if: None,
    });
    state.abilities.push(dies_ability(blocker));

    let mut engine = Engine::new(state);
    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::AssignCombatDamage { assignments: Vec::new() },
        })
        .expect("turn-based action, no priority required");
    let status = drain(&mut engine);

    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert_eq!(engine.state().stack.len(), 2, "both the DealsDamage and Dies triggers were collected");
    assert!(engine.state().object(blocker).is_none(), "the blocker died");

    // Each of the two triggers needs both players to pass in a row (CR 117.3b), then priority returns to the active player.
    pass(&mut engine, p0);
    drain(&mut engine);
    pass(&mut engine, p1);
    drain(&mut engine);
    pass(&mut engine, p0);
    drain(&mut engine);
    pass(&mut engine, p1);
    let status = drain(&mut engine);

    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0), "{status:?}");
    assert!(engine.state().stack.is_empty(), "both triggers resolved");
    assert_eq!(engine.state().player(p0).unwrap().life, 21, "DealsDamage: attacker's controller (p0) gained 1 life");
    assert_eq!(engine.state().player(p1).unwrap().life, 21, "Dies: blocker's controller (p1) gained 1 life");
}
