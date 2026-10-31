//! CR 605.3b: a mana ability taps its source and adds mana at once, with no target and no stack object.
//! Non-mana abilities get structural lookup and validation only.

use std::collections::VecDeque;

use stackfish_engine::commands::Rejected;
use stackfish_engine::engine::ActionError;
use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, minimal_card_definition, single_player_state};

fn land_definition() -> CardDefinition {
    CardDefinition {
        oracle_text: "{T}: Add {G}.".to_string(),
        ..minimal_card_definition(
            1,
            "Test Forest",
            "test_forest",
            ManaCost {
                symbols: Vec::new(),
            },
            CardTypeSet::LAND,
        )
    }
}

// Controlled since before turn 1, so never summoning sick unless a test overrides it.
fn permanent_object(id: ObjectId, definition: CardDefinition, owner: PlayerId) -> GameObject {
    card_object(id, definition, owner, ZoneKind::Battlefield)
}

fn state_with_source(source: ObjectId, controller: PlayerId, ability_owner: ObjectId) -> GameState {
    let mut objects = std::collections::HashMap::new();
    objects.insert(
        source,
        permanent_object(source, land_definition(), controller),
    );
    GameState {
        objects,
        abilities: vec![RegisteredAbility {
            modes: Vec::new(),
            mode_choice: ModeChoice::Exactly(1),
            id: AbilityId(0),
            source: ability_owner,
            kind: AbilityKind::Activated,
            condition: None,
            cost: None,
            target: None,
            operation: ScriptHandle(0),
            effect: Some(AbilityEffect::AddMana(vec![ManaUnit::Colored(
                Color::Green,
            )])),
            intervening_if: None,
        }],
        ..single_player_state(
            controller,
            vec![Zone {
                kind: ZoneKind::Battlefield,
                owner: None,
                contents: VecDeque::from([source]),
            }],
        )
    }
}

fn activate(engine: &mut Engine, player: PlayerId, source: ObjectId, targets: Vec<Target>) {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::ActivateAbility {
                source,
                ability_index: 0,
                targets,
                chosen_colors: Vec::new(),
                x_value: None,
            },
        })
        .expect("player holds priority");
}

// Returns the `CommandError` raised mid-pipeline, which `submit_action` does not check.
fn activate_and_drain(
    engine: &mut Engine,
    player: PlayerId,
    source: ObjectId,
    targets: Vec<Target>,
) -> StepStatus {
    activate(engine, player, source, targets);
    drain(engine)
}

fn try_activate(
    engine: &mut Engine,
    player: PlayerId,
    source: ObjectId,
    targets: Vec<Target>,
) -> Result<(), ActionError> {
    engine.submit_action(PlayerAction {
        player,
        kind: PlayerActionKind::ActivateAbility {
            source,
            ability_index: 0,
            targets,
            chosen_colors: Vec::new(),
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

#[test]
fn mana_ability_taps_source_and_adds_mana_no_stack_object() {
    let p0 = PlayerId(0);
    let source = ObjectId(0);
    let state = state_with_source(source, p0, source);
    let mut engine = Engine::new(state);

    let status = activate_and_drain(&mut engine, p0, source, Vec::new());
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));

    assert!(engine.state().object(source).unwrap().tapped, "source taps");
    assert_eq!(
        engine.state().player(p0).unwrap().mana_pool.green,
        1,
        "mana lands in the pool"
    );
    assert!(
        engine.state().stack.is_empty(),
        "a mana ability never touches the stack"
    );
    assert_eq!(
        engine.state().consecutive_passes,
        0,
        "activating breaks the pass chain (CR 117.4)"
    );
    assert_eq!(
        engine.state().priority_player,
        p0,
        "priority stays with the activator (CR 117.3c)"
    );

    let events: Vec<_> = engine.state().ledger.iter().map(|e| &e.kind).collect();
    assert!(matches!(events[0], GameEventKind::Tapped { object } if *object == source));
    assert!(matches!(
        &events[1],
        GameEventKind::ManaAdded { player, units } if *player == p0 && units == &[ManaUnit::Colored(Color::Green)]
    ));
}

#[test]
fn reject_when_actor_does_not_control_source() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let source = ObjectId(0);
    let mut state = state_with_source(source, p0, source);
    // p0 holds priority, so the reject path is changing the source's controller.
    state.object_mut(source).unwrap().controller = p1;

    let mut engine = Engine::new(state);
    let err = try_activate_and_drain(&mut engine, p0, source, Vec::new()).unwrap_err();
    assert!(matches!(
        err,
        ActionError::Rejected(Rejected::IllegalAction(_))
    ));
    assert!(
        !engine.state().object(source).unwrap().tapped,
        "no partial mutation"
    );
}

#[test]
fn reject_when_source_already_tapped() {
    let p0 = PlayerId(0);
    let source = ObjectId(0);
    let mut state = state_with_source(source, p0, source);
    state.object_mut(source).unwrap().tapped = true;

    let mut engine = Engine::new(state);
    let err = try_activate_and_drain(&mut engine, p0, source, Vec::new()).unwrap_err();
    assert!(matches!(
        err,
        ActionError::Rejected(Rejected::IllegalAction(_))
    ));
    assert_eq!(
        engine.state().player(p0).unwrap().mana_pool.green,
        0,
        "no mana added"
    );
}

// CR 602.5a, CR 302.6
// Only creatures are summoning sick, so the fixture land becomes a creature. Resolves the TODO on mana abilities and sickness.
#[test]
fn reject_when_source_has_summoning_sickness() {
    let p0 = PlayerId(0);
    let source = ObjectId(0);
    let mut state = state_with_source(source, p0, source);
    state.object_mut(source).unwrap().definition.card_types = CardTypeSet::CREATURE;
    // Controlled since turn 1 (the current turn), not since the turn began, so it is sick.
    state.object_mut(source).unwrap().controlled_since_turn = 1;

    let mut engine = Engine::new(state);
    let err = try_activate_and_drain(&mut engine, p0, source, Vec::new()).unwrap_err();
    assert!(matches!(
        err,
        ActionError::Rejected(Rejected::IllegalAction(_))
    ));
    assert!(
        !engine.state().object(source).unwrap().tapped,
        "no partial mutation"
    );
    assert_eq!(
        engine.state().player(p0).unwrap().mana_pool.green,
        0,
        "no mana added"
    );
}

#[test]
fn reject_when_actor_does_not_hold_priority() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let source = ObjectId(0);
    let mut state = state_with_source(source, p0, source);
    state.players.push(PlayerState {
        player_id: p1,
        life: 20,
        mana_pool: ManaPool::default(),
        counters: CounterSet::new(),
        emblems: Vec::new(),
        commander_damage_taken: Vec::new(),
        eliminated: false,
        drew_from_empty_library: false,
        mulligans_taken: 0,
        hand_kept: false,
        commander_casts: std::collections::HashMap::new(),
    });
    state.turn_order.push(p1);
    state.priority_player = p1;

    let mut engine = Engine::new(state);
    // p0 does not hold priority, so submit_action rejects it before execute_command (CR 117.3a).
    let err = engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::ActivateAbility {
                source,
                ability_index: 0,
                targets: Vec::new(),
                chosen_colors: Vec::new(),
                x_value: None,
            },
        })
        .unwrap_err();
    assert!(matches!(err, engine::ActionError::NotYourTurn));
}

