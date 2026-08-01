//! Phase 5 vertical slice: CR 700.2d — choosing two or more modes of a modal
//! spell. Builds on `phase5_script_modal_spells.rs`'s "choose exactly one"
//! (CR 700.2a) machinery: `CardDefinition.mode_choice: ModeChoice` now names
//! how many of `modes` must be chosen (`Exactly(n)`/`UpTo(n)`), and choosing
//! 2+ combines the chosen modes' own target requirements into one
//! `TargetSpec::Multiple` and their own effects into one
//! `AbilityEffect::Modal`, resolved in *card-print* order (not the order the
//! player listed them in) via `ResolveStackTop`'s own `Modal` arm.
//!
//! Narrowed to the synchronous `AbilityEffect` subset `Modal` can resolve
//! (`GainLife`/`Damage`/`Counters`/`Pump`/`GainControl`/`Shuffle` — see
//! `commands::is_modal_resolvable`'s own doc comment); combining a mode
//! using an unsupported kind (e.g. `Destroy`, which pauses mid-resolution)
//! is rejected at cast time. CR 700.2d's "same mode twice" exception
//! (reminder-text-gated) stays unmodeled — a repeated index is always
//! rejected. CR 700.2e/h/i stay out of scope, same as before this slice.

use std::collections::HashMap;

use stackfish_engine::commands::CommandError;
use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, hand_battlefield_graveyard_zones, two_player_state as base_two_player_state};

fn creature_definition(definition_id: u64, name: &str) -> CardDefinition {
    CardDefinition {
        definition_id,
        name: name.to_string(),
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

/// "Choose two —
///  * You gain 3 life.
///  * Test Modal Spell deals 2 damage to any target.
///  * Destroy target creature."
/// Mode 2 (Destroy) is deliberately an unsupported-inside-Modal effect kind
/// - used only by the rejection test below, never combined with a
/// supported mode in a success-path test.
fn choose_two_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 31,
        name: "Test Choose Two".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::INSTANT,
        subtypes: Vec::new(),
        oracle_text: "Choose two - \
            * You gain 3 life. \
            * Test Choose Two deals 2 damage to any target. \
            * Destroy target creature."
            .to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_choose_two".to_string()),
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
            SpellMode {
                target_requirement: Some(TargetSpec::Primitive(TargetRequirement {
                    filter: TargetFilter::Object {
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
                    exclude_source: false,
                })),
                effect: Some(AbilityEffect::Destroy),
                additional_cost: None,
                pawprint_weight: 0,
            },
        ],
        mode_choice: ModeChoice::Exactly(2),
    }
}

/// Same three modes, but "Choose one or two —" (`UpTo(2)`) instead of a
/// fixed "Choose two —".
fn choose_up_to_two_definition() -> CardDefinition {
    CardDefinition { mode_choice: ModeChoice::UpTo(2), ..choose_two_definition() }
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
fn choosing_modes_zero_and_one_resolves_both_effects_against_their_own_targets() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, choose_two_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    // Mode 0 has no target, mode 1 needs exactly one - only mode 1's own
    // target is supplied.
    cast_spell(&mut engine, p0, card, vec![0, 1], vec![Target::Player(p1)]);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    assert_eq!(engine.state().player(p0).unwrap().life, 23, "mode 0's GainLife(3) resolved");
    assert_eq!(engine.state().player(p1).unwrap().life, 18, "mode 1's Damage(2) resolved against its own target");
}

#[test]
fn chosen_mode_order_in_the_action_does_not_matter_resolution_is_card_print_order() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, choose_two_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    // Listed as [1, 0] instead of [0, 1] - same two modes, same outcome,
    // since resolution order is always card-print order (0 then 1), not
    // submission order.
    cast_spell(&mut engine, p0, card, vec![1, 0], vec![Target::Player(p1)]);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    assert_eq!(engine.state().player(p0).unwrap().life, 23, "mode 0's GainLife(3) resolved");
    assert_eq!(engine.state().player(p1).unwrap().life, 18, "mode 1's Damage(2) resolved");
}

