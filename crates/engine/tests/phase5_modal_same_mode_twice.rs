//! Phase 5 vertical slice: CR 700.2d's own "same mode twice" exception —
//! "some modal spells include the instruction 'You may choose the same mode
//! more than once.'" (Profane Command is the classic example.) A repeated
//! mode is treated as if it appeared that many times in sequence: each
//! instance gets its own target slot (the same object/player may be chosen
//! for each, or different ones), and if the repeated mode carries a CR
//! 700.2h additional cost, that cost is paid once per instance.
//!
//! `ModeChoice::ExactlyRepeatable(u32)`/`UpToRepeatable(u32)` are new
//! variants (not a field added to `Exactly`/`UpTo` or to `CardDefinition`
//! itself — either would ripple through every bare `CardDefinition`/
//! `ModeChoice::Exactly(1)` struct literal in this test suite, the same
//! class of change `phase5_modal_multi_mode.rs` already paid for once).
//! `commands::execute_command`'s `CastSpell` arm only skips its own dedup
//! check when the resolved `mode_choice` is one of these two variants —
//! everything downstream (sorting chosen indices, building `TargetSpec::
//! Multiple`/`AbilityEffect::Modal`, `ResolveStackTop`'s own resolution) was
//! already agnostic to whether two chosen-mode chunks came from the same
//! `SpellMode` index or two different ones, so none of it changed.

use std::collections::HashMap;

use stackfish_engine::commands::CommandError;
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

/// "Test Repeatable: Choose two —
///  * You gain 3 life.
///  * Test Repeatable deals 2 damage to any target.
///  You may choose the same mode more than once."
fn repeatable_definition(mode_choice: ModeChoice) -> CardDefinition {
    CardDefinition {
        definition_id: 50,
        name: "Test Repeatable".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::INSTANT,
        subtypes: Vec::new(),
        oracle_text: "Choose two - \
            * You gain 3 life. \
            * Test Repeatable deals 2 damage to any target. \
            You may choose the same mode more than once."
            .to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_repeatable".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        effect: None,
        modes: vec![
            SpellMode { target_requirement: None, effect: Some(AbilityEffect::GainLife(3)), additional_cost: None, pawprint_weight: 0 },
            SpellMode {
                target_requirement: Some(any_target_requirement()),
                effect: Some(AbilityEffect::Damage(2)),
                additional_cost: None,
                pawprint_weight: 0,
            },
        ],
        mode_choice,
    }
}

/// "Test Repeatable Cost: Choose two —
///  * You gain 3 life.
///  * Pay 2 life. Test Repeatable Cost deals 2 damage to any target.
///  You may choose the same mode more than once."
fn repeatable_cost_definition() -> CardDefinition {
    CardDefinition {
        modes: vec![
            SpellMode { target_requirement: None, effect: Some(AbilityEffect::GainLife(3)), additional_cost: None, pawprint_weight: 0 },
            SpellMode {
                target_requirement: Some(any_target_requirement()),
                effect: Some(AbilityEffect::Damage(2)),
                additional_cost: Some(AdditionalCost::LifePayment { amount: 2 }),
                pawprint_weight: 0,
            },
        ],
        mode_choice: ModeChoice::ExactlyRepeatable(2),
        ..repeatable_definition(ModeChoice::ExactlyRepeatable(2))
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
            kind: PlayerActionKind::CastSpell { card, modes, targets, x_value: None, alternative_cost: None },
        })
        .expect("player holds priority, spell is in hand");
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
fn choosing_the_same_mode_twice_doubles_its_effect_when_repeatable() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, repeatable_definition(ModeChoice::ExactlyRepeatable(2)), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    // Mode 0 (GainLife(3), no target) chosen twice.
    cast_spell(&mut engine, p0, card, vec![0, 0], Vec::new());
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    assert_eq!(engine.state().player(p0).unwrap().life, 26, "20 + 3 + 3 - mode 0 resolved twice, once per chosen instance");
}

#[test]
fn repeated_mode_with_a_target_allows_reusing_the_same_target() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, repeatable_definition(ModeChoice::ExactlyRepeatable(2)), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    // Mode 1 (Damage(2), any target) chosen twice, same target both times -
    // CR 700.2d: "the same player or object may be chosen as the target for
    // each of those modes."
    cast_spell(&mut engine, p0, card, vec![1, 1], vec![Target::Player(p1), Target::Player(p1)]);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    assert_eq!(engine.state().player(p1).unwrap().life, 16, "20 - 2 - 2 - both damage instances hit the same target");
}

#[test]
fn repeated_mode_with_a_target_allows_choosing_different_targets() {
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
                effect: None,
                modes: Vec::new(),
                mode_choice: ModeChoice::Exactly(1),
            },
            p1,
            ZoneKind::Battlefield,
        ),
    );
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(target);
    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, repeatable_definition(ModeChoice::ExactlyRepeatable(2)), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    // Mode 1 chosen twice, a different target for each instance.
    cast_spell(&mut engine, p0, card, vec![1, 1], vec![Target::Player(p1), Target::Object(target)]);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    assert_eq!(engine.state().player(p1).unwrap().life, 18, "first instance's Damage(2) hit the player");
    assert_eq!(engine.state().object(target).unwrap().damage_marked, 2, "second instance's Damage(2) hit the creature");
}

#[test]
fn repeated_mode_with_an_additional_cost_charges_it_once_per_instance() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, repeatable_cost_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    // Mode 1 (LifePayment{2}) chosen twice - CR 700.2h + 700.2d: both
    // instances' costs must be paid, not just one.
    cast_spell(&mut engine, p0, card, vec![1, 1], vec![Target::Player(p1), Target::Player(p1)]);
    drain(&mut engine);

    let err = engine.submit_payment(PaymentPlan { payments: vec![CostPayment::Life(2)] }).unwrap_err();
    assert!(matches!(err, PaymentError::InvalidPlan), "only one instance's cost paid - the second is still owed");

    engine
        .submit_payment(PaymentPlan { payments: vec![CostPayment::Life(2), CostPayment::Life(2)] })
        .expect("both instances' own LifePayment{2} cost paid");
    drain(&mut engine);
    assert_eq!(engine.state().player(p0).unwrap().life, 16, "20 - 2 - 2 paid immediately, ability not resolved yet");

    pass_both_and_drain(&mut engine, p0, p1);
    assert_eq!(engine.state().player(p1).unwrap().life, 16, "20 - 2 - 2 - both damage instances resolved");
}

#[test]
fn up_to_repeatable_allows_choosing_one_mode_three_times() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, repeatable_definition(ModeChoice::UpToRepeatable(3)), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![0, 0, 0], Vec::new());
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    assert_eq!(engine.state().player(p0).unwrap().life, 29, "20 + 3*3 - mode 0 resolved three times");
}

#[test]
fn a_non_repeatable_modal_card_still_rejects_a_repeated_index() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let card = ObjectId(2);
    // Same two modes, but plain `Exactly` - not repeatable.
    state.objects.insert(card, card_object(card, repeatable_definition(ModeChoice::Exactly(2)), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![0, 0], Vec::new());
    let status = drain(&mut engine);

    assert!(matches!(status, StepStatus::Error(CommandError::IllegalAction(_))));
    assert_eq!(engine.state().object(card).unwrap().zone, ZoneKind::Hand, "state untouched");
}
