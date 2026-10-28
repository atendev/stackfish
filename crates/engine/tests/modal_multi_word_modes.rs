//! A mode with several target words (a `TargetSpec::Multiple`, such as "two target creatures fight each other") combined with other modes. All words form one flat `TargetSpec::Multiple`, and `AbilityEffect::Modal::words[i]` says how many mode i owns.

use stackfish_engine::engine::{ActionError, Engine, StepStatus};
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, minimal_card_definition, two_player_state as base_two_player_state, zones};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const SPELL: ObjectId = ObjectId(100);

fn creature(id: u64, name: &str, power: i32, toughness: i32, keywords: KeywordSet, oracle: &str) -> CardDefinition {
    CardDefinition {
        script_ref: ScriptReference::Inline("return { register_abilities = function(c) end }".to_string()),
        power: Some(PTValue::Fixed(power)),
        toughness: Some(PTValue::Fixed(toughness)),
        keywords,
        oracle_text: oracle.to_string(),
        ..minimal_card_definition(id, name, oracle, ManaCost { symbols: Vec::new() }, CardTypeSet::CREATURE)
    }
}

fn creature_word() -> TargetRequirement {
    TargetRequirement {
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
        exclude_source: false,
    }
}

fn two_creatures() -> Option<TargetSpec> {
    Some(TargetSpec::Multiple(vec![creature_word(), creature_word()]))
}

fn one_creature() -> Option<TargetSpec> {
    Some(TargetSpec::Primitive(creature_word()))
}

fn player() -> Option<TargetSpec> {
    Some(TargetSpec::Primitive(TargetRequirement {
        filter: TargetFilter::Player { controller: None },
        count: 1,
        up_to: false,
        at_least: false,
        exclude_source: false,
    }))
}

fn mode(target_requirement: Option<TargetSpec>, effect: AbilityEffect) -> SpellMode {
    SpellMode { target_requirement, effect: Some(effect), additional_cost: None, pawprint_weight: 0 }
}

fn obj(id: u64) -> Target {
    Target::Object(ObjectId(id))
}

fn engine_with(modes: Vec<SpellMode>, choose: u32) -> Engine {
    let spell = CardDefinition {
        modes,
        mode_choice: ModeChoice::Exactly(choose),
        ..minimal_card_definition(99, "Test Spell", "unused", ManaCost { symbols: Vec::new() }, CardTypeSet::INSTANT)
    };
    let mut state = GameState {
        step: Step::PreCombatMain,
        ..base_two_player_state(zones(P0, P1, &[ZoneKind::Hand, ZoneKind::Library, ZoneKind::Battlefield, ZoneKind::Graveyard, ZoneKind::Exile]))
    };
    let mut place = |id: u64, def: CardDefinition, owner: PlayerId, zone: ZoneKind| {
        state.objects.insert(ObjectId(id), card_object(ObjectId(id), def, owner, zone));
        let scope = if zone == ZoneKind::Battlefield { None } else { Some(owner) };
        state.zone_mut(zone, scope).unwrap().push_bottom(ObjectId(id));
    };
    place(1, creature(1, "A", 3, 3, KeywordSet::FLYING, "Flying"), P0, ZoneKind::Battlefield);
    place(2, creature(2, "B", 2, 2, KeywordSet::TRAMPLE, "Trample"), P1, ZoneKind::Battlefield);
    place(3, creature(3, "C", 1, 1, KeywordSet::empty(), ""), P1, ZoneKind::Battlefield);
    place(4, creature(4, "D", 2, 2, KeywordSet::empty(), ""), P0, ZoneKind::Battlefield);
    place(SPELL.0, spell, P0, ZoneKind::Hand);
    Engine::new(state)
}

fn try_cast(engine: &mut Engine, chosen: Vec<u32>, targets: Vec<Target>) -> Result<StepStatus, ActionError> {
    engine.submit_action(PlayerAction {
        player: P0,
        kind: PlayerActionKind::CastSpell { card: SPELL, modes: chosen, targets, x_value: None, alternative_cost: None, pay_buyback: false },
    })?;
    Ok(drain(engine))
}

fn cast(engine: &mut Engine, chosen: Vec<u32>, targets: Vec<Target>) {
    let status = try_cast(engine, chosen, targets).expect("p0 holds priority");
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }), "{status:?}");
    engine.submit_payment(PaymentPlan { payments: Vec::new() }).unwrap();
    drain(engine);
}

