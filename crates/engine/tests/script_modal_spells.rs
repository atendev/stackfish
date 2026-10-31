//! CR 700.2a: the controller chooses exactly one of a spell's modes as part of casting, before targets (CR 601.2b precedes 601.2c). `CastSpell::modes` selects which `SpellMode` supplies the target requirement and effect.
//! Multi-mode choice is in modal_multi_mode.rs, opponent-chosen modes (CR 700.2e) in modal_chosen_by_opponent.rs, and mode costs (CR 700.2h) in modal_mode_additional_cost.rs.

use std::collections::HashMap;

use stackfish_engine::commands::Rejected;
use stackfish_engine::engine::ActionError;
use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::*;
use stackfish_testutil::{
    card_object, drain, hand_battlefield_graveyard_zones, two_player_state as base_two_player_state,
};

fn creature_definition(definition_id: u64, name: &str) -> CardDefinition {
    CardDefinition {
        definition_id,
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
    }
}

// CR 601.2c
// Mode 0 has no target requirement, so a spell is cast as though it did not require targets its chosen mode does not ask for. Mode 1 requires one `AnyTarget`.
fn modal_spell_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 30,
        name: "Test Modal Spell".to_string(),
        mana_cost: ManaCost {
            symbols: Vec::new(),
        },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::INSTANT,
        subtypes: Vec::new(),
        oracle_text: "Choose one - \
            * You gain 3 life. \
            * Test Modal Spell deals 2 damage to any target."
            .to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_modal_spell".to_string()),
        keywords: KeywordSet::empty(),
        // Ignored - a modal card's per-mode fields (below) are used instead.
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: vec![
            SpellMode {
                target_requirement: None,
                effect: Some(AbilityEffect::GainLife(3)),
                additional_cost: None,
                pawprint_weight: 0,
            },
            SpellMode {
                target_requirement: Some(TargetSpec::Primitive(TargetRequirement {
                    filter: TargetFilter::AnyTarget,
                    count: 1,
                    up_to: false,
                    at_least: false,
                    exclude_source: false,
                })),
                effect: Some(AbilityEffect::Damage(2)),
                additional_cost: None,
                pawprint_weight: 0,
            },
        ],
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn shock_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 20,
        name: "Test Shock".to_string(),
        mana_cost: ManaCost {
            symbols: Vec::new(),
        },
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
        target_requirement: Some(TargetSpec::Primitive(TargetRequirement {
            filter: TargetFilter::AnyTarget,
            count: 1,
            up_to: false,
            at_least: false,
            exclude_source: false,
        })),
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: Some(AbilityEffect::Damage(2)),
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
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

fn cast_spell(
    engine: &mut Engine,
    player: PlayerId,
    card: ObjectId,
    modes: Vec<u32>,
    targets: Vec<Target>,
) {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::CastSpell {
                card,
                modes,
                targets,
                x_value: None,
                alternative_cost: None,
                pay_buyback: false,
            },
        })
        .expect("player holds priority, spell is in hand");
}

fn try_cast_spell(
    engine: &mut Engine,
    player: PlayerId,
    card: ObjectId,
    modes: Vec<u32>,
    targets: Vec<Target>,
) -> Result<(), ActionError> {
    engine.submit_action(PlayerAction {
        player,
        kind: PlayerActionKind::CastSpell {
            card,
            modes,
            targets,
            x_value: None,
            alternative_cost: None,
            pay_buyback: false,
        },
    })
}

fn pay_free_and_drain(engine: &mut Engine) -> StepStatus {
    let status = drain(engine);
    assert!(
        matches!(status, StepStatus::AwaitingPayment { .. }),
        "expected a pause for the cast's (free) cost, got {status:?}"
    );
    engine
        .submit_payment(PaymentPlan {
            payments: Vec::new(),
        })
        .expect("empty plan pays a {0} cost");
    drain(engine)
}

fn pass_both_and_drain(engine: &mut Engine, p0: PlayerId, p1: PlayerId) -> StepStatus {
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

#[test]
fn choosing_mode_zero_gains_life_and_never_touches_mode_ones_target_requirement() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(2);
    state.objects.insert(
        card,
        card_object(card, modal_spell_definition(), p0, ZoneKind::Hand),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);

    let mut engine = Engine::new(state);
    // No targets are supplied, legal only because mode 0 needs none. A leaked mode 1 AnyTarget would reject this cast.
    cast_spell(&mut engine, p0, card, vec![0], Vec::new());
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    assert_eq!(
        engine.state().player(p0).unwrap().life,
        23,
        "mode 0's GainLife(3) resolved"
    );
    assert_eq!(
        engine.state().player(p1).unwrap().life,
        20,
        "mode 1's Damage never ran"
    );
}

