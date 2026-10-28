//! CR 602.2: `ActivateAbility` with a bare `{T}` cost uses the stack and pauses on `AwaitingPayment`.
//! The tap happens at `submit_payment` commit time. Other costs live in activate_ability_costs.rs.

use std::collections::VecDeque;

use stackfish_engine::engine::ActionError;
use stackfish_engine::commands::Rejected;
use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, minimal_card_definition, single_player_state};

fn creature_definition() -> CardDefinition {
    CardDefinition {
        power: Some(PTValue::Fixed(1)),
        toughness: Some(PTValue::Fixed(1)),
        oracle_text: "{T}: You gain 1 life.".to_string(),
        ..minimal_card_definition(1, "Test Cleric", "test_cleric", ManaCost { symbols: Vec::new() }, CardTypeSet::CREATURE)
    }
}

// Controlled since before turn 1, so never summoning sick. The sickness test overrides it.
fn permanent_object(id: ObjectId, definition: CardDefinition, owner: PlayerId) -> GameObject {
    card_object(id, definition, owner, ZoneKind::Battlefield)
}

fn state_with_source(source: ObjectId, controller: PlayerId, ability_owner: ObjectId) -> GameState {
    let mut objects = std::collections::HashMap::new();
    objects.insert(source, permanent_object(source, creature_definition(), controller));
    GameState {
        objects,
        abilities: vec![RegisteredAbility {
            modes: Vec::new(),
            mode_choice: ModeChoice::Exactly(1),
            id: AbilityId(0),
            source: ability_owner,
            kind: AbilityKind::Activated,
            condition: None,
            cost: Some(CostSpec {
                mana: None,
                additional: vec![AdditionalCost::TapSelf],
                alternative: Vec::new(),
            }),
            target: None,
            operation: ScriptHandle(0),
            effect: Some(AbilityEffect::GainLife(1)),
            intervening_if: None,
        }],
        ..single_player_state(controller, vec![Zone { kind: ZoneKind::Battlefield, owner: None, contents: VecDeque::from([source]) }])
    }
}

fn activate(engine: &mut Engine, player: PlayerId, source: ObjectId, targets: Vec<Target>) {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::ActivateAbility { source, ability_index: 0, targets, chosen_colors: Vec::new(),
            x_value: None,
            },
        })
        .expect("player holds priority");
}

fn activate_and_drain(
    engine: &mut Engine,
    player: PlayerId,
    source: ObjectId,
    targets: Vec<Target>,
) -> StepStatus {
    activate(engine, player, source, targets);
    drain(engine)
}

fn try_activate(engine: &mut Engine, player: PlayerId, source: ObjectId, targets: Vec<Target>) -> Result<(), ActionError> {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::ActivateAbility { source, ability_index: 0, targets, chosen_colors: Vec::new(),
            x_value: None,
            },
        })
        
}

fn try_activate_and_drain(
    engine: &mut Engine,
    player: PlayerId,
    source: ObjectId,
    targets: Vec<Target>,
) -> Result<StepStatus, ActionError> {
    try_activate(engine, player, source, targets)?;
    Ok(drain(engine))
}

// Asserts the engine paused on `AwaitingPayment` before paying (CR 602.2f-h).
fn pay_tap_and_drain(engine: &mut Engine, source: ObjectId) -> StepStatus {
    let status = drain(engine);
    assert!(
        matches!(status, StepStatus::AwaitingPayment { .. }),
        "expected a pause for the ability's {{T}} cost, got {status:?}"
    );
    engine
        .submit_payment(PaymentPlan { payments: vec![CostPayment::Tap(source)] })
        .expect("a Tap(source) payment pays a bare TapSelf cost");
    drain(engine)
}

