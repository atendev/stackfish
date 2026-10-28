//! `AbilityEffect::Counters` (`Game.Ops.Counters(kind, n)`) pushes an unapplied `CounterAdded` event that `CommitEvents` applies, like `Damage`. It targets objects only.
// TODO: player counters (Energy, Experience) and `EventFilter::CounterAdded` replacement or prevention
// TODO: CR 704.5q +1/+1 versus -1/-1 annihilation, since both kinds coexist and `net_counters` nets them for P/T

use std::collections::HashMap;

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::layers::effective_characteristics;
use stackfish_engine::script::ScriptRuntime;
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

fn counters_spell_definition(kind: CounterKind, amount: i32) -> CardDefinition {
    CardDefinition {
        definition_id: 20,
        name: "Test Counters".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::INSTANT,
        subtypes: Vec::new(),
        oracle_text: "Put counters on target creature.".to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_counters".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: Some(TargetSpec::Primitive(TargetRequirement {
            filter: TargetFilter::Object { mana_value: None, excluded_card_types: None, excluded_subtypes: None, card_types: Some(CardTypeSet::CREATURE), subtypes: None, supertypes: None, excluded_supertypes: None, controller: None, zone: None },
            count: 1,
            up_to: false, at_least: false,
            exclude_source: false,
        })),
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: Some(AbilityEffect::Counters { kind, amount }),
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn two_player_state() -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let mut zones = hand_battlefield_graveyard_zones(p0, p1);
    zones.push(Zone::new(ZoneKind::Exile, Some(p0)));
    zones.push(Zone::new(ZoneKind::Exile, Some(p1)));
    GameState {
        step: Step::PreCombatMain,
        last_active_turn_number: HashMap::from([(p0, 1), (p1, 1)]),
        ..base_two_player_state(zones)
    }
}

fn cast_spell(engine: &mut Engine, player: PlayerId, card: ObjectId, targets: Vec<Target>) {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::CastSpell { card, modes: Vec::new(), targets, x_value: None, alternative_cost: None , pay_buyback: false},
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
fn counters_spell_puts_plus_one_plus_one_counters_on_target_creature() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let target = ObjectId(0);
    state.objects.insert(target, card_object(target, creature_definition(1, "Bear"), p1, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(target);
    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, counters_spell_definition(CounterKind::PlusOnePlusOne, 2), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(target)]);
    pay_free_and_drain(&mut engine);
    let status = pass_both_and_drain(&mut engine, p0, p1);

    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert_eq!(engine.state().object(target).unwrap().counters.count(&CounterKind::PlusOnePlusOne), 2);

    let script_runtime = ScriptRuntime::new();
    let effective = effective_characteristics(engine.state(), &script_runtime, target);
    assert_eq!(effective.power, Some(4), "CR 122.1a: base 2 + two +1/+1 counters");
    assert_eq!(effective.toughness, Some(4));
}

#[test]
fn counters_spell_puts_minus_one_minus_one_counters_on_target_creature() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let target = ObjectId(0);
    state.objects.insert(target, card_object(target, creature_definition(1, "Bear"), p1, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(target);
    let card = ObjectId(2);
    // amount = 1, because a 2/2 hitting toughness 0 would die to CR 704.5g before this test can inspect its counters.
    state.objects.insert(card, card_object(card, counters_spell_definition(CounterKind::MinusOneMinusOne, 1), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(target)]);
    pay_free_and_drain(&mut engine);
    let status = pass_both_and_drain(&mut engine, p0, p1);

    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert_eq!(engine.state().object(target).unwrap().counters.count(&CounterKind::MinusOneMinusOne), 1);

    let script_runtime = ScriptRuntime::new();
    let effective = effective_characteristics(engine.state(), &script_runtime, target);
    assert_eq!(effective.power, Some(1), "CR 122.1a: base 2 - one -1/-1 counter");
    assert_eq!(effective.toughness, Some(1));
}

#[test]
fn both_counter_kinds_coexist_unannihilated_but_still_net_correctly() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let target = ObjectId(0);
    let mut bear = card_object(target, creature_definition(1, "Bear"), p1, ZoneKind::Battlefield);
    bear.counters.add(CounterKind::PlusOnePlusOne, 3, SequenceNo(0));
    state.objects.insert(target, bear);
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(target);
    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, counters_spell_definition(CounterKind::MinusOneMinusOne, 1), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(target)]);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    let obj = engine.state().object(target).unwrap();
    assert_eq!(obj.counters.count(&CounterKind::PlusOnePlusOne), 3, "no CR 704.5q annihilation - both kinds pile up literally");
    assert_eq!(obj.counters.count(&CounterKind::MinusOneMinusOne), 1);

    let script_runtime = ScriptRuntime::new();
    let effective = effective_characteristics(engine.state(), &script_runtime, target);
    assert_eq!(effective.power, Some(4), "net_counters still nets 2 + 3 - 1 = 4 correctly for P/T despite no annihilation");
    assert_eq!(effective.toughness, Some(4));
}

