//! CR 700.2e: a triggered ability's modes chosen by a player other than the controller. CR 700.2h does not apply to abilities, since it says "modal spells" and "as the spell is cast".
//! The narrowing matches modal_chosen_by_opponent.rs (targetless, costless modes, exactly one other non-eliminated player), but `push_ordered_triggers` resolves the chooser itself and skips the trigger if a precondition fails, as CR 603.3d removes it from the stack.

use stackfish_engine::engine::{Engine, StepStatus, TriggerModeChoiceError};
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, player_state, two_player_state as base_two_player_state, zones};

fn creature_definition(name: &str) -> CardDefinition {
    CardDefinition {
        definition_id: 1,
        name: name.to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::CREATURE,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: Some(PTValue::Fixed(2)),
        toughness: Some(PTValue::Fixed(2)),
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("bear".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn any_target_requirement() -> TargetSpec {
    TargetSpec::Primitive(TargetRequirement {
        filter: TargetFilter::AnyTarget,
        count: 1,
        up_to: false,
        at_least: false,
        exclude_source: false,
    })
}

fn modal_upkeep_ability_chosen_by_opponent(source: ObjectId, mode_choice: ModeChoice) -> RegisteredAbility {
    RegisteredAbility {
        modes: vec![
            SpellMode { target_requirement: None, effect: Some(AbilityEffect::GainLife(3)), additional_cost: None, pawprint_weight: 0 },
            SpellMode { target_requirement: None, effect: Some(AbilityEffect::Shuffle), additional_cost: None, pawprint_weight: 0 },
        ],
        mode_choice,
        id: AbilityId(0),
        source,
        kind: AbilityKind::Triggered,
        condition: Some(ConditionSpec::Primitive(ConditionPrimitive::BeginningOfYourUpkeep)),
        cost: None,
        target: None,
        operation: ScriptHandle(0),
        effect: None,
        intervening_if: None,
    }
}

fn modal_upkeep_ability_chosen_by_opponent_with_a_target(source: ObjectId) -> RegisteredAbility {
    RegisteredAbility {
        modes: vec![
            SpellMode { target_requirement: None, effect: Some(AbilityEffect::GainLife(3)), additional_cost: None, pawprint_weight: 0 },
            SpellMode {
                target_requirement: Some(any_target_requirement()),
                effect: Some(AbilityEffect::Damage(2)),
                additional_cost: None,
                pawprint_weight: 0,
            },
        ],
        ..modal_upkeep_ability_chosen_by_opponent(source, ModeChoice::ExactlyChosenByOpponent(1))
    }
}

fn two_player_state() -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        phase: Phase::PreGame,
        // The library zone must exist for mode 1's Shuffle effect, even when empty.
        ..base_two_player_state(zones(p0, p1, &[ZoneKind::Hand, ZoneKind::Library, ZoneKind::Battlefield, ZoneKind::Graveyard]))
    }
}

fn pass_both(engine: &mut Engine, p0: PlayerId, p1: PlayerId) -> StepStatus {
    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine);
    engine.submit_action(PlayerAction { player: p1, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine)
}

fn pass_all(engine: &mut Engine, players: &[PlayerId]) -> StepStatus {
    let mut status = StepStatus::Progressed;
    for &player in players {
        engine.submit_action(PlayerAction { player, kind: PlayerActionKind::PassPriority }).unwrap();
        status = drain(engine);
    }
    status
}

#[test]
fn the_trigger_pauses_for_the_other_players_mode_choice_not_the_controllers() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let reaper = ObjectId(0);
    let mut state = two_player_state();
    state.objects.insert(reaper, card_object(reaper, creature_definition("Reaper"), p0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(reaper);
    state.abilities.push(modal_upkeep_ability_chosen_by_opponent(reaper, ModeChoice::ExactlyChosenByOpponent(1)));

    let mut engine = Engine::new(state);
    let status = pass_both(&mut engine, p0, p1);

    let StepStatus::AwaitingTriggerModeChoice { player, min, max, .. } = status else {
        panic!("expected a CR 603.3c mode-choice pause, got {status:?}")
    };
    assert_eq!(player, p1, "CR 700.2e: p0's opponent chooses, not p0 (the trigger's own controller)");
    assert_eq!((min, max), (1, 1));
    assert_eq!(engine.state().player(p0).unwrap().life, 20, "not resolved yet");
}

#[test]
fn the_other_players_chosen_mode_resolves_for_the_triggers_own_controller() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let reaper = ObjectId(0);
    let mut state = two_player_state();
    state.objects.insert(reaper, card_object(reaper, creature_definition("Reaper"), p0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(reaper);
    state.abilities.push(modal_upkeep_ability_chosen_by_opponent(reaper, ModeChoice::ExactlyChosenByOpponent(1)));

    let mut engine = Engine::new(state);
    pass_both(&mut engine, p0, p1);

    // p1 chooses mode 0 (GainLife(3)) though p0 is the controller and benefits.
    engine.submit_trigger_mode_choice(vec![0]).expect("p1 chooses mode 0, within range, no duplicates");
    drain(&mut engine);
    let status = pass_both(&mut engine, p0, p1);

    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0), "got {status:?}");
    assert_eq!(engine.state().player(p0).unwrap().life, 23, "p0 (the trigger's controller) still gets the effect - only the choice moved");
}

#[test]
fn choosing_two_modes_resolves_both_for_the_controller() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let reaper = ObjectId(0);
    let mut state = two_player_state();
    state.objects.insert(reaper, card_object(reaper, creature_definition("Reaper"), p0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(reaper);
    state.abilities.push(modal_upkeep_ability_chosen_by_opponent(reaper, ModeChoice::ExactlyChosenByOpponent(2)));

    let mut engine = Engine::new(state);
    pass_both(&mut engine, p0, p1);

    engine.submit_trigger_mode_choice(vec![1, 0]).expect("order doesn't matter - resolved in card-print order");
    // 2 or more chosen modes always combine into a TargetSpec::Multiple (synthetic zero-count placeholders), so this still chains into a trivially satisfied AwaitingTriggerTargets pause.
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingTriggerTargets { .. }), "got {status:?}");
    engine.submit_trigger_targets(Vec::new()).expect("every combined mode is targetless");
    drain(&mut engine);
    let status = pass_both(&mut engine, p0, p1);

    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0), "got {status:?}");
    assert_eq!(engine.state().player(p0).unwrap().life, 23, "mode 0's GainLife(3) resolved for p0");
    assert!(engine.state().stack.is_empty(), "both modes resolved and the ability left the stack");
}

