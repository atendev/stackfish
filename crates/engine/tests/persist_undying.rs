//! CR 702.79a Persist and CR 702.93a Undying: when the permanent dies with no -1/-1 (+1/+1) counters it returns under its owner's control with one. `InterveningIfPrimitive::HadNoCountersOfKind` checks the object as it died (`source_lki`, `find_departed_lki`), and `AbilityEffect::ReturnSelfWithCounter` is the mover.
//! Trigger collection is as in dies_triggers.rs.

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::*;
use stackfish_testutil::{battlefield_graveyard_exile_zones, card_object, drain, minimal_card_definition, two_player_state as base_two_player_state};

fn creature_definition(name: &str, power: i32, toughness: i32) -> CardDefinition {
    CardDefinition {
        power: Some(PTValue::Fixed(power)),
        toughness: Some(PTValue::Fixed(toughness)),
        ..minimal_card_definition(1, name, name, ManaCost { symbols: Vec::new() }, CardTypeSet::CREATURE)
    }
}

fn creature(id: ObjectId, owner: PlayerId, controller: PlayerId, toughness: i32, damage_marked: u32) -> GameObject {
    GameObject {
        controller,
        damage_marked,
        ..card_object(id, creature_definition("Test Creature", 2, toughness), owner, ZoneKind::Battlefield)
    }
}

fn creature_with_counter(
    id: ObjectId,
    owner: PlayerId,
    toughness: i32,
    damage_marked: u32,
    kind: CounterKind,
) -> GameObject {
    let mut counters = CounterSet::new();
    counters.add(kind, 1, SequenceNo(0));
    GameObject { counters, ..creature(id, owner, owner, toughness, damage_marked) }
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

fn put_on_battlefield(state: &mut GameState, object: GameObject) {
    let id = object.object_id;
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(id);
    state.objects.insert(id, object);
}

fn persist_undying_ability(source: ObjectId, kind: CounterKind) -> RegisteredAbility {
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
        effect: Some(AbilityEffect::ReturnSelfWithCounter(kind.clone())),
        intervening_if: Some(InterveningIfSpec::Primitive(InterveningIfPrimitive::HadNoCountersOfKind(kind))),
    }
}

fn pass(engine: &mut Engine, player: PlayerId) {
    engine.submit_action(PlayerAction { player, kind: PlayerActionKind::PassPriority }).unwrap();
}

fn run_death_and_resolution(engine: &mut Engine, p0: PlayerId, p1: PlayerId) -> StepStatus {
    pass(engine, p0);
    drain(engine);
    pass(engine, p1);
    drain(engine)
}

#[test]
fn persist_returns_with_a_minus_one_minus_one_counter_after_dying_with_none() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let bear = ObjectId(0);
    let mut state = base_state();
    // Lethal damage is already marked, so PassPriority cascades into SbaLoop and CR 704.5g kills the creature.
    put_on_battlefield(&mut state, creature(bear, p0, p0, 2, 2));
    state.abilities.push(persist_undying_ability(bear, CounterKind::MinusOneMinusOne));

    let mut engine = Engine::new(state);
    let status = run_death_and_resolution(&mut engine, p0, p1);

    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert!(engine.state().stack.is_empty(), "the Persist trigger resolved");
    assert!(engine.state().object(bear).is_none(), "the old incarnation never comes back - CR 400.7");

    let battlefield = engine.state().zone(ZoneKind::Battlefield, None).unwrap();
    assert_eq!(battlefield.contents.len(), 1, "returned to the battlefield as exactly one new object");
    let new_id = battlefield.contents[0];
    let returned = engine.state().object(new_id).unwrap();
    assert_eq!(returned.owner, p0);
    assert_eq!(returned.controller, p0);
    assert!(!returned.tapped);
    assert_eq!(returned.damage_marked, 0, "a new object has no memory of the damage that killed it");
    assert_eq!(
        returned.counters.count(&CounterKind::MinusOneMinusOne),
        1,
        "persist's own counter, exactly one"
    );
    assert_eq!(
        returned.controlled_since_turn,
        engine.state().turn_number,
        "a freshly entered permanent is summoning sick"
    );

    let graveyard = engine.state().zone(ZoneKind::Graveyard, Some(p0)).unwrap();
    assert!(graveyard.contents.is_empty(), "the card left the graveyard - it's back on the battlefield");
}