#[test]
fn counters_ability_works_the_same_as_a_counters_spell() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let source = ObjectId(0);
    let target = ObjectId(1);
    state.objects.insert(source, card_object(source, creature_definition(1, "Druid"), p0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(source);
    state.objects.insert(target, card_object(target, creature_definition(2, "Bear"), p1, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(target);
    state.abilities.push(RegisteredAbility {
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
        id: AbilityId(0),
        source,
        kind: AbilityKind::Activated,
        condition: None,
        cost: Some(CostSpec { mana: None, additional: vec![AdditionalCost::TapSelf], alternative: Vec::new() }),
        target: Some(TargetSpec::Primitive(TargetRequirement {
            filter: TargetFilter::Object { mana_value: None, excluded_card_types: None, excluded_subtypes: None, card_types: Some(CardTypeSet::CREATURE), subtypes: None, supertypes: None, excluded_supertypes: None, controller: None, zone: None },
            count: 1,
            up_to: false, at_least: false,
            exclude_source: false,
        })),
        operation: ScriptHandle(0),
        effect: Some(AbilityEffect::Counters { kind: CounterKind::PlusOnePlusOne, amount: 1 }),
        intervening_if: None,
    });

    let mut engine = Engine::new(state);
    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::ActivateAbility {
                source,
                ability_index: 0,
                targets: vec![Target::Object(target)],
                chosen_colors: Vec::new(),
                x_value: None,
            },
        })
        .expect("player holds priority");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }));
    engine.submit_payment(PaymentPlan { payments: vec![CostPayment::Tap(source)] }).expect("Tap pays TapSelf");
    drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    assert_eq!(engine.state().object(target).unwrap().counters.count(&CounterKind::PlusOnePlusOne), 1);
}

#[test]
fn unrecognized_counter_kind_name_round_trips_through_custom() {
    let mut runtime = ScriptRuntime::new();
    let (effect, _handle) = runtime
        .load_operation("return Game.Ops.Counters(\"Skull\", 3)")
        .expect("Game.Ops.Counters(\"Skull\", 3) should decode via the OperationTag path");
    assert!(
        matches!(effect, Some(AbilityEffect::Counters { kind: CounterKind::Custom(ref name), amount: 3 }) if name == "Skull")
    );
}

#[test]
fn fizzled_counters_spell_adds_no_counters() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let target = ObjectId(0);
    state.objects.insert(target, card_object(target, creature_definition(1, "Bear"), p1, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(target);
    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, counters_spell_definition(CounterKind::PlusOnePlusOne, 2), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(target)]);
    pay_free_and_drain(&mut engine);
    assert_eq!(engine.state().stack.len(), 1);

    // CR 608.2b: the target leaves the battlefield before this resolves.
    engine.state_mut().zone_mut(ZoneKind::Battlefield, None).unwrap().remove(target);
    engine.state_mut().objects.remove(&target);

    let status = pass_both_and_drain(&mut engine, p0, p1);

    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert!(engine.state().stack.is_empty(), "fizzled spell still leaves the stack");
    let fizzled = engine.state().ledger.iter().any(|e| matches!(e.kind, GameEventKind::Fizzled { .. }));
    assert!(fizzled, "expected a Fizzled event");
    let counter_added = engine.state().ledger.iter().any(|e| matches!(e.kind, GameEventKind::CounterAdded { .. }));
    assert!(!counter_added, "no CounterAdded event when the spell fizzled");
}
