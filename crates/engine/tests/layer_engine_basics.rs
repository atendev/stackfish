//! CR 613: the layer engine for layer 6 (ability adding) and layers 7b and 7c (power/toughness set and modify), ordered by timestamp within a layer.
//! `ContinuousEffect`s are built directly by struct literal.

use std::collections::HashMap;

use stackfish_engine::layers::effective_characteristics;
use stackfish_engine::sba::{check_state_based_actions, SbaOutcome};
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{card_object, two_player_state as base_two_player_state, zones};

// No fixture registers a ReplacementEffect on a dying creature, so a CR 616.1 pause here would mean the fixture is wrong.
fn progressed(outcome: SbaOutcome) -> Vec<GameEvent> {
    match outcome {
        SbaOutcome::Progressed(events, _) => events,
        SbaOutcome::NeedsOrderingChoice { .. } => {
            panic!("fixture didn't expect a replacement ordering-choice pause")
        }
        SbaOutcome::NeedsLegendRuleChoice { .. } => {
            panic!("fixture didn't expect a legend-rule choice pause")
        }
        SbaOutcome::NeedsCommanderReturnChoice { .. } => {
            panic!("fixture didn't expect a commander-return choice pause")
        }
    }
}

fn creature_definition(power: i32, toughness: i32, keywords: KeywordSet) -> CardDefinition {
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
        keywords,
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn creature(id: ObjectId, owner: PlayerId, power: i32, toughness: i32) -> GameObject {
    card_object(id, creature_definition(power, toughness, KeywordSet::empty()), owner, ZoneKind::Battlefield)
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
    EffectFilter {  controller: None, card_types: None, self_only: false, subtypes: None, excluded_subtypes: None, supertypes: None, excluded_supertypes: None, }
}

#[test]
fn base_case_no_continuous_effects_mirrors_card_definition() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let bear = ObjectId(0);
    put_on_battlefield(&mut state, creature(bear, p0, 2, 2));

    let effective = effective_characteristics(&state, &ScriptRuntime::new(), bear);

    assert_eq!(effective.power, Some(2));
    assert_eq!(effective.toughness, Some(2));
    assert_eq!(effective.keywords, KeywordSet::empty());
}

#[test]
fn add_keyword_unions_into_effective_keywords() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let anthem = ObjectId(0);
    let bear = ObjectId(1);
    put_on_battlefield(&mut state, creature(anthem, p0, 0, 0));
    put_on_battlefield(&mut state, creature(bear, p0, 2, 2));
    state.continuous_effects.push(continuous_effect(
        anthem,
        Layer::AbilityAdding,
        EffectPrimitive::AddKeyword(KeywordSet::FLYING),
        no_filter(),
        0,
    ));

    let effective = effective_characteristics(&state, &ScriptRuntime::new(), bear);

    assert!(effective.keywords.contains(KeywordSet::FLYING));
}

#[test]
fn modify_pt_adds_on_top_of_base() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let anthem = ObjectId(0);
    let bear = ObjectId(1);
    put_on_battlefield(&mut state, creature(anthem, p0, 0, 0));
    put_on_battlefield(&mut state, creature(bear, p0, 2, 2));
    state.continuous_effects.push(continuous_effect(
        anthem,
        Layer::PowerToughnessModifying,
        EffectPrimitive::ModifyPT { power: 1, toughness: 1 },
        no_filter(),
        0,
    ));

    let effective = effective_characteristics(&state, &ScriptRuntime::new(), bear);

    assert_eq!(effective.power, Some(3));
    assert_eq!(effective.toughness, Some(3));
}

