//! CR 108.4, 613.1: layer 2 control-changing effects. `layers::effective_controller` lets a later-timestamp `EffectPrimitive::GainControl(PlayerId)` replace the controller outright, like the 7b `SetPT` override.
//! Effects are built directly by struct literal. The gameplay consumers are in phase5_control_change_consumers.rs.

use std::collections::HashMap;

use stackfish_engine::layers::effective_controller;
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

fn gain_control(source: ObjectId, new_controller: PlayerId, filter: EffectFilter, timestamp: u64) -> ContinuousEffect {
    ContinuousEffect {
        source,
        layer: Layer::ControlChangingEffects,
        affected: AffectedFilter::Primitive(filter),
        effect: EffectPayload::Primitive(EffectPrimitive::GainControl(new_controller)),
        timestamp: SequenceNo(timestamp),
        duration: EffectDuration::Permanent,
        condition: None,
    }
}

#[test]
fn no_effect_returns_the_plain_controller_field() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let bear = ObjectId(0);
    put_on_battlefield(&mut state, creature(bear, p0, 2, 2));

    assert_eq!(effective_controller(&state, &ScriptRuntime::new(), bear), p0);
}

#[test]
fn a_gain_control_effect_overrides_the_base_controller() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let bear = ObjectId(0);
    put_on_battlefield(&mut state, creature(bear, p0, 2, 2));
    state.continuous_effects.push(gain_control(bear, p1, no_filter(), 0));

    assert_eq!(effective_controller(&state, &ScriptRuntime::new(), bear), p1);
}

// CR 613.7
// A later-timestamp GainControl overrides an earlier one outright and does not combine.
#[test]
fn later_timestamp_gain_control_wins_outright() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let bear = ObjectId(0);
    put_on_battlefield(&mut state, creature(bear, p0, 2, 2));
    state.continuous_effects.push(gain_control(bear, p1, no_filter(), 0));
    state.continuous_effects.push(gain_control(bear, p0, no_filter(), 1));

    assert_eq!(
        effective_controller(&state, &ScriptRuntime::new(), bear),
        p0,
        "the later-timestamp effect handing control back to p0 should win over the earlier grant to p1"
    );
}

#[test]
fn effect_filter_still_scopes_a_gain_control_effect() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let source = ObjectId(0);
    let own_creature = ObjectId(1);
    let opposing_creature = ObjectId(2);
    put_on_battlefield(&mut state, creature(source, p0, 0, 0));
    put_on_battlefield(&mut state, creature(own_creature, p0, 1, 1));
    put_on_battlefield(&mut state, creature(opposing_creature, p1, 1, 1));
    state.continuous_effects.push(gain_control(
        source,
        p0,
        EffectFilter {  controller: Some(FilterController::Opponent), card_types: None, self_only: false, subtypes: None, excluded_subtypes: None, supertypes: None, excluded_supertypes: None, },
        0,
    ));

    assert_eq!(
        effective_controller(&state, &ScriptRuntime::new(), opposing_creature),
        p0,
        "the opponent's creature should be under the effect's scope"
    );
    assert_eq!(
        effective_controller(&state, &ScriptRuntime::new(), own_creature),
        p0,
        "the caster's own creature is filtered out (Opponent-only) so it keeps its plain controller"
    );
}
