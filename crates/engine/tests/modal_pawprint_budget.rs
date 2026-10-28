//! CR 700.2i: pawprint ({P}) modes are chosen by total weight against a budget and not by count, validated by an explicit weight-sum check in `CastSpell` and `submit_trigger_mode_choice` (CR 603.3c). Choosing zero modes is legal.
// TODO: repeatable pawprint-budget modes and the CR 700.2e opponent-chosen combination

use std::collections::HashMap;

use stackfish_engine::engine::ActionError;
use stackfish_engine::commands::Rejected;
use stackfish_engine::engine::{Engine, StepStatus, TriggerModeChoiceError};
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, hand_battlefield_graveyard_zones, two_player_state as base_two_player_state};

fn any_target_requirement() -> TargetSpec {
    TargetSpec::Primitive(TargetRequirement {
        filter: TargetFilter::AnyTarget,
        count: 1,
        up_to: false,
        at_least: false,
        exclude_source: false,
    })
}

fn pawprint_budget_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 41,
        name: "Test Pawprint Budget".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::INSTANT,
        subtypes: Vec::new(),
        oracle_text: "Choose up to 3 {P} worth - \
            {P} You gain 1 life. \
            {P}{P} Test Pawprint Budget deals 2 damage to any target."
            .to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_pawprint_budget".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: vec![
            SpellMode { target_requirement: None, effect: Some(AbilityEffect::GainLife(1)), additional_cost: None, pawprint_weight: 1 },
            SpellMode {
                target_requirement: Some(any_target_requirement()),
                effect: Some(AbilityEffect::Damage(2)),
                additional_cost: None,
                pawprint_weight: 2,
            },
        ],
        mode_choice: ModeChoice::PawprintBudget(3),
    }
}

fn two_player_state() -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        step: Step::PreCombatMain,
        last_active_turn_number: HashMap::from([(p0, 1), (p1, 1)]),
        ..base_two_player_state(hand_battlefield_graveyard_zones(p0, p1))
    }
}

fn cast_spell(engine: &mut Engine, player: PlayerId, card: ObjectId, modes: Vec<u32>, targets: Vec<Target>) {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::CastSpell { card, modes, targets, x_value: None, alternative_cost: None , pay_buyback: false},
        })
        .expect("player holds priority, spell is in hand");
}

fn try_cast_spell(engine: &mut Engine, player: PlayerId, card: ObjectId, modes: Vec<u32>, targets: Vec<Target>) -> Result<(), ActionError> {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::CastSpell { card, modes, targets, x_value: None, alternative_cost: None , pay_buyback: false},
        })
        
}

fn pay_free_and_drain(engine: &mut Engine) -> StepStatus {
    let status = drain(engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }), "expected a pause for the cast's (free) cost, got {status:?}");
    engine.submit_payment(PaymentPlan { payments: Vec::new() }).expect("empty plan pays a {0} cost");
    drain(engine)
}

fn pass_both_and_drain(engine: &mut Engine, p0: PlayerId, p1: PlayerId) -> StepStatus {
    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine);
    engine.submit_action(PlayerAction { player: p1, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine)
}

#[test]
fn choosing_both_modes_exactly_fills_the_budget() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, pawprint_budget_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    // Mode 0 (weight 1) + mode 1 (weight 2) = 3, exactly the budget.
    cast_spell(&mut engine, p0, card, vec![0, 1], vec![Target::Player(p1)]);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    assert_eq!(engine.state().player(p0).unwrap().life, 21, "mode 0's GainLife(1) resolved");
    assert_eq!(engine.state().player(p1).unwrap().life, 18, "mode 1's Damage(2) resolved");
}

#[test]
fn choosing_only_the_cheap_mode_is_legal_with_budget_to_spare() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, pawprint_budget_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![0], Vec::new());
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    assert_eq!(engine.state().player(p0).unwrap().life, 21, "mode 0 (weight 1, budget 3) resolved on its own");
}

#[test]
fn choosing_zero_modes_is_legal_cr_700_2i_sets_no_floor() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, pawprint_budget_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, Vec::new(), Vec::new());
    let status = pay_free_and_drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0), "the spell still resolves, doing nothing, got {status:?}");
    assert_eq!(engine.state().player(p0).unwrap().life, 20, "no mode chosen, no effect");
}

