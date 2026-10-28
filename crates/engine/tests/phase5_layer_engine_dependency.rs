//! CR 613.6-613.8: `layers::order_by_dependency` reorders one layer's effects around real dependency (CR 613.8a) and not timestamp. `EffectPrimitive::DoublePT` (Berserk) is the one primitive pairing that can produce a dependency.

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

fn continuous_effect(source: ObjectId, effect: EffectPrimitive, timestamp: u64) -> ContinuousEffect {
    ContinuousEffect {
        source,
        layer: Layer::PowerToughnessModifying,
        affected: AffectedFilter::Primitive(no_filter()),
        effect: EffectPayload::Primitive(effect),
        timestamp: SequenceNo(timestamp),
        duration: EffectDuration::Permanent,
        condition: None,
    }
}

fn modify(source: ObjectId, power: i32, toughness: i32, timestamp: u64) -> ContinuousEffect {
    continuous_effect(source, EffectPrimitive::ModifyPT { power, toughness }, timestamp)
}

fn double(source: ObjectId, power: bool, toughness: bool, timestamp: u64) -> ContinuousEffect {
    continuous_effect(source, EffectPrimitive::DoublePT { power, toughness }, timestamp)
}

fn scripted(source: ObjectId, handle: ScriptHandle, timestamp: u64) -> ContinuousEffect {
    ContinuousEffect {
        source,
        layer: Layer::PowerToughnessModifying,
        affected: AffectedFilter::Primitive(no_filter()),
        effect: EffectPayload::Script(handle),
        timestamp: SequenceNo(timestamp),
        duration: EffectDuration::Permanent,
        condition: None,
    }
}

// Two independent ModifyPTs have no dependency and just sum.
#[test]
fn independent_modify_effects_still_sum_regardless_of_order() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let bear = ObjectId(0);
    put_on_battlefield(&mut state, creature(bear, p0, 2, 2));
    state.continuous_effects.push(modify(bear, 1, 1, 5));
    state.continuous_effects.push(modify(bear, 2, 0, 0));

    let effective = effective_characteristics(&state, &ScriptRuntime::new(), bear);

    assert_eq!(effective.power, Some(5));
    assert_eq!(effective.toughness, Some(3));
}

// CR 613.8a, b
// DoublePT depends on a same-field ModifyPT in the same sublayer, so the modify applies first despite its later timestamp: (2+2)*2 = 8/8.
#[test]
fn double_pt_waits_for_a_later_timestamp_modify_it_depends_on() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let bear = ObjectId(0);
    put_on_battlefield(&mut state, creature(bear, p0, 2, 2));
    state.continuous_effects.push(double(bear, true, true, 0));
    state.continuous_effects.push(modify(bear, 2, 2, 1));

    let effective = effective_characteristics(&state, &ScriptRuntime::new(), bear);

    assert_eq!(effective.power, Some(8));
    assert_eq!(effective.toughness, Some(8));
}

// Timestamps the other way around give the same 8/8, so the previous test was not tie-breaking luck.
#[test]
fn double_pt_applies_after_an_earlier_timestamp_modify_too() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let bear = ObjectId(0);
    put_on_battlefield(&mut state, creature(bear, p0, 2, 2));
    state.continuous_effects.push(modify(bear, 2, 2, 0));
    state.continuous_effects.push(double(bear, true, true, 1));

    let effective = effective_characteristics(&state, &ScriptRuntime::new(), bear);

    assert_eq!(effective.power, Some(8));
    assert_eq!(effective.toughness, Some(8));
}

// A power-only DoublePT leaves toughness untouched.
#[test]
fn double_pt_scoped_to_one_field_leaves_the_other_alone() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let bear = ObjectId(0);
    put_on_battlefield(&mut state, creature(bear, p0, 2, 2));
    state.continuous_effects.push(double(bear, true, false, 0));
    state.continuous_effects.push(modify(bear, 2, 2, 1));

    let effective = effective_characteristics(&state, &ScriptRuntime::new(), bear);

    assert_eq!(effective.power, Some(8));
    assert_eq!(effective.toughness, Some(4));
}

// CR 613.8a
// A scripted DoublePT depends on a primitive ModifyPT the same way, since depends_on resolves the script through resolve_primitive first: 8/8.
#[test]
fn scripted_double_pt_waits_for_a_later_timestamp_modify_it_depends_on() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let bear = ObjectId(0);
    put_on_battlefield(&mut state, creature(bear, p0, 2, 2));

    let mut script_runtime = ScriptRuntime::new();
    let handle = script_runtime
        .load_function("return function(source, controller) return { double_pt = { power = true, toughness = true } } end")
        .unwrap();
    state.continuous_effects.push(scripted(bear, handle, 0));
    state.continuous_effects.push(modify(bear, 2, 2, 1));

    let effective = effective_characteristics(&state, &script_runtime, bear);

    assert_eq!(effective.power, Some(8));
    assert_eq!(effective.toughness, Some(8));
}

// The other direction: a primitive DoublePT depends on a scripted ModifyPT.
#[test]
fn primitive_double_pt_waits_for_a_scripted_modify_it_depends_on() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let bear = ObjectId(0);
    put_on_battlefield(&mut state, creature(bear, p0, 2, 2));

    let mut script_runtime = ScriptRuntime::new();
    let handle = script_runtime
        .load_function("return function(source, controller) return { modify_pt = { power = 2, toughness = 2 } } end")
        .unwrap();
    state.continuous_effects.push(double(bear, true, true, 0));
    state.continuous_effects.push(scripted(bear, handle, 1));

    let effective = effective_characteristics(&state, &script_runtime, bear);

    assert_eq!(effective.power, Some(8));
    assert_eq!(effective.toughness, Some(8));
}

// A scripted effect that resolves to a non-participating shape (AddKeyword) is not depended on and falls back to timestamp order.
#[test]
fn scripted_non_double_pt_shape_introduces_no_spurious_dependency() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let bear = ObjectId(0);
    put_on_battlefield(&mut state, creature(bear, p0, 2, 2));

    let mut script_runtime = ScriptRuntime::new();
    let handle = script_runtime
        .load_function("return function(source, controller) return { switch_pt = true } end")
        .unwrap();
    // A later timestamp than the modify. The assertion is only that this resolves without hanging or panicking and the modify still applies.
    state.continuous_effects.push(modify(bear, 1, 1, 0));
    state.continuous_effects.push(scripted(bear, handle, 1));

    let effective = effective_characteristics(&state, &script_runtime, bear);

    assert_eq!(effective.power, Some(3));
    assert_eq!(effective.toughness, Some(3));
}

// CR 613.8b
// Two DoublePTs depend on each other (a 2-cycle), so dependency is ignored and timestamp order applies. The point is that ordering does not hang on the cycle.
#[test]
fn two_mutually_dependent_double_pt_effects_fall_back_to_timestamp_order() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let bear = ObjectId(0);
    put_on_battlefield(&mut state, creature(bear, p0, 2, 2));
    state.continuous_effects.push(double(bear, true, true, 1));
    state.continuous_effects.push(double(bear, true, true, 0));

    let effective = effective_characteristics(&state, &ScriptRuntime::new(), bear);

    assert_eq!(effective.power, Some(8));
    assert_eq!(effective.toughness, Some(8));
}
