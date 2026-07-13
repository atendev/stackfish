//! Phase 5 vertical slice: `AbilityEffect::Counters` (backlog item 8, third
//! Script API primitive) — `Game.Ops.Counters(kind, n)`, e.g. "Put two +1/+1
//! counters on target creature." Cheaper than `Destroy`: `GameEventKind::
//! CounterAdded` already existed as dead code with exactly this shape, so
//! this reuses `Damage`'s "push an unapplied event, let `CommitEvents`
//! retroactively apply it" pattern rather than `Destroy`'s heavier inline
//! replacement-check path.
//!
//! Object targets only this slice - no player-facing counters (Energy/
//! Experience). No replacement/prevention modeling for counter placement
//! (no `EventFilter::CounterAdded`). No CR 704.5q +1/+1-vs--1/-1
//! annihilation SBA - both kinds can coexist un-annihilated; `net_counters`
//! still nets them correctly for P/T purposes regardless (see
//! docs/implementation.md).

use std::collections::{HashMap, VecDeque};

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::layers::effective_characteristics;
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;

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
        effect: None,
    }
}

/// "Test Counters: Put N counters of a kind on target creature."
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
            filter: TargetFilter::Object { card_types: Some(CardTypeSet::CREATURE), controller: None, zone: None },
            count: 1,
        })),
        alternative_costs: Vec::new(),
        effect: Some(AbilityEffect::Counters { kind, amount }),
    }
}

fn card_object(id: ObjectId, definition: CardDefinition, owner: PlayerId, zone: ZoneKind) -> GameObject {
    GameObject {
        object_id: id,
        lineage: ObjectLineage { previous: None },
        definition,
        owner,
        controller: owner,
        zone,
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
        controlled_since_turn: 0,
        is_token: false,
        visibility: VisibilityPolicy { public: true, visible_to: Vec::new() },
    }
}

fn two_player_state() -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        players: vec![
            PlayerState {
                player_id: p0,
                life: 20,
                mana_pool: ManaPool::default(),
                counters: CounterSet::new(),
                emblems: Vec::new(),
                commander_damage_taken: Vec::new(),
                eliminated: false,
                drew_from_empty_library: false,
                mulligans_taken: 0,
                hand_kept: false,
            },
            PlayerState {
                player_id: p1,
                life: 20,
                mana_pool: ManaPool::default(),
                counters: CounterSet::new(),
                emblems: Vec::new(),
                commander_damage_taken: Vec::new(),
                eliminated: false,
                drew_from_empty_library: false,
                mulligans_taken: 0,
                hand_kept: false,
            },
        ],
        turn_order: vec![p0, p1],
        active_player: p0,
        turn_number: 1,
        phase: Phase::Main,
        step: Step::PreCombatMain,
        priority_player: p0,
        consecutive_passes: 0,
        lands_played_this_turn: HashMap::new(),
        combat: None,
        last_active_turn_number: HashMap::from([(p0, 1), (p1, 1)]),
        objects: HashMap::new(),
        zones: vec![
            Zone { kind: ZoneKind::Hand, owner: Some(p0), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Hand, owner: Some(p1), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Battlefield, owner: None, contents: VecDeque::new() },
            Zone { kind: ZoneKind::Graveyard, owner: Some(p0), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Graveyard, owner: Some(p1), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Exile, owner: Some(p0), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Exile, owner: Some(p1), contents: VecDeque::new() },
        ],
        stack: Vec::new(),
        continuous_effects: Vec::new(),
        replacement_effects: Vec::new(),
        cost_modifiers: Vec::new(),
        abilities: Vec::new(),
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
        match engine.step().status {
            StepStatus::Progressed => continue,
            other => return other,
        }
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
    // amount = 1, not 2: a 2/2 hitting toughness 0 would die to CR 704.5g
    // SBAs before this test can inspect its counters - keep the bear alive.
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
    bear.counters.add(CounterKind::PlusOnePlusOne, 3);
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
        source,
        kind: AbilityKind::Activated,
        condition: None,
        cost: Some(CostSpec { mana: None, additional: vec![AdditionalCost::TapSelf], alternative: Vec::new() }),
        target: Some(TargetSpec::Primitive(TargetRequirement {
            filter: TargetFilter::Object { card_types: Some(CardTypeSet::CREATURE), controller: None, zone: None },
            count: 1,
        })),
        operation: ScriptHandle(0),
        effect: Some(AbilityEffect::Counters { kind: CounterKind::PlusOnePlusOne, amount: 1 }),
    });

    let mut engine = Engine::new(state);
    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::ActivateAbility { source, ability_index: 0, targets: vec![Target::Object(target)] },
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