#[test]
fn choosing_a_combination_over_budget_is_rejected() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    // A third weight-2 mode makes 2+2+1 = 5 > 3 possible to attempt.
    let mut definition = pawprint_budget_definition();
    definition.modes.push(SpellMode {
        target_requirement: Some(any_target_requirement()),
        effect: Some(AbilityEffect::GainLife(4)),
        additional_cost: None,
        pawprint_weight: 2,
    });
    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, definition, p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    // Modes 1 and 2 together cost 2 + 2 = 4 > budget of 3.
    let err = try_cast_spell(&mut engine, p0, card, vec![1, 2], vec![Target::Player(p1), Target::Player(p0)]).unwrap_err();

    assert!(matches!(err, ActionError::Rejected(Rejected::IllegalAction(_))));
    assert_eq!(engine.state().object(card).unwrap().zone, ZoneKind::Hand, "state untouched");
}

#[test]
fn choosing_the_same_pawprint_mode_twice_is_rejected() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, pawprint_budget_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    // Weight 1 + 1 = 2 is within budget, but this variant does not support repeats.
    let err = try_cast_spell(&mut engine, p0, card, vec![0, 0], Vec::new()).unwrap_err();

    assert!(matches!(err, ActionError::Rejected(Rejected::IllegalAction(_))));
    assert_eq!(engine.state().object(card).unwrap().zone, ZoneKind::Hand, "state untouched");
}

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

fn trigger_two_player_state() -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        phase: Phase::PreGame,
        ..base_two_player_state(stackfish_testutil::zones(p0, p1, &[ZoneKind::Hand, ZoneKind::Library, ZoneKind::Battlefield, ZoneKind::Graveyard]))
    }
}

fn pass_both(engine: &mut Engine, p0: PlayerId, p1: PlayerId) -> StepStatus {
    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine);
    engine.submit_action(PlayerAction { player: p1, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine)
}

fn pawprint_budget_upkeep_ability(source: ObjectId, extra_mode: Option<SpellMode>) -> RegisteredAbility {
    let mut modes = vec![
        SpellMode { target_requirement: None, effect: Some(AbilityEffect::GainLife(1)), additional_cost: None, pawprint_weight: 1 },
        SpellMode { target_requirement: None, effect: Some(AbilityEffect::GainLife(5)), additional_cost: None, pawprint_weight: 2 },
    ];
    if let Some(mode) = extra_mode {
        modes.push(mode);
    }
    RegisteredAbility {
        modes,
        mode_choice: ModeChoice::PawprintBudget(3),
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

// CR 603.3c
#[test]
fn a_pawprint_budgeted_trigger_accepts_a_combination_within_budget() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let reaper = ObjectId(0);
    let mut state = trigger_two_player_state();
    state.objects.insert(reaper, card_object(reaper, creature_definition("Reaper"), p0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(reaper);
    state.abilities.push(pawprint_budget_upkeep_ability(reaper, None));

    let mut engine = Engine::new(state);
    let status = pass_both(&mut engine, p0, p1);
    let StepStatus::AwaitingTriggerModeChoice { player, min, max, .. } = status else {
        panic!("expected a CR 603.3c mode-choice pause, got {status:?}")
    };
    assert_eq!(player, p0);
    assert_eq!((min, max), (0, 3), "CR 700.2i: no floor, budget as the display hint");

    engine.submit_trigger_mode_choice(vec![0, 1]).expect("weight 1 + 2 = 3, exactly the budget");
    let status = drain(&mut engine);
    // Both modes are targetless, but combine_chosen_modes still produces a Multiple of two zero-count slots for any 2+ mode combination.
    assert!(matches!(status, StepStatus::AwaitingTriggerTargets { .. }), "got {status:?}");
    engine.submit_trigger_targets(vec![]).expect("both requirements are zero-count");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0), "resolution finishes cleanly, got {status:?}");
    let status = pass_both(&mut engine, p0, p1);
    assert!(matches!(status, StepStatus::AwaitingAction { .. }), "got {status:?}");
    assert_eq!(engine.state().player(p0).unwrap().life, 26, "both modes' GainLife resolved (1 + 5)");
}

#[test]
fn a_pawprint_budgeted_trigger_rejects_a_combination_over_budget() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let reaper = ObjectId(0);
    let mut state = trigger_two_player_state();
    state.objects.insert(reaper, card_object(reaper, creature_definition("Reaper"), p0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(reaper);
    let extra = SpellMode { target_requirement: None, effect: Some(AbilityEffect::GainLife(9)), additional_cost: None, pawprint_weight: 2 };
    state.abilities.push(pawprint_budget_upkeep_ability(reaper, Some(extra)));

    let mut engine = Engine::new(state);
    pass_both(&mut engine, p0, p1);

    // Modes 1 and 2 together cost 2 + 2 = 4 > budget of 3.
    let err = engine.submit_trigger_mode_choice(vec![1, 2]).unwrap_err();
    assert!(matches!(err, TriggerModeChoiceError::InvalidCount));
}
