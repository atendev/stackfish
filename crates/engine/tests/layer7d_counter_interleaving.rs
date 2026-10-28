//! CR 613.4c, 613.5: power/toughness effects and counters apply together in layer 7c, ordered by timestamp, and a counter added before a `SetPT` override still contributes after it (Gray Ogre).
//! `CounterSet::batches` tracks one timestamped batch per `add`, so the layer 7c fold in `effective_characteristics` can interleave them with `ModifyPT` and `DoublePT`.

use std::collections::HashMap;

use stackfish_engine::layers::effective_characteristics;
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{card_object, two_player_state as base_two_player_state, zones};

fn creature_definition(power: i32, toughness: i32) -> CardDefinition {
    CardDefinition {
        definition_id: 1,
        name: "Test Creature".to_string(),
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
        script_ref: ScriptReference::ByKey("test_creature".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn creature(id: ObjectId, owner: PlayerId, power: i32, toughness: i32) -> GameObject {
    card_object(id, creature_definition(power, toughness), owner, ZoneKind::Battlefield)
}

fn base_state() -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        phase: Phase::Combat,
        step: Step::CombatDamage,
        last_active_turn_number: HashMap::from([(p0, 1)]),
        ..base_two_player_state(zones(p0, p1, &[ZoneKind::Battlefield, ZoneKind::Graveyard]))
    }
}

fn put_on_battlefield(state: &mut GameState, object: GameObject) {
    let id = object.object_id;
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(id);
    state.objects.insert(id, object);
}

fn no_filter() -> EffectFilter {
    EffectFilter {  controller: None, card_types: None, self_only: false, subtypes: None, excluded_subtypes: None, supertypes: None, excluded_supertypes: None, }
}

fn continuous_effect(source: ObjectId, layer: Layer, effect: EffectPrimitive, timestamp: u64) -> ContinuousEffect {
    ContinuousEffect {
        source,
        layer,
        affected: AffectedFilter::Primitive(no_filter()),
        effect: EffectPayload::Primitive(effect),
        timestamp: SequenceNo(timestamp),
        duration: EffectDuration::Permanent,
        condition: None,
    }
}

fn modify(source: ObjectId, power: i32, toughness: i32, timestamp: u64) -> ContinuousEffect {
    continuous_effect(source, Layer::PowerToughnessModifying, EffectPrimitive::ModifyPT { power, toughness }, timestamp)
}

fn double(source: ObjectId, power: bool, toughness: bool, timestamp: u64) -> ContinuousEffect {
    continuous_effect(source, Layer::PowerToughnessModifying, EffectPrimitive::DoublePT { power, toughness }, timestamp)
}

fn set(source: ObjectId, power: i32, toughness: i32, timestamp: u64) -> ContinuousEffect {
    continuous_effect(source, Layer::PowerToughnessSetting, EffectPrimitive::SetPT { power, toughness }, timestamp)
}

// CR 613.5
// Gray Ogre (2/2) gets a +1/+1 counter, +4/+4, +0/+2, then becomes 0/1 in layer 7b, landing on 5/8 because 7b precedes 7c regardless of timestamp.
#[test]
fn cr_613_5_gray_ogre_worked_example() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let ogre = ObjectId(0);
    put_on_battlefield(&mut state, creature(ogre, p0, 2, 2));
    state.object_mut(ogre).unwrap().counters.add(CounterKind::PlusOnePlusOne, 1, SequenceNo(1));
    state.continuous_effects.push(modify(ogre, 4, 4, 2));
    state.continuous_effects.push(modify(ogre, 0, 2, 3));
    state.continuous_effects.push(set(ogre, 0, 1, 4));

    let effective = effective_characteristics(&state, &ScriptRuntime::new(), ogre);

    assert_eq!(effective.power, Some(5));
    assert_eq!(effective.toughness, Some(8));
}

// A counter added before a later SetPT must still contribute after it. If it were pre-netted into the base it would read 0/0.
#[test]
fn counter_survives_a_later_setpt_override() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let bear = ObjectId(0);
    put_on_battlefield(&mut state, creature(bear, p0, 2, 2));
    state.object_mut(bear).unwrap().counters.add(CounterKind::PlusOnePlusOne, 1, SequenceNo(1));
    state.continuous_effects.push(set(bear, 0, 0, 2));

    let effective = effective_characteristics(&state, &ScriptRuntime::new(), bear);

    assert_eq!(effective.power, Some(1), "the counter must still apply after the SetPT override, not get discarded");
    assert_eq!(effective.toughness, Some(1));
}

// CR 613.4c, 613.8a
// A counter added before DoublePT is doubled with everything summed by then: (1+1+1)*2 = 6/6.
#[test]
fn counter_added_before_double_pt_gets_doubled() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let bear = ObjectId(0);
    put_on_battlefield(&mut state, creature(bear, p0, 1, 1));
    state.continuous_effects.push(modify(bear, 1, 1, 1));
    state.object_mut(bear).unwrap().counters.add(CounterKind::PlusOnePlusOne, 1, SequenceNo(2));
    state.continuous_effects.push(double(bear, true, true, 3));

    let effective = effective_characteristics(&state, &ScriptRuntime::new(), bear);

    assert_eq!(effective.power, Some(6));
    assert_eq!(effective.toughness, Some(6));
}

// A counter added after DoublePT is not doubled: (1+1)*2 + 1 = 5/5, so this is timestamp ordering.
#[test]
fn counter_added_after_double_pt_is_not_doubled() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let bear = ObjectId(0);
    put_on_battlefield(&mut state, creature(bear, p0, 1, 1));
    state.continuous_effects.push(modify(bear, 1, 1, 1));
    state.continuous_effects.push(double(bear, true, true, 2));
    state.object_mut(bear).unwrap().counters.add(CounterKind::PlusOnePlusOne, 1, SequenceNo(3));

    let effective = effective_characteristics(&state, &ScriptRuntime::new(), bear);

    assert_eq!(effective.power, Some(5));
    assert_eq!(effective.toughness, Some(5));
}

// A counter with no other layer 7 effects still works.
#[test]
fn counter_alone_still_nets_into_the_final_value() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let bear = ObjectId(0);
    put_on_battlefield(&mut state, creature(bear, p0, 2, 2));
    state.object_mut(bear).unwrap().counters.add(CounterKind::PlusOnePlusOne, 2, SequenceNo(1));
    state.object_mut(bear).unwrap().counters.add(CounterKind::MinusOneMinusOne, 1, SequenceNo(2));

    let effective = effective_characteristics(&state, &ScriptRuntime::new(), bear);

    assert_eq!(effective.power, Some(3), "net +1/+1 (2 plus-counters, 1 minus-counter) over the 2/2 base");
    assert_eq!(effective.toughness, Some(3));
}
