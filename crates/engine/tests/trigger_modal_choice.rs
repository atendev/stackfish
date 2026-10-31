//! CR 603.3c: a modal triggered ability's controller announces the mode choice when it goes on the stack, before the target choice (CR 603.3d). `RegisteredAbility` carries `modes` and `mode_choice`, and `push_ordered_triggers` pauses on `AwaitingTriggerModeChoice`.
//! `submit_trigger_mode_choice` combines modes via `combine_chosen_modes` (shared with `CastSpell`) and chains into `AwaitingTriggerTargets` if the combined modes need a target, or pushes at once.

use stackfish_engine::engine::{Engine, StepStatus, TriggerModeChoiceError};
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, two_player_state as base_two_player_state, zones};

fn creature_definition(name: &str) -> CardDefinition {
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

fn two_player_state() -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        phase: Phase::PreGame,
        ..base_two_player_state(zones(
            p0,
            p1,
            &[
                ZoneKind::Hand,
                ZoneKind::Library,
                ZoneKind::Battlefield,
                ZoneKind::Graveyard,
            ],
        ))
    }
}

fn pass_both(engine: &mut Engine, p0: PlayerId, p1: PlayerId) -> StepStatus {
    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::PassPriority,
        })
        .unwrap();
    drain(engine);
    engine
        .submit_action(PlayerAction {
            player: p1,
            kind: PlayerActionKind::PassPriority,
        })
        .unwrap();
    drain(engine)
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

fn creature_target_requirement(exclude_source: bool) -> TargetSpec {
    TargetSpec::Primitive(TargetRequirement {
        filter: TargetFilter::Object {
            mana_value: None,
            card_types: Some(CardTypeSet::CREATURE),
            excluded_card_types: None,
            subtypes: None,
            excluded_subtypes: None,
            supertypes: None,
            excluded_supertypes: None,
            controller: None,
            zone: None,
        },
        count: 1,
        up_to: false,
        at_least: false,
        exclude_source,
    })
}

// Mode 2 (Destroy) is unsupported inside AbilityEffect::Modal, so it is used only by single-mode tests and never combined.
fn modal_upkeep_ability(
    source: ObjectId,
    mode_choice: ModeChoice,
    destroy_excludes_source: bool,
) -> RegisteredAbility {
    RegisteredAbility {
        modes: vec![
            SpellMode {
                target_requirement: None,
                effect: Some(AbilityEffect::GainLife(3)),
                additional_cost: None,
                pawprint_weight: 0,
            },
            SpellMode {
                target_requirement: Some(any_target_requirement()),
                effect: Some(AbilityEffect::Damage(2)),
                additional_cost: None,
                pawprint_weight: 0,
            },
            SpellMode {
                target_requirement: Some(creature_target_requirement(destroy_excludes_source)),
                effect: Some(AbilityEffect::Destroy),
                additional_cost: None,
                pawprint_weight: 0,
            },
        ],
        mode_choice,
        id: AbilityId(0),
        source,
        kind: AbilityKind::Triggered,
        condition: Some(ConditionSpec::Primitive(
            ConditionPrimitive::BeginningOfYourUpkeep,
        )),
        cost: None,
        target: None,
        operation: ScriptHandle(0),
        effect: None,
        intervening_if: None,
    }
}

#[test]
fn single_mode_chosen_resolves_only_that_effect() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let reaper = ObjectId(0);
    let mut state = two_player_state();
    state.objects.insert(
        reaper,
        card_object(
            reaper,
            creature_definition("Reaper"),
            p0,
            ZoneKind::Battlefield,
        ),
    );
    state
        .zone_mut(ZoneKind::Battlefield, None)
        .unwrap()
        .push_bottom(reaper);
    state
        .abilities
        .push(modal_upkeep_ability(reaper, ModeChoice::Exactly(1), false));

    let mut engine = Engine::new(state);
    let status = pass_both(&mut engine, p0, p1);
    let StepStatus::AwaitingTriggerModeChoice {
        player,
        modes,
        min,
        max,
        ..
    } = status
    else {
        panic!("expected a CR 603.3c mode-choice pause, got {status:?}")
    };
    assert_eq!(player, p0, "the trigger's own controller chooses");
    assert_eq!(modes.len(), 3);
    assert_eq!((min, max), (1, 1));
    assert_eq!(
        engine.state().stack.len(),
        0,
        "not pushed yet - still waiting on the choice"
    );

    engine
        .submit_trigger_mode_choice(vec![0])
        .expect("mode 0 is in range");
    let status = drain(&mut engine);
    // Mode 0 is targetless, so it is pushed at once with no AwaitingTriggerTargets detour.
    assert!(
        matches!(status, StepStatus::AwaitingAction { player } if player == p0),
        "got {status:?}"
    );
    assert_eq!(engine.state().stack.len(), 1);

    let status = pass_both(&mut engine, p0, p1);
    assert!(
        matches!(status, StepStatus::AwaitingAction { player } if player == p0),
        "got {status:?}"
    );
    assert_eq!(
        engine.state().player(p0).unwrap().life,
        23,
        "only mode 0's GainLife(3) resolved"
    );
}

