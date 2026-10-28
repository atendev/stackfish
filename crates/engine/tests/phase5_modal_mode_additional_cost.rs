//! CR 700.2h: a modal spell mode with its own additional cost, `SpellMode.additional_cost`. The `CastSpell` arm appends every chosen mode's cost to `CostSpec.additional`, so `PaymentPlan::validate` and `submit_payment` need no change.
//! `AdditionalCost::LifePayment` is the exercised shape.

use std::collections::HashMap;

use stackfish_engine::engine::{Engine, PaymentError, StepStatus};
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

fn mode_cost_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 40,
        name: "Test Mode Cost".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::INSTANT,
        subtypes: Vec::new(),
        oracle_text: "Choose one or both - \
            * You gain 3 life. \
            * Pay 2 life. Test Mode Cost deals 2 damage to any target."
            .to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_mode_cost".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: vec![
            SpellMode { target_requirement: None, effect: Some(AbilityEffect::GainLife(3)), additional_cost: None, pawprint_weight: 0 },
            SpellMode {
                target_requirement: Some(any_target_requirement()),
                effect: Some(AbilityEffect::Damage(2)),
                additional_cost: Some(AdditionalCost::LifePayment { amount: 2 }),
                pawprint_weight: 0,
            },
        ],
        mode_choice: ModeChoice::UpTo(2),
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

fn pass_both_and_drain(engine: &mut Engine, p0: PlayerId, p1: PlayerId) -> StepStatus {
    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine);
    engine.submit_action(PlayerAction { player: p1, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine)
}

#[test]
fn mode_with_no_additional_cost_pays_with_an_empty_plan() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, mode_cost_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![0], Vec::new());
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }), "no mana, no additional cost from mode 0 - expected {{0}}-style empty-plan pause, got {status:?}");
    engine.submit_payment(PaymentPlan { payments: Vec::new() }).expect("mode 0 has no additional cost");
    drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    assert_eq!(engine.state().player(p0).unwrap().life, 23, "mode 0's GainLife(3) resolved");
}

#[test]
fn mode_with_an_additional_cost_requires_paying_it() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, mode_cost_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![1], vec![Target::Player(p1)]);
    drain(&mut engine);

    // An empty plan is illegal - mode 1's LifePayment{2} must be paid too.
    let err = engine.submit_payment(PaymentPlan { payments: Vec::new() }).unwrap_err();
    assert!(matches!(err, PaymentError::InvalidPlan));

    engine
        .submit_payment(PaymentPlan { payments: vec![CostPayment::Life(2)] })
        .expect("paying the mode's own LifePayment{2} cost");
    drain(&mut engine);
    assert_eq!(engine.state().player(p0).unwrap().life, 18, "20 - 2 paid immediately, ability not resolved yet");

    pass_both_and_drain(&mut engine, p0, p1);
    assert_eq!(engine.state().player(p1).unwrap().life, 18, "mode 1's Damage(2) resolved");
}

#[test]
fn choosing_both_modes_still_only_requires_the_one_costed_modes_payment() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, mode_cost_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    // CR 700.2h: both modes chosen, and all additional costs must be paid, here just mode 1's.
    cast_spell(&mut engine, p0, card, vec![0, 1], vec![Target::Player(p1)]);
    drain(&mut engine);

    engine
        .submit_payment(PaymentPlan { payments: vec![CostPayment::Life(2)] })
        .expect("mode 1's own LifePayment{2} cost, mode 0 contributes nothing");
    drain(&mut engine);
    assert_eq!(engine.state().player(p0).unwrap().life, 18, "20 - 2 paid immediately");

    pass_both_and_drain(&mut engine, p0, p1);
    assert_eq!(engine.state().player(p0).unwrap().life, 21, "18 + 3 from mode 0's GainLife");
    assert_eq!(engine.state().player(p1).unwrap().life, 18, "mode 1's Damage(2) resolved");
}

#[test]
fn insufficient_life_to_pay_the_modes_cost_is_rejected() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, mode_cost_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);
    // Drop this player's life below the mode's own LifePayment{2} cost.
    state.player_mut(p0).unwrap().life = 1;

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![1], vec![Target::Player(PlayerId(1))]);
    drain(&mut engine);

    let err = engine.submit_payment(PaymentPlan { payments: vec![CostPayment::Life(2)] }).unwrap_err();
    assert!(matches!(err, PaymentError::InvalidPlan), "CR 119.4: can't pay more life than the player has");
}

#[test]
fn non_modal_spell_is_unaffected_by_mode_additional_cost_plumbing() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let target = ObjectId(0);
    state.objects.insert(
        target,
        card_object(
            target,
            CardDefinition {
                definition_id: 1,
                name: "Test Bear".to_string(),
                mana_cost: ManaCost { symbols: Vec::new() },
                color_indicator: None,
                supertypes: SupertypeSet::empty(),
                card_types: CardTypeSet::CREATURE,
                subtypes: Vec::new(),
                oracle_text: String::new(),
                power: Some(PTValue::Fixed(2)),
                toughness: Some(PTValue::Fixed(5)),
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
            },
            p1,
            ZoneKind::Battlefield,
        ),
    );
    let card = ObjectId(2);
    let shock = CardDefinition {
        definition_id: 2,
        name: "Test Shock".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::INSTANT,
        subtypes: Vec::new(),
        oracle_text: "Test Shock deals 2 damage to any target.".to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_shock".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: Some(any_target_requirement()),
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: Some(AbilityEffect::Damage(2)),
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    };
    state.objects.insert(card, card_object(card, shock, p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(target);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, Vec::new(), vec![Target::Object(target)]);
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }));
    engine.submit_payment(PaymentPlan { payments: Vec::new() }).expect("a non-modal card still pays its plain {0} cost with an empty plan");
    drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    assert_eq!(engine.state().object(target).unwrap().damage_marked, 2, "regression guard: unaffected by this slice");
}
