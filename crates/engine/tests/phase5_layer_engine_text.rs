//! Phase 5 vertical slice: CR 613.1c/612 layer 3 (text-changing effects) -
//! backlog item 1's final sub-slice (5d), narrowed to CR 612.8 name-setting
//! only per that item's own pre-existing recommendation (the general CR
//! 612.2 "correct usage" word-substitution case, and the CR 612.5-612.10
//! single-card rules, stay unmodeled - see `Layer::TextChanging`'s doc
//! comment). `EffectPrimitive::SetName(String)` mirrors `GainControl`/`SetPT`'s
//! override-wins-outright semantics exactly (a later-timestamp `SetName`
//! replaces an earlier one's result, it doesn't combine with it).
//!
//! `sba.rs`'s legend-rule grouping (`find_legend_rule_group`) already keys
//! off `effective_characteristics(...).name`, not the raw `CardDefinition`
//! field - a real gameplay consumer for free, same "turned out to need no
//! new plumbing" finding as the layer-engine-consumers sub-slice.

use std::collections::HashMap;

use stackfish_engine::layers::effective_characteristics;
use stackfish_engine::sba::{check_state_based_actions, SbaOutcome};
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{card_object, two_player_state as base_two_player_state, zones};

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

fn permanent_definition(name: &str, supertypes: SupertypeSet, card_types: CardTypeSet) -> CardDefinition {
    CardDefinition {
        definition_id: 1,
        name: name.to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes,
        card_types,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: if card_types.contains(CardTypeSet::CREATURE) { Some(PTValue::Fixed(2)) } else { None },
        toughness: if card_types.contains(CardTypeSet::CREATURE) { Some(PTValue::Fixed(2)) } else { None },
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_layer_engine_text".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn permanent(id: ObjectId, owner: PlayerId, name: &str, supertypes: SupertypeSet, timestamp: u64) -> GameObject {
    GameObject {
        timestamp: SequenceNo(timestamp),
        ..card_object(id, permanent_definition(name, supertypes, CardTypeSet::CREATURE), owner, ZoneKind::Battlefield)
    }
}

fn no_filter() -> EffectFilter {
    EffectFilter {  controller: None, card_types: None, self_only: false, subtypes: None, excluded_subtypes: None, supertypes: None, excluded_supertypes: None, }
}

fn set_name(source: ObjectId, new_name: &str, filter: EffectFilter, timestamp: u64) -> ContinuousEffect {
    ContinuousEffect {
        source,
        layer: Layer::TextChanging,
        affected: AffectedFilter::Primitive(filter),
        effect: EffectPayload::Primitive(EffectPrimitive::SetName(new_name.to_string())),
        timestamp: SequenceNo(timestamp),
        duration: EffectDuration::Permanent,
        condition: None,
    }
}

#[test]
fn no_effect_returns_the_plain_name() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let bear = ObjectId(0);
    put_on_battlefield(&mut state, permanent(bear, p0, "Test Bear", SupertypeSet::empty(), 0));

    assert_eq!(effective_characteristics(&state, &ScriptRuntime::new(), bear).name, "Test Bear");
}

#[test]
fn a_set_name_effect_overrides_the_base_name() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let bear = ObjectId(0);
    put_on_battlefield(&mut state, permanent(bear, p0, "Test Bear", SupertypeSet::empty(), 0));
    state.continuous_effects.push(set_name(bear, "Renamed Bear", no_filter(), 1));

    assert_eq!(effective_characteristics(&state, &ScriptRuntime::new(), bear).name, "Renamed Bear");
}

/// CR 612.8/613.7: a later-timestamp `SetName` overrides an earlier one
/// outright - not combined, same as `GainControl`/`SetPT`'s own override
/// semantics.
#[test]
fn later_timestamp_set_name_wins_outright() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let bear = ObjectId(0);
    put_on_battlefield(&mut state, permanent(bear, p0, "Test Bear", SupertypeSet::empty(), 0));
    state.continuous_effects.push(set_name(bear, "First Rename", no_filter(), 1));
    state.continuous_effects.push(set_name(bear, "Second Rename", no_filter(), 2));

    assert_eq!(
        effective_characteristics(&state, &ScriptRuntime::new(), bear).name,
        "Second Rename",
        "the later-timestamp SetName should win over the earlier one, not combine with it"
    );
}

#[test]
fn effect_filter_still_scopes_a_set_name_effect() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let source = ObjectId(0);
    let other = ObjectId(1);
    put_on_battlefield(&mut state, permanent(source, p0, "Source", SupertypeSet::empty(), 0));
    put_on_battlefield(&mut state, permanent(other, p0, "Test Bear", SupertypeSet::empty(), 0));
    state.continuous_effects.push(set_name(
        source,
        "Renamed",
        EffectFilter {  controller: None, card_types: None, self_only: true, subtypes: None, excluded_subtypes: None, supertypes: None, excluded_supertypes: None, },
        1,
    ));

    assert_eq!(
        effective_characteristics(&state, &ScriptRuntime::new(), source).name,
        "Renamed",
        "self_only should still scope the effect onto its own source"
    );
    assert_eq!(
        effective_characteristics(&state, &ScriptRuntime::new(), other).name,
        "Test Bear",
        "self_only should keep an unrelated object out of the effect's scope"
    );
}

// ---------------------------------------------------------------------
// Real consumer: sba.rs's legend-rule grouping already reads the
// effective name, not the raw CardDefinition field.
// ---------------------------------------------------------------------

#[test]
fn renaming_a_legendary_to_match_another_triggers_the_legend_rule() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let a = ObjectId(0);
    let b = ObjectId(1);
    // Two differently-named legendaries under the same controller - no
    // legend rule violation yet.
    put_on_battlefield(&mut state, permanent(a, p0, "Legend A", SupertypeSet::LEGENDARY, 0));
    put_on_battlefield(&mut state, permanent(b, p0, "Legend B", SupertypeSet::LEGENDARY, 1));

    assert!(
        matches!(
            check_state_based_actions(&mut state, &ScriptRuntime::new()),
            SbaOutcome::Progressed(events, _) if events.is_empty()
        ),
        "different names shouldn't violate the legend rule before the rename"
    );

    // A SetName effect makes `a`'s effective name match `b`'s printed name.
    state.continuous_effects.push(set_name(a, "Legend B", no_filter(), 2));

    let outcome = check_state_based_actions(&mut state, &ScriptRuntime::new());
    let SbaOutcome::NeedsLegendRuleChoice { player, candidates, .. } = outcome else {
        panic!("renaming one legendary to match another's name must trigger CR 704.5j")
    };
    assert_eq!(player, p0);
    assert_eq!(candidates, vec![a, b]);
}
