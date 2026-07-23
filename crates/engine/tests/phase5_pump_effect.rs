//! Phase 5 vertical slice: `AbilityEffect::Pump` — `Game.Ops.Pump(power,
//! toughness, duration)`, the "target creature gets +X/+Y until <duration>"
//! shape (CR 611.2a). Closes the other half of `phase5/effect-duration-
//! expiration`: that slice built the drain paths for `EffectDuration::
//! UntilYourNextTurn`/`UntilEndOfCombat`, but nothing could actually create
//! an effect with either duration - `StaticAbilityBuilder` is setup-time/
//! permanent-only. `Pump` is the first real creation path, mirroring
//! `AbilityEffect::Damage`/`Counters`'s own "resolves against every legal
//! target, no script call needed" shape rather than a generic continuous-
//! effect builder.
//!
//! Same harness as `phase5_script_ops_damage.rs`: `CardDefinition.effect`
//! set directly exercises the Rust-native fast path `ResolveStackTop`
//! already takes for a recognized primitive, without needing the actual
//! `Game.Ops.Pump` Lua binding invoked (same precedent every other
//! `AbilityEffect` primitive's own test file already follows).

use std::collections::HashMap;

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::layers::effective_characteristics;
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, hand_battlefield_graveyard_zones, two_player_state as base_two_player_state};

fn creature_definition(definition_id: u64, name: &str, power: i32, toughness: i32) -> CardDefinition {
    CardDefinition {
        definition_id,
        name: name.to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::CREATURE,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: Some(PTValue::Fixed(power)),
        toughness: Some(PTValue::Fixed(toughness)),
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("bear".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        effect: None,
        modes: Vec::new(),
    }
}

/// "Test Growth: target creature gets +3/+3 until <duration>."
fn growth_definition(duration: EffectDuration) -> CardDefinition {
    CardDefinition {
        definition_id: 20,
        name: "Test Growth".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::INSTANT,
        subtypes: Vec::new(),
        oracle_text: "Test Growth: target creature gets +3/+3.".to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_growth".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: Some(TargetSpec::Primitive(TargetRequirement {
            filter: TargetFilter::AnyTarget,
            count: 1,
            up_to: false, at_least: false,
            exclude_source: false,
        })),
        alternative_costs: Vec::new(),
        effect: Some(AbilityEffect::Pump { power: 3, toughness: 3, duration }),
        modes: Vec::new(),
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

fn cast_spell(engine: &mut Engine, player: PlayerId, card: ObjectId, targets: Vec<Target>) {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::CastSpell { card, modes: Vec::new(), targets, x_value: None, alternative_cost: None },
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

/// Passes priority for whichever player the engine is waiting on until the
/// active player rotates - i.e. until Cleanup has actually run, regardless
/// of exactly how many steps (combat, end step, ...) sit in between.
fn advance_to_next_turn(engine: &mut Engine) {
    let starting_turn = engine.state().turn_number;
    loop {
        match drain(engine) {
            StepStatus::AwaitingAction { player } => {
                engine.submit_action(PlayerAction { player, kind: PlayerActionKind::PassPriority }).unwrap();
            }
            other => panic!("unexpected pause while advancing turns: {other:?}"),
        }
        if engine.state().turn_number != starting_turn {
            break;
        }
    }
}

#[test]
fn pump_spell_boosts_its_target_immediately_on_resolution() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let target = ObjectId(0);
    state.objects.insert(target, card_object(target, creature_definition(1, "Bear", 2, 2), p1, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(target);
    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, growth_definition(EffectDuration::UntilEndOfTurn), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(target)]);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    let effective = effective_characteristics(engine.state(), &ScriptRuntime::new(), target);
    assert_eq!(effective.power, Some(5), "2 base + 3 from Pump");
    assert_eq!(effective.toughness, Some(5));
    assert_eq!(engine.state().continuous_effects.len(), 1);
    assert!(matches!(engine.state().continuous_effects[0].duration, EffectDuration::UntilEndOfTurn));
}

#[test]
fn pump_effect_is_pinned_to_its_target_not_the_spells_own_source() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let target = ObjectId(0);
    let bystander = ObjectId(1);
    state.objects.insert(target, card_object(target, creature_definition(1, "Bear", 2, 2), p1, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(target);
    state.objects.insert(bystander, card_object(bystander, creature_definition(2, "Bystander", 1, 1), p0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(bystander);
    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, growth_definition(EffectDuration::UntilEndOfTurn), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(target)]);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    let bystander_pt = effective_characteristics(engine.state(), &ScriptRuntime::new(), bystander);
    assert_eq!(bystander_pt.power, Some(1), "untargeted creature is untouched");
    assert_eq!(bystander_pt.toughness, Some(1));
}

#[test]
fn until_end_of_turn_pump_expires_by_the_start_of_the_next_turn() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let target = ObjectId(0);
    state.objects.insert(target, card_object(target, creature_definition(1, "Bear", 2, 2), p1, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(target);
    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, growth_definition(EffectDuration::UntilEndOfTurn), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(target)]);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);
    assert_eq!(engine.state().continuous_effects.len(), 1, "sanity: the effect was created");

    advance_to_next_turn(&mut engine);

    assert_eq!(engine.state().continuous_effects.len(), 0, "UntilEndOfTurn drains at cleanup");
    let effective = effective_characteristics(engine.state(), &ScriptRuntime::new(), target);
    assert_eq!(effective.power, Some(2), "back to base stats once the effect is gone");
    assert_eq!(effective.toughness, Some(2));
}

#[test]
fn permanent_duration_pump_survives_into_the_next_turn() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let target = ObjectId(0);
    state.objects.insert(target, card_object(target, creature_definition(1, "Bear", 2, 2), p1, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(target);
    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, growth_definition(EffectDuration::Permanent), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(target)]);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    advance_to_next_turn(&mut engine);

    assert_eq!(engine.state().continuous_effects.len(), 1, "Permanent duration is unaffected by cleanup");
}
