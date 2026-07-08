//! Phase 5 vertical slice: baseline `ActivateAbility` (CR 602), implemented
//! far enough to support one concrete case end-to-end: a mana ability
//! (CR 605) that taps its source and adds mana to the pool. CR 605.3b: a
//! mana ability has no target and never uses the stack - it resolves
//! immediately as part of activation. Non-mana abilities get the structural
//! lookup/validation only - effect resolution stays `todo!()` (no script
//! bridge yet, same gap every other ability/operation hits).

use std::collections::{HashMap, VecDeque};

use stackfish_engine::commands::CommandError as EngineCommandError;
use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::*;

fn land_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 1,
        name: "Test Forest".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::LAND,
        subtypes: Vec::new(),
        oracle_text: "{T}: Add {G}.".to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_forest".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
    }
}

fn permanent_object(id: ObjectId, definition: CardDefinition, owner: PlayerId) -> GameObject {
    GameObject {
        object_id: id,
        lineage: ObjectLineage { previous: None },
        definition,
        owner,
        controller: owner,
        zone: ZoneKind::Battlefield,
        tapped: false,
        flipped: false,
        face_down: false,
        phased_out: false,
        damage_marked: 0,
        deathtouched: false,
        counters: CounterSet::new(),
        attachments: Vec::new(),
        attached_to: None,
        timestamp: SequenceNo(0),
        // Controlled well before turn 1 started - state_with_source seeds
        // last_active_turn_number to 1, so this is never summoning sick by
        // default. Tests exercising sickness itself override this field
        // directly on the object after construction.
        controlled_since_turn: 0,
        visibility: VisibilityPolicy { public: true, visible_to: Vec::new() },
    }
}

/// One player, main phase, empty stack, a battlefield zone with a single
/// tap-for-mana permanent registered as its only ability.
fn state_with_source(source: ObjectId, controller: PlayerId, ability_owner: ObjectId) -> GameState {
    GameState {
        players: vec![PlayerState {
            player_id: controller,
            life: 20,
            mana_pool: ManaPool { white: 0, blue: 0, black: 0, red: 0, green: 0, colorless: 0 },
            counters: CounterSet::new(),
            emblems: Vec::new(),
            commander_damage_taken: Vec::new(),
            eliminated: false,
            mulligans_taken: 0,
            hand_kept: false,
        }],
        turn_order: vec![controller],
        active_player: controller,
        turn_number: 1,
        phase: Phase::Main,
        step: Step::PreCombatMain,
        priority_player: controller,
        consecutive_passes: 0,
        lands_played_this_turn: HashMap::new(),
        combat: None,
        last_active_turn_number: { let mut m = HashMap::new(); m.insert(controller, 1); m },
        objects: {
            let mut objects = HashMap::new();
            objects.insert(source, permanent_object(source, land_definition(), controller));
            objects
        },
        zones: vec![Zone {
            kind: ZoneKind::Battlefield,
            owner: None,
            contents: VecDeque::from([source]),
        }],
        stack: Vec::new(),
        continuous_effects: Vec::new(),
        replacement_effects: Vec::new(),
        abilities: vec![RegisteredAbility {
            source: ability_owner,
            kind: AbilityKind::Activated,
            condition: None,
            cost: None,
            target: None,
            operation: ScriptHandle(0),
            effect: Some(AbilityEffect::AddMana(vec![ManaUnit::Colored(Color::Green)])),
        }],
        ledger: Vec::new(),
        rng: RngState { seed: 1, draws_consumed: 0 },
        next_event_id: 0,
        next_sequence_no: 0,
        next_object_id: 1000,
        pending_step_skips: Vec::new(),
    }
}

fn drain(engine: &mut Engine) -> StepStatus {
    loop {
        let result = engine.step();
        match result.status {
            StepStatus::Progressed => continue,
            other => return other,
        }
    }
}

fn activate(engine: &mut Engine, player: PlayerId, source: ObjectId, targets: Vec<Target>) {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::ActivateAbility { source, ability_index: 0, targets },
        })
        .expect("player holds priority");
}

/// Submits the action and drains, returning the engine's `CommandError` if
/// `execute_command` rejected it mid-pipeline (the validations this slice
/// adds all surface as `StepStatus::Error`, not a `submit_action` rejection,
/// since they need state `submit_action` doesn't check).
fn activate_and_drain(
    engine: &mut Engine,
    player: PlayerId,
    source: ObjectId,
    targets: Vec<Target>,
) -> StepStatus {
    activate(engine, player, source, targets);
    drain(engine)
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
    assert_eq!(engine.state().player(p0).unwrap().mana_pool.green, 1, "mana lands in the pool");
    assert!(engine.state().stack.is_empty(), "a mana ability never touches the stack");
    assert_eq!(engine.state().consecutive_passes, 0, "activating breaks the pass chain (CR 117.4)");
    assert_eq!(engine.state().priority_player, p0, "priority stays with the activator (CR 117.3c)");

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
    // p1 doesn't control anything, but has priority via a (fake) second seat
    // isn't needed - simplest reject path is to have p0 (who holds priority)
    // not control the source, by changing the source's controller.
    state.object_mut(source).unwrap().controller = p1;

    let mut engine = Engine::new(state);
    let status = activate_and_drain(&mut engine, p0, source, Vec::new());
    assert!(matches!(status, StepStatus::Error(EngineCommandError::IllegalAction(_))));
    assert!(!engine.state().object(source).unwrap().tapped, "no partial mutation");
}