#[test]
fn reject_nonexistent_ability_index() {
    let p0 = PlayerId(0);
    let source = ObjectId(0);
    let state = state_with_source(source, p0, source);

    let mut engine = Engine::new(state);
    let err = engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::ActivateAbility {
                source,
                ability_index: 1,
                targets: Vec::new(),
                chosen_colors: Vec::new(),
                x_value: None,
            },
        })
        .unwrap_err();
    assert!(matches!(
        err,
        ActionError::Rejected(Rejected::IllegalAction(_))
    ));
}

#[test]
fn reject_ability_kind_not_activated() {
    let p0 = PlayerId(0);
    let source = ObjectId(0);
    let mut state = state_with_source(source, p0, source);
    state.abilities[0].kind = AbilityKind::Triggered;

    let mut engine = Engine::new(state);
    let err = try_activate_and_drain(&mut engine, p0, source, Vec::new()).unwrap_err();
    assert!(matches!(
        err,
        ActionError::Rejected(Rejected::IllegalAction(_))
    ));
}

#[test]
fn reject_target_supplied_for_mana_ability() {
    let p0 = PlayerId(0);
    let source = ObjectId(0);
    let state = state_with_source(source, p0, source);

    let mut engine = Engine::new(state);
    let err =
        try_activate_and_drain(&mut engine, p0, source, vec![Target::Player(p0)]).unwrap_err();
    assert!(matches!(
        err,
        ActionError::Rejected(Rejected::IllegalAction(_))
    ));
    assert!(
        !engine.state().object(source).unwrap().tapped,
        "no partial mutation"
    );
    assert_eq!(engine.state().player(p0).unwrap().mana_pool.green, 0);
}

#[test]
fn multiple_mana_abilities_pick_the_right_one_and_accumulate() {
    let p0 = PlayerId(0);
    let source_a = ObjectId(0);
    let source_b = ObjectId(1);
    let mut state = state_with_source(source_a, p0, source_a);
    state
        .objects
        .insert(source_b, permanent_object(source_b, land_definition(), p0));
    state
        .zone_mut(ZoneKind::Battlefield, None)
        .unwrap()
        .push_bottom(source_b);
    // source_a gets a second ability (index 1) that adds colorless.
    state.abilities.push(RegisteredAbility {
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
        id: AbilityId(1),
        source: source_a,
        kind: AbilityKind::Activated,
        condition: None,
        cost: None,
        target: None,
        operation: ScriptHandle(0),
        effect: Some(AbilityEffect::AddMana(vec![ManaUnit::Colorless])),
        intervening_if: None,
    });
    state.abilities.push(RegisteredAbility {
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
        id: AbilityId(2),
        source: source_b,
        kind: AbilityKind::Activated,
        condition: None,
        cost: None,
        target: None,
        operation: ScriptHandle(0),
        effect: Some(AbilityEffect::AddMana(vec![ManaUnit::Colored(
            Color::Green,
        )])),
        intervening_if: None,
    });

    let mut engine = Engine::new(state);

    // Index 1 (colorless), not index 0 (green).
    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::ActivateAbility {
                source: source_a,
                ability_index: 1,
                targets: Vec::new(),
                chosen_colors: Vec::new(),
                x_value: None,
            },
        })
        .unwrap();
    drain(&mut engine);
    assert_eq!(engine.state().player(p0).unwrap().mana_pool.colorless, 1);
    assert_eq!(engine.state().player(p0).unwrap().mana_pool.green, 0);
    assert!(engine.state().object(source_a).unwrap().tapped);
    assert!(!engine.state().object(source_b).unwrap().tapped);

    // source_b's only ability (green) accumulates on top.
    activate(&mut engine, p0, source_b, Vec::new());
    drain(&mut engine);
    assert_eq!(engine.state().player(p0).unwrap().mana_pool.colorless, 1);
    assert_eq!(engine.state().player(p0).unwrap().mana_pool.green, 1);
    assert!(engine.state().object(source_b).unwrap().tapped);
}