#[test]
fn persist_does_not_trigger_when_the_creature_already_had_a_minus_one_minus_one_counter() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let bear = ObjectId(0);
    let mut state = base_state();
    // Base toughness 2 with one -1/-1 counter (effective 1), so 1 damage is lethal.
    put_on_battlefield(&mut state, creature_with_counter(bear, p0, 2, 1, CounterKind::MinusOneMinusOne));
    state.abilities.push(persist_undying_ability(bear, CounterKind::MinusOneMinusOne));

    let mut engine = Engine::new(state);
    let status = run_death_and_resolution(&mut engine, p0, p1);

    // One PassPriority round was needed with no trigger to resolve, so the second round is a harmless no-op ending on p0.
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert!(engine.state().stack.is_empty(), "CR 603.4: the intervening-if failed, so the ability never went on the stack at all");
    assert!(
        engine.state().zone(ZoneKind::Battlefield, None).unwrap().contents.is_empty(),
        "nothing returned - the creature stays dead"
    );
    let graveyard = engine.state().zone(ZoneKind::Graveyard, Some(p0)).unwrap();
    assert_eq!(graveyard.contents.len(), 1, "the card is still sitting in the graveyard");
}

#[test]
fn undying_returns_with_a_plus_one_plus_one_counter_after_dying_with_none() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let bear = ObjectId(0);
    let mut state = base_state();
    put_on_battlefield(&mut state, creature(bear, p0, p0, 2, 2));
    state.abilities.push(persist_undying_ability(bear, CounterKind::PlusOnePlusOne));

    let mut engine = Engine::new(state);
    run_death_and_resolution(&mut engine, p0, p1);

    let battlefield = engine.state().zone(ZoneKind::Battlefield, None).unwrap();
    assert_eq!(battlefield.contents.len(), 1);
    let returned = engine.state().object(battlefield.contents[0]).unwrap();
    assert_eq!(returned.counters.count(&CounterKind::PlusOnePlusOne), 1);
    assert_eq!(returned.counters.count(&CounterKind::MinusOneMinusOne), 0);
}

#[test]
fn undying_does_not_trigger_when_the_creature_already_had_a_plus_one_plus_one_counter() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let bear = ObjectId(0);
    let mut state = base_state();
    // Base toughness 2 with one +1/+1 counter (effective 3), so 3 damage is lethal.
    put_on_battlefield(&mut state, creature_with_counter(bear, p0, 2, 3, CounterKind::PlusOnePlusOne));
    state.abilities.push(persist_undying_ability(bear, CounterKind::PlusOnePlusOne));

    let mut engine = Engine::new(state);
    run_death_and_resolution(&mut engine, p0, p1);

    assert!(engine.state().stack.is_empty());
    assert!(engine.state().zone(ZoneKind::Battlefield, None).unwrap().contents.is_empty());
}

#[test]
fn persist_returns_a_stolen_creature_under_its_owners_control_not_the_thiefs() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let bear = ObjectId(0);
    let mut state = base_state();
    // Owned by p0, controlled by p1 (a stolen creature) - CR 800.4a.
    put_on_battlefield(&mut state, creature(bear, p0, p1, 2, 2));
    state.abilities.push(persist_undying_ability(bear, CounterKind::MinusOneMinusOne));

    let mut engine = Engine::new(state);
    run_death_and_resolution(&mut engine, p0, p1);

    let battlefield = engine.state().zone(ZoneKind::Battlefield, None).unwrap();
    assert_eq!(battlefield.contents.len(), 1);
    let returned = engine.state().object(battlefield.contents[0]).unwrap();
    assert_eq!(returned.owner, p0);
    assert_eq!(returned.controller, p0, "returns under the owner's control, not the temporary controller's");
}

#[test]
fn persist_is_a_clean_no_op_if_the_card_left_the_graveyard_before_it_resolves() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let bear = ObjectId(0);
    let mut state = base_state();
    put_on_battlefield(&mut state, creature(bear, p0, p0, 2, 2));
    state.abilities.push(persist_undying_ability(bear, CounterKind::MinusOneMinusOne));

    let mut engine = Engine::new(state);
    pass(&mut engine, p0);
    drain(&mut engine);
    assert_eq!(engine.state().stack.len(), 1, "the Persist trigger is on the stack, not yet resolved");

    // Simulates "exiled in response": the graveyard object is gone before the trigger resolves.
    let graveyard_id = engine
        .state()
        .zone(ZoneKind::Graveyard, Some(p0))
        .unwrap()
        .contents[0];
    engine.state_mut().zone_mut(ZoneKind::Graveyard, Some(p0)).unwrap().remove(graveyard_id);
    engine.state_mut().objects.remove(&graveyard_id);

    pass(&mut engine, p1);
    let status = drain(&mut engine);

    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert!(engine.state().stack.is_empty(), "the ability still resolved (as a no-op), not stuck");
    assert!(
        engine.state().zone(ZoneKind::Battlefield, None).unwrap().contents.is_empty(),
        "nothing to return - the card was already gone"
    );
}