#[test]
fn a_mode_with_a_target_requirement_makes_the_whole_trigger_never_fire() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let reaper = ObjectId(0);
    let mut state = two_player_state();
    state.objects.insert(reaper, card_object(reaper, creature_definition("Reaper"), p0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(reaper);
    state.abilities.push(modal_upkeep_ability_chosen_by_opponent_with_a_target(reaper));

    let mut engine = Engine::new(state);
    let status = pass_both(&mut engine, p0, p1);

    // No pause: straight to the next priority window, as for a trigger with no legal target (CR 603.3d).
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0), "got {status:?}");
    assert!(engine.state().stack.is_empty(), "a chosen-by-opponent mode with its own target can never validly go on the stack");
    assert_eq!(engine.state().player(p0).unwrap().life, 20, "nothing resolved");
}

#[test]
fn a_three_player_game_makes_the_whole_trigger_never_fire() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let p2 = PlayerId(2);
    let reaper = ObjectId(0);
    let mut state = two_player_state();
    state.players.push(player_state(p2));
    state.turn_order.push(p2);
    state.objects.insert(reaper, card_object(reaper, creature_definition("Reaper"), p0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(reaper);
    state.abilities.push(modal_upkeep_ability_chosen_by_opponent(reaper, ModeChoice::ExactlyChosenByOpponent(1)));

    let mut engine = Engine::new(state);
    let status = pass_all(&mut engine, &[p0, p1, p2]);

    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0), "got {status:?}");
    assert!(engine.state().stack.is_empty(), "CR 700.2e's 3+-player sub-clause isn't supported - never validly chosen, so never pushed");
    assert_eq!(engine.state().player(p0).unwrap().life, 20, "nothing resolved");
}

#[test]
fn submit_trigger_mode_choice_still_rejects_a_duplicate_for_a_chosen_by_opponent_ability() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let reaper = ObjectId(0);
    let mut state = two_player_state();
    state.objects.insert(reaper, card_object(reaper, creature_definition("Reaper"), p0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(reaper);
    state.abilities.push(modal_upkeep_ability_chosen_by_opponent(reaper, ModeChoice::UpToChosenByOpponent(2)));

    let mut engine = Engine::new(state);
    pass_both(&mut engine, p0, p1);

    let err = engine.submit_trigger_mode_choice(vec![0, 0]).unwrap_err();
    assert!(matches!(err, TriggerModeChoiceError::DuplicateMode), "CR 700.2e doesn't grant 700.2d's own repeat exception");
}