#[test]
fn choosing_the_same_mode_twice_is_rejected() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, choose_two_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![0, 0], Vec::new());
    let status = drain(&mut engine);

    assert!(matches!(status, StepStatus::Error(CommandError::IllegalAction(_))));
    assert_eq!(engine.state().object(card).unwrap().zone, ZoneKind::Hand, "state untouched");
}

#[test]
fn choosing_only_one_mode_when_exactly_two_are_required_is_rejected() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, choose_two_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![0], Vec::new());
    let status = drain(&mut engine);

    assert!(matches!(status, StepStatus::Error(CommandError::IllegalAction(_))));
    assert_eq!(engine.state().object(card).unwrap().zone, ZoneKind::Hand, "state untouched");
}

#[test]
fn choosing_all_three_modes_when_exactly_two_are_required_is_rejected() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let target = ObjectId(0);
    state.objects.insert(target, card_object(target, creature_definition(1, "Bear"), p0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(target);
    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, choose_two_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![0, 1, 2], vec![Target::Player(p0), Target::Object(target)]);
    let status = drain(&mut engine);

    assert!(matches!(status, StepStatus::Error(CommandError::IllegalAction(_))));
    assert_eq!(engine.state().object(card).unwrap().zone, ZoneKind::Hand, "state untouched");
}

#[test]
fn combining_a_mode_with_an_unsupported_effect_kind_is_rejected_at_cast_time() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let target = ObjectId(0);
    state.objects.insert(target, card_object(target, creature_definition(1, "Bear"), p0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(target);
    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, choose_two_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    // Mode 2 (Destroy) can't be combined with another mode - it pauses
    // mid-resolution, which the Modal arm doesn't support.
    cast_spell(&mut engine, p0, card, vec![0, 2], vec![Target::Object(target)]);
    let status = drain(&mut engine);

    assert!(matches!(status, StepStatus::Error(CommandError::IllegalAction(_))));
    assert_eq!(engine.state().object(card).unwrap().zone, ZoneKind::Hand, "state untouched");
}

#[test]
fn up_to_two_allows_choosing_just_one_mode() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, choose_up_to_two_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![0], Vec::new());
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    assert_eq!(engine.state().player(p0).unwrap().life, 23, "single chosen mode still uses the pre-Modal bare-effect shape");
}

#[test]
fn up_to_two_allows_choosing_both_modes() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, choose_up_to_two_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![0, 1], vec![Target::Player(p1)]);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    assert_eq!(engine.state().player(p0).unwrap().life, 23, "mode 0 resolved");
    assert_eq!(engine.state().player(p1).unwrap().life, 18, "mode 1 resolved");
}

#[test]
fn up_to_two_rejects_choosing_zero_modes() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, choose_up_to_two_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    // CR 700.2a: "choose a number of those options" - never zero, even for
    // an UpTo spell.
    cast_spell(&mut engine, p0, card, Vec::new(), Vec::new());
    let status = drain(&mut engine);

    assert!(matches!(status, StepStatus::Error(CommandError::IllegalAction(_))));
    assert_eq!(engine.state().object(card).unwrap().zone, ZoneKind::Hand, "state untouched");
}

#[test]
fn a_target_illegal_for_its_own_mode_rejects_the_whole_cast() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, choose_two_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    // Modes 0 and 1 chosen, but no target supplied for mode 1's own
    // requirement - illegal, same as any missing-target cast.
    cast_spell(&mut engine, p0, card, vec![0, 1], Vec::new());
    let status = drain(&mut engine);

    assert!(matches!(status, StepStatus::Error(CommandError::InvalidTarget)));
    assert_eq!(engine.state().object(card).unwrap().zone, ZoneKind::Hand, "state untouched");
}
