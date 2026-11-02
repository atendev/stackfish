//! CR 502.1: `ConditionPrimitive::BeginningOfYourUpkeep` and `BeginningOfEachUpkeep`. `AdvanceTurnStep` routes its events through the real `CollectTriggers` pipeline, so a step-boundary trigger can fire. The construction mirrors beginning_phase.rs.

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::*;
use stackfish_testutil::{
    card_object, creature_definition, pass_both_and_drain,
    two_player_state as base_two_player_state, zones,
};

fn two_player_state() -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        phase: Phase::PreGame,
        ..base_two_player_state(zones(
            p0,
            p1,
            &[ZoneKind::Hand, ZoneKind::Library, ZoneKind::Battlefield],
        ))
    }
}

#[test]
fn beginning_of_your_upkeep_trigger_fires_and_resolves_for_its_controller() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let permanent = ObjectId(0);
    let mut state = two_player_state();
    state.objects.insert(
        permanent,
        card_object(
            permanent,
            creature_definition("Bear"),
            p0,
            ZoneKind::Battlefield,
        ),
    );
    state
        .zone_mut(ZoneKind::Battlefield, None)
        .unwrap()
        .push_bottom(permanent);
    state.abilities.push(RegisteredAbility {
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
        id: AbilityId(0),
        source: permanent,
        kind: AbilityKind::Triggered,
        condition: Some(ConditionSpec::Primitive(
            ConditionPrimitive::BeginningOfYourUpkeep,
        )),
        cost: None,
        target: None,
        operation: ScriptHandle(0),
        effect: Some(AbilityEffect::GainLife(1)),
        intervening_if: None,
    });

    let mut engine = Engine::new(state);
    // One pass-round from PreGame lands on Upkeep (Untap opens no priority window), and the trigger is on the stack by then.
    let status = pass_both_and_drain(&mut engine, p0, p1);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert_eq!(engine.state().phase, Phase::Beginning);
    assert_eq!(engine.state().step, Step::Upkeep);
    assert_eq!(
        engine.state().stack.len(),
        1,
        "the upkeep trigger was collected and pushed before priority opened"
    );

    let status = pass_both_and_drain(&mut engine, p0, p1);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert!(
        engine.state().stack.is_empty(),
        "the triggered ability resolved"
    );
    assert_eq!(
        engine.state().player(p0).unwrap().life,
        21,
        "GainLife(1) applied at p0's own upkeep"
    );
}

#[test]
fn beginning_of_your_upkeep_trigger_does_not_fire_for_a_different_controller() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let permanent = ObjectId(0);
    let mut state = two_player_state();
    // Controlled by p1, but this is p0's upkeep, so BeginningOfYourUpkeep must not fire for p1.
    state.objects.insert(
        permanent,
        card_object(
            permanent,
            creature_definition("Bear"),
            p1,
            ZoneKind::Battlefield,
        ),
    );
    state
        .zone_mut(ZoneKind::Battlefield, None)
        .unwrap()
        .push_bottom(permanent);
    state.abilities.push(RegisteredAbility {
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
        id: AbilityId(1),
        source: permanent,
        kind: AbilityKind::Triggered,
        condition: Some(ConditionSpec::Primitive(
            ConditionPrimitive::BeginningOfYourUpkeep,
        )),
        cost: None,
        target: None,
        operation: ScriptHandle(0),
        effect: Some(AbilityEffect::GainLife(1)),
        intervening_if: None,
    });

    let mut engine = Engine::new(state);
    let status = pass_both_and_drain(&mut engine, p0, p1);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert_eq!(engine.state().step, Step::Upkeep);
    assert!(
        engine.state().stack.is_empty(),
        "p1's ability shouldn't fire on p0's own upkeep"
    );
}

#[test]
fn beginning_of_each_upkeep_trigger_fires_regardless_of_whose_turn_it_is() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let permanent = ObjectId(0);
    let mut state = two_player_state();
    // Controlled by p1 while p0 is active, so EachUpkeep fires anyway, unlike BeginningOfYourUpkeep.
    state.objects.insert(
        permanent,
        card_object(
            permanent,
            creature_definition("Bear"),
            p1,
            ZoneKind::Battlefield,
        ),
    );
    state
        .zone_mut(ZoneKind::Battlefield, None)
        .unwrap()
        .push_bottom(permanent);
    state.abilities.push(RegisteredAbility {
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
        id: AbilityId(2),
        source: permanent,
        kind: AbilityKind::Triggered,
        condition: Some(ConditionSpec::Primitive(
            ConditionPrimitive::BeginningOfEachUpkeep,
        )),
        cost: None,
        target: None,
        operation: ScriptHandle(0),
        effect: Some(AbilityEffect::GainLife(1)),
        intervening_if: None,
    });

    let mut engine = Engine::new(state);
    let status = pass_both_and_drain(&mut engine, p0, p1);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert_eq!(engine.state().step, Step::Upkeep);
    assert_eq!(
        engine.state().stack.len(),
        1,
        "EachUpkeep fires even though its controller isn't the active player"
    );

    let status = pass_both_and_drain(&mut engine, p0, p1);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert!(engine.state().stack.is_empty());
    assert_eq!(
        engine.state().player(p1).unwrap().life,
        21,
        "GainLife(1) applied to the ability's own controller, p1"
    );
}