#[test]
fn choosing_mode_one_deals_damage_and_validates_its_own_target_requirement() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let target = ObjectId(0);
    state.objects.insert(
        target,
        card_object(
            target,
            creature_definition(1, "Bear"),
            p1,
            ZoneKind::Battlefield,
        ),
    );
    state
        .zone_mut(ZoneKind::Battlefield, None)
        .unwrap()
        .push_bottom(target);
    let card = ObjectId(2);
    state.objects.insert(
        card,
        card_object(card, modal_spell_definition(), p0, ZoneKind::Hand),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![1], vec![Target::Object(target)]);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    assert_eq!(
        engine.state().object(target).unwrap().damage_marked,
        2,
        "mode 1's Damage(2) resolved"
    );
    assert_eq!(
        engine.state().player(p0).unwrap().life,
        20,
        "mode 0's GainLife never ran"
    );
}

#[test]
fn modal_spell_rejects_a_target_illegal_for_the_chosen_modes_own_requirement() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let card = ObjectId(2);
    state.objects.insert(
        card,
        card_object(card, modal_spell_definition(), p0, ZoneKind::Hand),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);

    let mut engine = Engine::new(state);
    // Mode 1 requires exactly one AnyTarget, so supplying none is illegal.
    let err = try_cast_spell(&mut engine, p0, card, vec![1], Vec::new()).unwrap_err();

    assert!(matches!(
        err,
        ActionError::Rejected(Rejected::InvalidTarget)
    ));
    assert_eq!(
        engine.state().object(card).unwrap().zone,
        ZoneKind::Hand,
        "state untouched"
    );
}

#[test]
fn modal_spell_rejects_casting_with_no_mode_chosen() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let card = ObjectId(2);
    state.objects.insert(
        card,
        card_object(card, modal_spell_definition(), p0, ZoneKind::Hand),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);

    let mut engine = Engine::new(state);
    let err = try_cast_spell(&mut engine, p0, card, Vec::new(), Vec::new()).unwrap_err();

    assert!(matches!(
        err,
        ActionError::Rejected(Rejected::IllegalAction(_))
    ));
    assert_eq!(
        engine.state().object(card).unwrap().zone,
        ZoneKind::Hand,
        "state untouched"
    );
    assert_eq!(engine.state().ledger.len(), 0);
}

#[test]
fn modal_spell_rejects_choosing_two_modes() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let card = ObjectId(2);
    state.objects.insert(
        card,
        card_object(card, modal_spell_definition(), p0, ZoneKind::Hand),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);

    let mut engine = Engine::new(state);
    // Choosing two modes is out of scope here (CR 700.2d), so a non-single selection is rejected.
    let err = try_cast_spell(&mut engine, p0, card, vec![0, 1], Vec::new()).unwrap_err();

    assert!(matches!(
        err,
        ActionError::Rejected(Rejected::IllegalAction(_))
    ));
    assert_eq!(
        engine.state().object(card).unwrap().zone,
        ZoneKind::Hand,
        "state untouched"
    );
}

#[test]
fn modal_spell_rejects_an_out_of_range_mode_index() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let card = ObjectId(2);
    state.objects.insert(
        card,
        card_object(card, modal_spell_definition(), p0, ZoneKind::Hand),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);

    let mut engine = Engine::new(state);
    let err = try_cast_spell(&mut engine, p0, card, vec![7], Vec::new()).unwrap_err();

    assert!(matches!(
        err,
        ActionError::Rejected(Rejected::IllegalAction(_))
    ));
    assert_eq!(
        engine.state().object(card).unwrap().zone,
        ZoneKind::Hand,
        "state untouched"
    );
}

#[test]
fn non_modal_spell_rejects_a_mode_choice() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let target = ObjectId(0);
    state.objects.insert(
        target,
        card_object(
            target,
            creature_definition(1, "Bear"),
            p0,
            ZoneKind::Battlefield,
        ),
    );
    state
        .zone_mut(ZoneKind::Battlefield, None)
        .unwrap()
        .push_bottom(target);
    let card = ObjectId(2);
    state.objects.insert(
        card,
        card_object(card, shock_definition(), p0, ZoneKind::Hand),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);

    let mut engine = Engine::new(state);
    // shock_definition() has no modes - announcing one is illegal.
    let err =
        try_cast_spell(&mut engine, p0, card, vec![0], vec![Target::Object(target)]).unwrap_err();

    assert!(matches!(
        err,
        ActionError::Rejected(Rejected::IllegalAction(_))
    ));
    assert_eq!(
        engine.state().object(card).unwrap().zone,
        ZoneKind::Hand,
        "state untouched"
    );
}

#[test]
fn non_modal_spell_still_casts_normally_with_no_modes_announced() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let target = ObjectId(0);
    state.objects.insert(
        target,
        card_object(
            target,
            creature_definition(1, "Bear"),
            p1,
            ZoneKind::Battlefield,
        ),
    );
    state
        .zone_mut(ZoneKind::Battlefield, None)
        .unwrap()
        .push_bottom(target);
    let card = ObjectId(2);
    state.objects.insert(
        card,
        card_object(card, shock_definition(), p0, ZoneKind::Hand),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(
        &mut engine,
        p0,
        card,
        Vec::new(),
        vec![Target::Object(target)],
    );
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    assert_eq!(
        engine.state().object(target).unwrap().damage_marked,
        2,
        "regression guard: unaffected by this slice"
    );
}