#[test]
fn reject_when_source_already_tapped() {
    let p0 = PlayerId(0);
    let source = ObjectId(0);
    let mut state = state_with_source(source, p0, source);
    state.object_mut(source).unwrap().tapped = true;

    let mut engine = Engine::new(state);
    let status = activate_and_drain(&mut engine, p0, source, Vec::new());
    assert!(matches!(status, StepStatus::Error(EngineCommandError::IllegalAction(_))));
    assert_eq!(engine.state().player(p0).unwrap().mana_pool.green, 0, "no mana added");
}

/// CR 602.5a: closes the TODO the activate-ability-mana slice left open -
/// a creature that entered the battlefield this turn can't tap for mana
/// either (same summoning-sickness check `DeclareAttackers` uses). CR
/// 302.6/the keyword-primitives slice: summoning sickness only restricts a
/// *creature's* tap abilities, not a land's - `state_with_source`'s default
/// fixture is a land, so this test overrides it to a creature to actually
/// exercise the check it's testing.
#[test]
fn reject_when_source_has_summoning_sickness() {
    let p0 = PlayerId(0);
    let source = ObjectId(0);
    let mut state = state_with_source(source, p0, source);
    state.object_mut(source).unwrap().definition.card_types = CardTypeSet::CREATURE;
    // last_active_turn_number[p0] == 1 (state_with_source's default) - a
    // source controlled since turn 1 (the current turn) hasn't been
    // controlled since the turn *began*, so it's sick.
    state.object_mut(source).unwrap().controlled_since_turn = 1;

    let mut engine = Engine::new(state);
    let status = activate_and_drain(&mut engine, p0, source, Vec::new());
    assert!(matches!(status, StepStatus::Error(EngineCommandError::IllegalAction(_))));
    assert!(!engine.state().object(source).unwrap().tapped, "no partial mutation");
    assert_eq!(engine.state().player(p0).unwrap().mana_pool.green, 0, "no mana added");
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
        mana_pool: ManaPool { white: 0, blue: 0, black: 0, red: 0, green: 0, colorless: 0 },
        counters: CounterSet::new(),
        emblems: Vec::new(),
        commander_damage_taken: Vec::new(),
        eliminated: false,
        mulligans_taken: 0,
        hand_kept: false,
    });
    state.turn_order.push(p1);
    state.priority_player = p1;

    let mut engine = Engine::new(state);
    // p0 doesn't hold priority - submit_action itself rejects it (CR
    // 117.3a's check lives there), never even reaching execute_command.
    let err = engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::ActivateAbility { source, ability_index: 0, targets: Vec::new() },
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
    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::ActivateAbility { source, ability_index: 1, targets: Vec::new() },
        })
        .unwrap();
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::Error(EngineCommandError::IllegalAction(_))));
}

#[test]
fn reject_ability_kind_not_activated() {
    let p0 = PlayerId(0);
    let source = ObjectId(0);
    let mut state = state_with_source(source, p0, source);
    state.abilities[0].kind = AbilityKind::Triggered;

    let mut engine = Engine::new(state);
    let status = activate_and_drain(&mut engine, p0, source, Vec::new());
    assert!(matches!(status, StepStatus::Error(EngineCommandError::IllegalAction(_))));
}

#[test]
fn reject_target_supplied_for_mana_ability() {
    let p0 = PlayerId(0);
    let source = ObjectId(0);
    let state = state_with_source(source, p0, source);

    let mut engine = Engine::new(state);
    let status = activate_and_drain(&mut engine, p0, source, vec![Target::Player(p0)]);
    assert!(matches!(status, StepStatus::Error(EngineCommandError::IllegalAction(_))));
    assert!(!engine.state().object(source).unwrap().tapped, "no partial mutation");
    assert_eq!(engine.state().player(p0).unwrap().mana_pool.green, 0);
}

/// Two mana abilities on the same source, plus a second source - confirms
/// `ability_index`/`source` lookup picks the right one and the pool
/// accumulates correctly across separate activations.
#[test]
fn multiple_mana_abilities_pick_the_right_one_and_accumulate() {
    let p0 = PlayerId(0);
    let source_a = ObjectId(0);
    let source_b = ObjectId(1);
    let mut state = state_with_source(source_a, p0, source_a);
    state.objects.insert(source_b, permanent_object(source_b, land_definition(), p0));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(source_b);
    // source_a gets a second ability (index 1: adds colorless instead).
    state.abilities.push(RegisteredAbility {
        source: source_a,
        kind: AbilityKind::Activated,
        condition: None,
        cost: None,
        target: None,
        operation: ScriptHandle(0),
        effect: Some(AbilityEffect::AddMana(vec![ManaUnit::Colorless])),
    });
    state.abilities.push(RegisteredAbility {
        source: source_b,
        kind: AbilityKind::Activated,
        condition: None,
        cost: None,
        target: None,
        operation: ScriptHandle(0),
        effect: Some(AbilityEffect::AddMana(vec![ManaUnit::Colored(Color::Green)])),
    });

    let mut engine = Engine::new(state);

    // source_a's ability_index 1 (colorless), not index 0 (green).
    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::ActivateAbility { source: source_a, ability_index: 1, targets: Vec::new() },
        })
        .unwrap();
    drain(&mut engine);
    assert_eq!(engine.state().player(p0).unwrap().mana_pool.colorless, 1);
    assert_eq!(engine.state().player(p0).unwrap().mana_pool.green, 0);
    assert!(engine.state().object(source_a).unwrap().tapped);
    assert!(!engine.state().object(source_b).unwrap().tapped);

    // source_b's only ability (index 0, green) - accumulates on top.
    activate(&mut engine, p0, source_b, Vec::new());
    drain(&mut engine);
    assert_eq!(engine.state().player(p0).unwrap().mana_pool.colorless, 1);
    assert_eq!(engine.state().player(p0).unwrap().mana_pool.green, 1);
    assert!(engine.state().object(source_b).unwrap().tapped);
}