#[test]
fn tap_ability_pushes_stack_object_and_awaits_payment() {
    let p0 = PlayerId(0);
    let source = ObjectId(0);
    let state = state_with_source(source, p0, source);
    let mut engine = Engine::new(state);

    let status = activate_and_drain(&mut engine, p0, source, Vec::new());
    assert!(matches!(status, StepStatus::AwaitingPayment { player, .. } if player == p0));

    // The tap is a cost payment, so it has not happened yet (CR 602.2f-h).
    assert!(!engine.state().object(source).unwrap().tapped, "source doesn't tap until payment commits");
    assert_eq!(engine.state().consecutive_passes, 0, "activating breaks the pass chain (CR 117.4)");
    assert_eq!(engine.state().priority_player, p0, "priority stays with the activator (CR 117.3c)");

    let events: Vec<_> = engine.state().ledger.iter().map(|e| &e.kind).collect();
    assert!(matches!(events[0], GameEventKind::StackObjectPushed { .. }));

    let status = pay_tap_and_drain(&mut engine, source);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert!(engine.state().object(source).unwrap().tapped, "source taps once the cost is paid");

    let events: Vec<_> = engine.state().ledger.iter().map(|e| &e.kind).collect();
    assert!(events.iter().any(|e| matches!(e, GameEventKind::Tapped { object } if *object == source)));
    assert!(events.iter().any(|e| matches!(e, GameEventKind::CostPaid { .. })));
}

#[test]
fn tap_ability_resolves_gain_life_via_fast_path_and_empties_stack() {
    let p0 = PlayerId(0);
    let source = ObjectId(0);
    let state = state_with_source(source, p0, source);
    let mut engine = Engine::new(state);

    activate(&mut engine, p0, source, Vec::new());
    pay_tap_and_drain(&mut engine, source);
    // Passing priority with a non-empty stack resolves the ability.
    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(&mut engine);

    assert_eq!(engine.state().player(p0).unwrap().life, 21, "gain 1 life resolved");
    assert!(engine.state().stack.is_empty(), "stack empties once the ability resolves");
    assert!(engine.state().object(source).unwrap().tapped);
}

#[test]
fn reject_when_actor_does_not_control_source() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let source = ObjectId(0);
    let mut state = state_with_source(source, p0, source);
    state.object_mut(source).unwrap().controller = p1;

    let mut engine = Engine::new(state);
    let err = try_activate_and_drain(&mut engine, p0, source, Vec::new()).unwrap_err();
    assert!(matches!(err, ActionError::Rejected(Rejected::IllegalAction(_))));
    assert!(!engine.state().object(source).unwrap().tapped, "no partial mutation");
}

#[test]
fn reject_when_source_already_tapped() {
    let p0 = PlayerId(0);
    let source = ObjectId(0);
    let mut state = state_with_source(source, p0, source);
    state.object_mut(source).unwrap().tapped = true;

    let mut engine = Engine::new(state);
    let err = try_activate_and_drain(&mut engine, p0, source, Vec::new()).unwrap_err();
    assert!(matches!(err, ActionError::Rejected(Rejected::IllegalAction(_))));
    assert!(engine.state().stack.is_empty(), "nothing pushed");
}

#[test]
fn reject_when_source_has_summoning_sickness() {
    let p0 = PlayerId(0);
    let source = ObjectId(0);
    let mut state = state_with_source(source, p0, source);
    // Controlled since turn 1, not since the turn began, so the creature source is sick (CR 302.6).
    state.object_mut(source).unwrap().controlled_since_turn = 1;

    let mut engine = Engine::new(state);
    let err = try_activate_and_drain(&mut engine, p0, source, Vec::new()).unwrap_err();
    assert!(matches!(err, ActionError::Rejected(Rejected::IllegalAction(_))));
    assert!(!engine.state().object(source).unwrap().tapped, "no partial mutation");
    assert!(engine.state().stack.is_empty(), "nothing pushed");
}

#[test]
fn reject_ability_kind_not_activated() {
    let p0 = PlayerId(0);
    let source = ObjectId(0);
    let mut state = state_with_source(source, p0, source);
    state.abilities[0].kind = AbilityKind::Triggered;

    let mut engine = Engine::new(state);
    let err = try_activate_and_drain(&mut engine, p0, source, Vec::new()).unwrap_err();
    assert!(matches!(err, ActionError::Rejected(Rejected::IllegalAction(_))));
}

// CR 601.2c
// An unrequested target is rejected and nothing is mutated.
#[test]
fn unrequested_target_is_rejected() {
    let p0 = PlayerId(0);
    let source = ObjectId(0);
    let state = state_with_source(source, p0, source);
    let mut engine = Engine::new(state);

    let err = try_activate_and_drain(&mut engine, p0, source, vec![Target::Player(p0)]).unwrap_err();
    assert!(matches!(err, ActionError::Rejected(Rejected::InvalidTarget)));
    assert!(!engine.state().object(source).unwrap().tapped);
}