fn pass_both(engine: &mut Engine) -> StepStatus {
    engine.submit_action(PlayerAction { player: P0, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine);
    engine.submit_action(PlayerAction { player: P1, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine)
}

fn life(engine: &Engine, player: PlayerId) -> i32 {
    engine.state().player(player).unwrap().life
}

fn zone(engine: &Engine, id: u64) -> Option<ZoneKind> {
    engine.state().object(ObjectId(id)).map(|o| o.zone)
}

fn alive(engine: &Engine, id: u64) -> bool {
    zone(engine, id) == Some(ZoneKind::Battlefield)
}

fn assert_resolved_cleanly(engine: &Engine) {
    assert!(engine.state().stack.is_empty(), "stack drained");
    let markers = engine.state().ledger.iter().filter(|e| matches!(e.kind, GameEventKind::StackObjectResolved { .. })).count();
    assert_eq!(markers, 1, "the stack object resolves once, after its last mode");
    assert!(!engine.state().ledger.iter().any(|e| matches!(e.kind, GameEventKind::Fizzled { .. })), "not fizzled");
}

#[test]
fn an_exchange_mode_swaps_two_targets_text_boxes_and_the_other_mode_runs() {
    let mut engine = engine_with(vec![mode(two_creatures(), AbilityEffect::ExchangeTextBoxes), mode(None, AbilityEffect::GainLife(3))], 2);
    cast(&mut engine, vec![0, 1], vec![obj(1), obj(2)]);
    pass_both(&mut engine);

    let runtime = ScriptRuntime::new();
    let a = stackfish_engine::layers::effective_characteristics(engine.state(), &runtime, ObjectId(1));
    let b = stackfish_engine::layers::effective_characteristics(engine.state(), &runtime, ObjectId(2));
    assert!(a.keywords.contains(KeywordSet::TRAMPLE) && !a.keywords.contains(KeywordSet::FLYING), "A took B's text box");
    assert!(b.keywords.contains(KeywordSet::FLYING) && !b.keywords.contains(KeywordSet::TRAMPLE), "B took A's text box");
    assert_eq!(life(&engine, P0), 23);
    assert_resolved_cleanly(&engine);
}

#[test]
fn a_mode_printed_after_nothing_else_still_gets_its_own_word_range() {
    // GainLife owns word 0, the exchange owns words 1 and 2.
    let mut engine = engine_with(vec![mode(None, AbilityEffect::GainLife(3)), mode(two_creatures(), AbilityEffect::ExchangeTextBoxes)], 2);
    cast(&mut engine, vec![0, 1], vec![obj(1), obj(2)]);
    pass_both(&mut engine);

    assert!(engine.state().ledger.iter().any(|e| matches!(e.kind, GameEventKind::TextBoxesExchanged { a, b } if a == ObjectId(1) && b == ObjectId(2))));
    assert_eq!(life(&engine, P0), 23);
    assert_resolved_cleanly(&engine);
}

#[test]
fn three_modes_with_spans_of_two_one_and_one_each_read_their_own_targets() {
    let mut engine = engine_with(
        vec![mode(two_creatures(), AbilityEffect::Fight), mode(one_creature(), AbilityEffect::Tap), mode(player(), AbilityEffect::LoseLife(2))],
        3,
    );
    // Fight A and B, tap C, P1 loses 2.
    cast(&mut engine, vec![0, 1, 2], vec![obj(1), obj(2), obj(3), Target::Player(P1)]);
    pass_both(&mut engine);

    assert_eq!(engine.state().object(ObjectId(1)).unwrap().damage_marked, 2, "A took B's power");
    assert!(!alive(&engine, 2), "B took 3 and died");
    assert!(engine.state().object(ObjectId(3)).unwrap().tapped, "the one-word mode read word 2");
    assert_eq!(life(&engine, P1), 18, "the last mode read word 3");
    assert_resolved_cleanly(&engine);
}

#[test]
fn two_multi_word_modes_each_read_their_own_pair() {
    let mut engine = engine_with(vec![mode(two_creatures(), AbilityEffect::ExchangeTextBoxes), mode(two_creatures(), AbilityEffect::Fight)], 2);
    // Exchange A and B, then C fights D.
    cast(&mut engine, vec![0, 1], vec![obj(1), obj(2), obj(3), obj(4)]);
    pass_both(&mut engine);

    assert!(engine.state().ledger.iter().any(|e| matches!(e.kind, GameEventKind::TextBoxesExchanged { a, b } if a == ObjectId(1) && b == ObjectId(2))));
    assert!(!alive(&engine, 3), "C (1/1) took D's 2 damage");
    assert_eq!(engine.state().object(ObjectId(4)).unwrap().damage_marked, 1, "D took C's 1");
    assert_resolved_cleanly(&engine);
}

#[test]
fn a_fight_mode_whose_word_left_does_nothing_but_the_other_mode_still_runs() {
    let mut engine = engine_with(vec![mode(two_creatures(), AbilityEffect::Fight), mode(one_creature(), AbilityEffect::Tap)], 2);
    cast(&mut engine, vec![0, 1], vec![obj(1), obj(2), obj(3)]);
    // B leaves before resolution (CR 701.14b: neither creature deals damage).
    let state = engine.state_mut();
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().contents.retain(|o| *o != ObjectId(2));
    state.zone_mut(ZoneKind::Graveyard, Some(P1)).unwrap().push_bottom(ObjectId(2));
    state.object_mut(ObjectId(2)).unwrap().zone = ZoneKind::Graveyard;
    pass_both(&mut engine);

    assert_eq!(engine.state().object(ObjectId(1)).unwrap().damage_marked, 0, "no fight happened");
    assert!(engine.state().object(ObjectId(3)).unwrap().tapped, "the tap mode still resolved");
    assert_resolved_cleanly(&engine);
}

#[test]
fn too_few_targets_for_the_combined_words_are_rejected_at_cast_time() {
    let mut engine = engine_with(vec![mode(two_creatures(), AbilityEffect::Fight), mode(one_creature(), AbilityEffect::Tap)], 2);
    let result = try_cast(&mut engine, vec![0, 1], vec![obj(1), obj(2)]);

    assert!(matches!(result, Err(ActionError::Rejected(_))), "{result:?}");
    assert_eq!(zone(&engine, SPELL.0), Some(ZoneKind::Hand), "state untouched");
}

#[test]
fn a_mode_with_an_empty_multi_target_requirement_is_rejected() {
    let mut engine = engine_with(vec![mode(Some(TargetSpec::Multiple(Vec::new())), AbilityEffect::Fight), mode(None, AbilityEffect::GainLife(3))], 2);
    let result = try_cast(&mut engine, vec![0, 1], Vec::new());

    assert!(matches!(result, Err(ActionError::Rejected(_))), "{result:?}");
    assert_eq!(zone(&engine, SPELL.0), Some(ZoneKind::Hand), "state untouched");
}

#[test]
fn a_multi_word_mode_works_on_a_modal_triggered_ability() {
    let mut engine = engine_with(Vec::new(), 1);
    engine.state_mut().phase = Phase::PreGame;
    engine.state_mut().step = Step::Upkeep;
    engine.state_mut().abilities.push(RegisteredAbility {
        modes: vec![mode(two_creatures(), AbilityEffect::Fight), mode(None, AbilityEffect::GainLife(3))],
        mode_choice: ModeChoice::Exactly(2),
        id: AbilityId(0),
        source: ObjectId(1),
        kind: AbilityKind::Triggered,
        condition: Some(ConditionSpec::Primitive(ConditionPrimitive::BeginningOfYourUpkeep)),
        cost: None,
        target: None,
        operation: ScriptHandle(0),
        effect: None,
        intervening_if: None,
    });
    let status = pass_both(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingTriggerModeChoice { .. }), "{status:?}");
    engine.submit_trigger_mode_choice(vec![0, 1]).unwrap();
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingTriggerTargets { .. }), "{status:?}");
    engine.submit_trigger_targets(vec![obj(1), obj(2)]).unwrap();
    drain(&mut engine);
    pass_both(&mut engine);

    assert!(!alive(&engine, 2), "A (3 power) killed B");
    assert_eq!(engine.state().object(ObjectId(1)).unwrap().damage_marked, 2);
    assert_eq!(life(&engine, P0), 23);
    assert_resolved_cleanly(&engine);
}