#[test]
fn set_pt_overrides_base_before_modify_pt_applies() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let setter = ObjectId(0);
    let modifier = ObjectId(1);
    let bear = ObjectId(2);
    put_on_battlefield(&mut state, creature(setter, p0, 0, 0));
    put_on_battlefield(&mut state, creature(modifier, p0, 0, 0));
    put_on_battlefield(&mut state, creature(bear, p0, 2, 2));
    // Layer 7b: base 2/2 becomes a flat 0/1 ...
    state.continuous_effects.push(continuous_effect(
        setter,
        Layer::PowerToughnessSetting,
        EffectPrimitive::SetPT { power: 0, toughness: 1 },
        no_filter(),
        0,
    ));
    // ... then layer 7c adds +2/+2 on top of the set value, since 7b runs before 7c regardless of timestamp.
    state.continuous_effects.push(continuous_effect(
        modifier,
        Layer::PowerToughnessModifying,
        EffectPrimitive::ModifyPT { power: 2, toughness: 2 },
        no_filter(),
        0,
    ));

    let effective = effective_characteristics(&state, &ScriptRuntime::new(), bear);

    assert_eq!(effective.power, Some(2));
    assert_eq!(effective.toughness, Some(3));
}

#[test]
fn later_timestamp_set_pt_wins_within_the_same_layer() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let first = ObjectId(0);
    let second = ObjectId(1);
    let bear = ObjectId(2);
    put_on_battlefield(&mut state, creature(first, p0, 0, 0));
    put_on_battlefield(&mut state, creature(second, p0, 0, 0));
    put_on_battlefield(&mut state, creature(bear, p0, 2, 2));
    state.continuous_effects.push(continuous_effect(
        first,
        Layer::PowerToughnessSetting,
        EffectPrimitive::SetPT { power: 4, toughness: 4 },
        no_filter(),
        5,
    ));
    state.continuous_effects.push(continuous_effect(
        second,
        Layer::PowerToughnessSetting,
        EffectPrimitive::SetPT { power: 1, toughness: 1 },
        no_filter(),
        10,
    ));

    let effective = effective_characteristics(&state, &ScriptRuntime::new(), bear);

    // Timestamp 10 (second) landed after timestamp 5 (first) - it wins.
    assert_eq!(effective.power, Some(1));
    assert_eq!(effective.toughness, Some(1));
}

#[test]
fn effect_filter_only_affects_matching_objects() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let anthem = ObjectId(0);
    let own_bear = ObjectId(1);
    let opposing_bear = ObjectId(2);
    put_on_battlefield(&mut state, creature(anthem, p0, 0, 0));
    put_on_battlefield(&mut state, creature(own_bear, p0, 2, 2));
    put_on_battlefield(&mut state, creature(opposing_bear, p1, 2, 2));
    state.continuous_effects.push(continuous_effect(
        anthem,
        Layer::PowerToughnessModifying,
        EffectPrimitive::ModifyPT { power: 1, toughness: 1 },
        EffectFilter {  controller: Some(FilterController::You), card_types: None, self_only: false, subtypes: None, excluded_subtypes: None, supertypes: None, excluded_supertypes: None, },
        0,
    ));

    let own = effective_characteristics(&state, &ScriptRuntime::new(), own_bear);
    let opposing = effective_characteristics(&state, &ScriptRuntime::new(), opposing_bear);

    assert_eq!(own.power, Some(3), "creatures you control should be boosted");
    assert_eq!(opposing.power, Some(2), "an opponent's creature should be untouched");
}

#[test]
fn self_only_filter_excludes_other_objects() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let equipment = ObjectId(0);
    let equipped = ObjectId(1);
    let bystander = ObjectId(2);
    put_on_battlefield(&mut state, creature(equipment, p0, 0, 0));
    put_on_battlefield(&mut state, creature(equipped, p0, 2, 2));
    put_on_battlefield(&mut state, creature(bystander, p0, 2, 2));
    // self_only models "equipped creature" as the effect's own source.
    state.continuous_effects.push(continuous_effect(
        equipment,
        Layer::PowerToughnessModifying,
        EffectPrimitive::ModifyPT { power: 3, toughness: 0 },
        EffectFilter {  controller: None, card_types: None, self_only: true, subtypes: None, excluded_subtypes: None, supertypes: None, excluded_supertypes: None, },
        0,
    ));

    let source_itself = effective_characteristics(&state, &ScriptRuntime::new(), equipment);
    let other = effective_characteristics(&state, &ScriptRuntime::new(), bystander);

    assert_eq!(source_itself.power, Some(3));
    assert_eq!(other.power, Some(2));
}