#[test]
fn two_modes_chosen_combine_and_resolve_both_against_their_own_targets() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let reaper = ObjectId(0);
    let mut state = two_player_state();
    state.objects.insert(
        reaper,
        card_object(
            reaper,
            creature_definition("Reaper"),
            p0,
            ZoneKind::Battlefield,
        ),
    );
    state
        .zone_mut(ZoneKind::Battlefield, None)
        .unwrap()
        .push_bottom(reaper);
    state
        .abilities
        .push(modal_upkeep_ability(reaper, ModeChoice::UpTo(2), false));

    let mut engine = Engine::new(state);
    let status = pass_both(&mut engine, p0, p1);
    assert!(
        matches!(status, StepStatus::AwaitingTriggerModeChoice { .. }),
        "got {status:?}"
    );

    engine
        .submit_trigger_mode_choice(vec![0, 1])
        .expect("modes 0 and 1 are both in range");
    let status = drain(&mut engine);
    // Mode 1 needs a target, so it chains into the CR 603.3d pause with one combined TargetSpec::Multiple (a synthetic zero-count slot for mode 0).
    let StepStatus::AwaitingTriggerTargets {
        player,
        spec,
        candidates,
    } = status
    else {
        panic!(
            "expected the combined mode choice to chain into a CR 603.3d target pause, got {status:?}"
        )
    };
    assert_eq!(player, p0);
    assert!(matches!(spec, TargetSpec::Multiple(ref reqs) if reqs.len() == 2));
    assert_eq!(
        candidates.len(),
        2,
        "one candidate list per combined mode slot"
    );

    engine
        .submit_trigger_targets(vec![Target::Player(p1)])
        .expect("mode 1's own target");
    drain(&mut engine);
    let status = pass_both(&mut engine, p0, p1);
    assert!(
        matches!(status, StepStatus::AwaitingAction { player } if player == p0),
        "got {status:?}"
    );

    assert_eq!(
        engine.state().player(p0).unwrap().life,
        23,
        "mode 0's GainLife(3) resolved"
    );
    assert_eq!(
        engine.state().player(p1).unwrap().life,
        18,
        "mode 1's Damage(2) resolved against its own target"
    );
}

#[test]
fn a_single_chosen_mode_with_its_own_target_chains_into_real_target_selection() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let reaper = ObjectId(0);
    let bear = ObjectId(1);
    let mut state = two_player_state();
    state.objects.insert(
        reaper,
        card_object(
            reaper,
            creature_definition("Reaper"),
            p0,
            ZoneKind::Battlefield,
        ),
    );
    state
        .zone_mut(ZoneKind::Battlefield, None)
        .unwrap()
        .push_bottom(reaper);
    state.objects.insert(
        bear,
        card_object(bear, creature_definition("Bear"), p1, ZoneKind::Battlefield),
    );
    state
        .zone_mut(ZoneKind::Battlefield, None)
        .unwrap()
        .push_bottom(bear);
    state
        .abilities
        .push(modal_upkeep_ability(reaper, ModeChoice::Exactly(1), false));

    let mut engine = Engine::new(state);
    pass_both(&mut engine, p0, p1);
    // A single chosen mode (Destroy target creature) passes its effect and target_requirement through unwrapped from Modal.
    engine
        .submit_trigger_mode_choice(vec![2])
        .expect("mode 2 is in range");
    let status = drain(&mut engine);
    let StepStatus::AwaitingTriggerTargets {
        player, candidates, ..
    } = status
    else {
        panic!(
            "expected the chosen mode's own target requirement to raise a real CR 603.3d pause, got {status:?}"
        )
    };
    assert_eq!(player, p0);
    let mut sorted = candidates[0].clone();
    sorted.sort_by_key(|t| match t {
        Target::Object(id) => id.0,
        Target::Player(_) => u64::MAX,
        Target::Ability(id) => id.0,
    });
    assert_eq!(sorted, vec![Target::Object(reaper), Target::Object(bear)]);

    engine
        .submit_trigger_targets(vec![Target::Object(bear)])
        .expect("Bear was offered");
    drain(&mut engine);
    let status = pass_both(&mut engine, p0, p1);
    assert!(
        matches!(status, StepStatus::AwaitingAction { player } if player == p0),
        "got {status:?}"
    );
    assert!(
        engine.state().object(bear).is_none()
            || engine.state().object(bear).unwrap().zone != ZoneKind::Battlefield,
        "Bear was destroyed by the chosen mode's own real target"
    );
}

