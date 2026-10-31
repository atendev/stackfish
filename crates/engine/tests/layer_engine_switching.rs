//! CR 613.4d: layer 7e power/toughness switching, after 7a CDA, 7b set and 7c modify. Effects are built directly by struct literal.

use std::collections::HashMap;

use stackfish_engine::layers::effective_characteristics;
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{card_object, two_player_state as base_two_player_state, zones};

fn creature_definition(power: i32, toughness: i32) -> CardDefinition {
    CardDefinition {
        definition_id: 1,
        name: "Test Creature".to_string(),
        mana_cost: ManaCost {
            symbols: Vec::new(),
        },
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
    card_object(
        id,
        creature_definition(power, toughness),
        owner,
        ZoneKind::Battlefield,
    )
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
    state
        .zone_mut(ZoneKind::Battlefield, None)
        .unwrap()
        .push_bottom(id);
    state.objects.insert(id, object);
}

fn continuous_effect(
    source: ObjectId,
    layer: Layer,
    effect: EffectPrimitive,
    filter: EffectFilter,
    timestamp: u64,
) -> ContinuousEffect {
    ContinuousEffect {
        source,
        layer,
        affected: AffectedFilter::Primitive(filter),
        effect: EffectPayload::Primitive(effect),
        timestamp: SequenceNo(timestamp),
        duration: EffectDuration::Permanent,
        condition: None,
    }
}

fn no_filter() -> EffectFilter {
    EffectFilter {
        controller: None,
        card_types: None,
        self_only: false,
        subtypes: None,
        excluded_subtypes: None,
        supertypes: None,
        excluded_supertypes: None,
    }
}

fn switch_effect(source: ObjectId, timestamp: u64) -> ContinuousEffect {
    continuous_effect(
        source,
        Layer::PowerToughnessSwitching,
        EffectPrimitive::SwitchPT,
        no_filter(),
        timestamp,
    )
}

#[test]
fn a_single_switch_swaps_power_and_toughness() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let bear = ObjectId(0);
    put_on_battlefield(&mut state, creature(bear, p0, 1, 3));
    state.continuous_effects.push(switch_effect(bear, 0));

    let effective = effective_characteristics(&state, &ScriptRuntime::new(), bear);

    assert_eq!(effective.power, Some(3));
    assert_eq!(effective.toughness, Some(1));
}

// CR 613.4d
// A 1/3 gets +0/+1 (7c), then a switch (7e), then a later +5/+0 (7c). Its unswitched P/T is 6/4, so it ends 4/6, because 7c resolves as one pass before 7e swaps.
#[test]
fn a_later_modify_effect_still_contributes_to_the_pre_switch_sum() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let bear = ObjectId(0);
    put_on_battlefield(&mut state, creature(bear, p0, 1, 3));
    state.continuous_effects.push(continuous_effect(
        bear,
        Layer::PowerToughnessModifying,
        EffectPrimitive::ModifyPT {
            power: 0,
            toughness: 1,
        },
        no_filter(),
        0,
    ));
    state.continuous_effects.push(switch_effect(bear, 1));
    state.continuous_effects.push(continuous_effect(
        bear,
        Layer::PowerToughnessModifying,
        EffectPrimitive::ModifyPT {
            power: 5,
            toughness: 0,
        },
        no_filter(),
        2,
    ));

    let effective = effective_characteristics(&state, &ScriptRuntime::new(), bear);

    assert_eq!(effective.power, Some(4));
    assert_eq!(effective.toughness, Some(6));
}

// CR 613.4d
// Two switch effects on one object cancel out.
#[test]
fn two_switch_effects_on_the_same_object_cancel_out() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let bear = ObjectId(0);
    put_on_battlefield(&mut state, creature(bear, p0, 1, 3));
    state.continuous_effects.push(continuous_effect(
        bear,
        Layer::PowerToughnessModifying,
        EffectPrimitive::ModifyPT {
            power: 0,
            toughness: 1,
        },
        no_filter(),
        0,
    ));
    state.continuous_effects.push(switch_effect(bear, 1));
    state.continuous_effects.push(switch_effect(bear, 2));

    let effective = effective_characteristics(&state, &ScriptRuntime::new(), bear);

    assert_eq!(effective.power, Some(1));
    assert_eq!(effective.toughness, Some(4));
}

#[test]
fn effect_filter_still_scopes_a_switch_effect() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let source = ObjectId(0);
    let own_creature = ObjectId(1);
    let opposing_creature = ObjectId(2);
    put_on_battlefield(&mut state, creature(source, p0, 0, 0));
    put_on_battlefield(&mut state, creature(own_creature, p0, 1, 3));
    put_on_battlefield(&mut state, creature(opposing_creature, p1, 1, 3));
    state.continuous_effects.push(continuous_effect(
        source,
        Layer::PowerToughnessSwitching,
        EffectPrimitive::SwitchPT,
        EffectFilter {
            controller: Some(FilterController::You),
            card_types: None,
            self_only: false,
            subtypes: None,
            excluded_subtypes: None,
            supertypes: None,
            excluded_supertypes: None,
        },
        0,
    ));

    let own = effective_characteristics(&state, &ScriptRuntime::new(), own_creature);
    let opposing = effective_characteristics(&state, &ScriptRuntime::new(), opposing_creature);

    assert_eq!(
        (own.power, own.toughness),
        (Some(3), Some(1)),
        "controller's own creature should be switched"
    );
    assert_eq!(
        (opposing.power, opposing.toughness),
        (Some(1), Some(3)),
        "an opponent's creature should be untouched"
    );
}

#[test]
fn a_switch_effect_is_a_noop_on_an_object_with_no_resolvable_power_or_toughness() {
    // A noncreature permanent with no P/T gives the switch nothing to swap, so it must not panic or fabricate a value.
    let mut state = base_state();
    let p0 = PlayerId(0);
    let mut artifact = creature(ObjectId(0), p0, 0, 0);
    artifact.definition.power = None;
    artifact.definition.toughness = None;
    artifact.definition.card_types = CardTypeSet::ARTIFACT;
    let artifact_id = artifact.object_id;
    put_on_battlefield(&mut state, artifact);
    state.continuous_effects.push(switch_effect(artifact_id, 0));

    let effective = effective_characteristics(&state, &ScriptRuntime::new(), artifact_id);

    assert_eq!(effective.power, None);
    assert_eq!(effective.toughness, None);
}
