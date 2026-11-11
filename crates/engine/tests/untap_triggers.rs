//! CR 502.4: the Untap step's events route through the real `CollectTriggers` pipeline (`PipelineExit::AdvanceTurnStep`) without opening a priority window. `ConditionPrimitive::BeginningOfYourUntap` and `BeginningOfEachUntap` mirror upkeep_triggers.rs.

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
fn beginning_of_your_untap_trigger_is_already_on_the_stack_by_the_time_upkeep_priority_opens() {
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
        text: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
        id: AbilityId(0),
        source: permanent,
        kind: AbilityKind::Triggered,
        condition: Some(ConditionSpec::Primitive(
            ConditionPrimitive::BeginningOfYourUntap,
        )),
        cost: None,
        target: None,
        operation: ScriptHandle(0),
        effect: Some(AbilityEffect::GainLife(1)),
        intervening_if: None,
    });

    let mut engine = Engine::new(state);
    // One pass-round from PreGame lands on Upkeep (CR 502.4), and the untap trigger is already on the stack by then (CR 603.3).
    let status = pass_both_and_drain(&mut engine, p0, p1);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert_eq!(engine.state().phase, Phase::Beginning);
    assert_eq!(engine.state().step, Step::Upkeep);
    assert_eq!(
        engine.state().stack.len(),
        1,
        "the untap trigger was collected and pushed before priority ever opened"
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
        "GainLife(1) applied from the untap trigger"
    );
}

#[test]
fn beginning_of_your_untap_trigger_does_not_fire_for_a_different_controller() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let permanent = ObjectId(0);
    let mut state = two_player_state();
    // Controlled by p1, but this is p0's untap, so BeginningOfYourUntap must not fire for p1.
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
        text: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
        id: AbilityId(1),
        source: permanent,
        kind: AbilityKind::Triggered,
        condition: Some(ConditionSpec::Primitive(
            ConditionPrimitive::BeginningOfYourUntap,
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
        "p1's ability shouldn't fire on p0's own untap"
    );
}

#[test]
fn beginning_of_each_untap_trigger_fires_regardless_of_whose_turn_it_is() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let permanent = ObjectId(0);
    let mut state = two_player_state();
    // Controlled by p1 while p0 is active, so EachUntap fires anyway, unlike BeginningOfYourUntap.
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
        text: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
        id: AbilityId(2),
        source: permanent,
        kind: AbilityKind::Triggered,
        condition: Some(ConditionSpec::Primitive(
            ConditionPrimitive::BeginningOfEachUntap,
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
        "EachUntap fires even though its controller isn't the active player"
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