#[test]
fn a_chosen_mode_with_no_legal_target_removes_the_whole_ability_from_the_stack() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let reaper = ObjectId(0);
    let mut state = two_player_state();
    state.objects.insert(
        reaper,
        card_object(
            reaper,
            creature_definition("Reaper"),
            p0,
            ZoneKind::Battlefield,
        ),
    );
    state
        .zone_mut(ZoneKind::Battlefield, None)
        .unwrap()
        .push_bottom(reaper);
    // Mode 2's Destroy target excludes the source, and Reaper is the only creature, so there are zero candidates.
    state
        .abilities
        .push(modal_upkeep_ability(reaper, ModeChoice::Exactly(1), true));

    let mut engine = Engine::new(state);
    pass_both(&mut engine, p0, p1);
    engine
        .submit_trigger_mode_choice(vec![2])
        .expect("mode 2 is in range");
    let status = drain(&mut engine);

    // CR 603.3d: with no legal choice the ability is removed from the stack, never pushed, back to a fresh priority window.
    assert!(
        matches!(status, StepStatus::AwaitingAction { player } if player == p0),
        "got {status:?}"
    );
    assert!(
        engine.state().stack.is_empty(),
        "removed from the stack, not pushed"
    );
    assert!(
        engine.state().object(reaper).is_some(),
        "Reaper itself untouched"
    );
}

#[test]
fn submit_trigger_mode_choice_rejects_when_not_awaiting() {
    let mut engine = Engine::new(two_player_state());
    let err = engine.submit_trigger_mode_choice(vec![0]).unwrap_err();
    assert!(matches!(err, TriggerModeChoiceError::NotAwaitingChoice));
}

#[test]
fn submit_trigger_mode_choice_rejects_a_count_outside_the_announced_range() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let reaper = ObjectId(0);
    let mut state = two_player_state();
    state.objects.insert(
        reaper,
        card_object(
            reaper,
            creature_definition("Reaper"),
            p0,
            ZoneKind::Battlefield,
        ),
    );
    state
        .zone_mut(ZoneKind::Battlefield, None)
        .unwrap()
        .push_bottom(reaper);
    state
        .abilities
        .push(modal_upkeep_ability(reaper, ModeChoice::Exactly(1), false));

    let mut engine = Engine::new(state);
    pass_both(&mut engine, p0, p1);
    let err = engine.submit_trigger_mode_choice(vec![0, 1]).unwrap_err();
    assert!(matches!(err, TriggerModeChoiceError::InvalidCount));
    // Still paused - a rejected answer doesn't consume the choice.
    assert_eq!(engine.state().stack.len(), 0);
}

#[test]
fn submit_trigger_mode_choice_rejects_a_duplicate_mode_when_not_repeatable() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let reaper = ObjectId(0);
    let mut state = two_player_state();
    state.objects.insert(
        reaper,
        card_object(
            reaper,
            creature_definition("Reaper"),
            p0,
            ZoneKind::Battlefield,
        ),
    );
    state
        .zone_mut(ZoneKind::Battlefield, None)
        .unwrap()
        .push_bottom(reaper);
    state
        .abilities
        .push(modal_upkeep_ability(reaper, ModeChoice::UpTo(2), false));

    let mut engine = Engine::new(state);
    pass_both(&mut engine, p0, p1);
    let err = engine.submit_trigger_mode_choice(vec![0, 0]).unwrap_err();
    assert!(matches!(err, TriggerModeChoiceError::DuplicateMode));
}

#[test]
fn submit_trigger_mode_choice_rejects_an_out_of_range_index() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let reaper = ObjectId(0);
    let mut state = two_player_state();
    state.objects.insert(
        reaper,
        card_object(
            reaper,
            creature_definition("Reaper"),
            p0,
            ZoneKind::Battlefield,
        ),
    );
    state
        .zone_mut(ZoneKind::Battlefield, None)
        .unwrap()
        .push_bottom(reaper);
    state
        .abilities
        .push(modal_upkeep_ability(reaper, ModeChoice::Exactly(1), false));

    let mut engine = Engine::new(state);
    pass_both(&mut engine, p0, p1);
    let err = engine.submit_trigger_mode_choice(vec![99]).unwrap_err();
    assert!(matches!(err, TriggerModeChoiceError::NoSuchMode));
}