#[test]
fn cda_power_and_toughness_are_still_skipped_not_guessed() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let star = ObjectId(0);
    let mut object = creature(star, p0, 0, 0);
    object.definition.power =
        Some(PTValue::CharacteristicDefining { display: "*".to_string(), script: None });
    object.definition.toughness =
        Some(PTValue::CharacteristicDefining { display: "*".to_string(), script: None });
    put_on_battlefield(&mut state, object);

    let effective = effective_characteristics(&state, &ScriptRuntime::new(), star);

    assert_eq!(effective.power, None);
    assert_eq!(effective.toughness, None);
}

#[test]
fn a_modify_pt_boost_saves_a_creature_from_the_lethal_toughness_sba() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let anthem = ObjectId(0);
    let bear = ObjectId(1);
    put_on_battlefield(&mut state, anthem_source(anthem, p0));
    let mut daged_bear = creature(bear, p0, 2, 2);
    daged_bear.damage_marked = 2;
    put_on_battlefield(&mut state, daged_bear);

    // Without the boost, 2 damage on a 2-toughness creature is lethal.
    let events = progressed(check_state_based_actions(&mut state, &ScriptRuntime::new()));
    assert_eq!(events.len(), 1, "should have died without the boost");
    assert!(state.object(bear).is_none());
}

#[test]
fn with_the_boost_registered_the_same_creature_survives_sba() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let anthem = ObjectId(0);
    let bear = ObjectId(1);
    put_on_battlefield(&mut state, anthem_source(anthem, p0));
    state.continuous_effects.push(continuous_effect(
        anthem,
        Layer::PowerToughnessModifying,
        EffectPrimitive::ModifyPT { power: 1, toughness: 1 },
        no_filter(),
        0,
    ));
    let mut damaged_bear = creature(bear, p0, 2, 2);
    damaged_bear.damage_marked = 2;
    put_on_battlefield(&mut state, damaged_bear);

    let events = progressed(check_state_based_actions(&mut state, &ScriptRuntime::new()));

    assert!(events.is_empty(), "the +1/+1 anthem should make 2 damage sub-lethal on an effective 3 toughness");
    assert!(state.object(bear).is_some());
}

#[test]
fn a_granted_indestructible_survives_lethal_damage_via_sba() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let source = ObjectId(0);
    let bear = ObjectId(1);
    put_on_battlefield(&mut state, anthem_source(source, p0));
    state.continuous_effects.push(continuous_effect(
        source,
        Layer::AbilityAdding,
        EffectPrimitive::AddKeyword(KeywordSet::INDESTRUCTIBLE),
        no_filter(),
        0,
    ));
    let mut damaged_bear = creature(bear, p0, 2, 2);
    damaged_bear.damage_marked = 5;
    put_on_battlefield(&mut state, damaged_bear);

    let events = progressed(check_state_based_actions(&mut state, &ScriptRuntime::new()));

    assert!(events.is_empty(), "granted Indestructible should ignore 704.5g lethal damage");
    assert!(state.object(bear).is_some());
}

// A non-creature source, so apply_creature_death's CR 704.5f/g checks never apply to it and confuse a test about the bear.
fn anthem_source(id: ObjectId, owner: PlayerId) -> GameObject {
    let mut object = creature(id, owner, 0, 0);
    object.definition.card_types = CardTypeSet::ENCHANTMENT;
    object.definition.power = None;
    object.definition.toughness = None;
    object
}
