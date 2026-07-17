//! Phase 5 vertical slice: CR 613.6-613.8 dependency system (backlog item
//! 1.5a) — `layers::order_by_dependency` reorders a single layer's effects
//! around real CR 613.8a dependency instead of plain timestamp order.
//! `EffectPrimitive::DoublePT` (layer 7c, e.g. Berserk's "target creature's
//! power is doubled") is the one primitive combination that can actually
//! produce a dependency today — see that variant's own doc comment for why
//! no other primitive pairing can. Same "`ContinuousEffect`s built directly
//! via struct literal" bootstrap pattern as `phase5_layer_engine_switching.rs`
//! - no `Game.AddContinuousEffect` Lua binding yet.

use std::collections::{HashMap, VecDeque};

use stackfish_engine::layers::effective_characteristics;
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;

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
        effect: None,
        modes: Vec::new(),
    }
}

fn creature(id: ObjectId, owner: PlayerId, power: i32, toughness: i32) -> GameObject {
    GameObject {
        object_id: id,
        copiable_override: None,
        lineage: ObjectLineage { previous: None },
        definition: creature_definition(power, toughness),
        owner,
        controller: owner,
        zone: ZoneKind::Battlefield,
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
        is_commander: false,
        visibility: VisibilityPolicy { public: true, visible_to: Vec::new() },
    }
}

fn player_state(id: PlayerId, life: i32) -> PlayerState {
    PlayerState {
        player_id: id,
        life,
        mana_pool: ManaPool::default(),
        counters: CounterSet::new(),
        emblems: Vec::new(),
        commander_damage_taken: Vec::new(),
        eliminated: false,
        drew_from_empty_library: false,
        mulligans_taken: 0,
        hand_kept: false,
        commander_casts: std::collections::HashMap::new(),
    }
}

fn base_state() -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        players: vec![player_state(p0, 20), player_state(p1, 20)],
        turn_order: vec![p0, p1],
        active_player: p0,
        turn_number: 1,
        phase: Phase::Combat,
        step: Step::CombatDamage,
        priority_player: p0,
        consecutive_passes: 0,
        lands_played_this_turn: HashMap::new(),
        combat: None,
        last_active_turn_number: HashMap::from([(p0, 1)]),
        objects: HashMap::new(),
        zones: vec![
            Zone { kind: ZoneKind::Battlefield, owner: None, contents: VecDeque::new() },
            Zone { kind: ZoneKind::Graveyard, owner: Some(p0), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Graveyard, owner: Some(p1), contents: VecDeque::new() },
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
        once_per_turn_triggered: std::collections::HashSet::new(),
        commander_return_offered: std::collections::HashSet::new(),
    }
}

fn put_on_battlefield(state: &mut GameState, object: GameObject) {
    let id = object.object_id;
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(id);
    state.objects.insert(id, object);
}

fn no_filter() -> EffectFilter {
    EffectFilter { controller: None, card_types: None, self_only: false }
}

fn continuous_effect(source: ObjectId, effect: EffectPrimitive, timestamp: u64) -> ContinuousEffect {
    ContinuousEffect {
        source,
        layer: Layer::PowerToughnessModifying,
        affected: AffectedFilter::Primitive(no_filter()),
        effect: EffectPayload::Primitive(effect),
        timestamp: SequenceNo(timestamp),
        duration: EffectDuration::Permanent,
    }
}

fn modify(source: ObjectId, power: i32, toughness: i32, timestamp: u64) -> ContinuousEffect {
    continuous_effect(source, EffectPrimitive::ModifyPT { power, toughness }, timestamp)
}

fn double(source: ObjectId, power: bool, toughness: bool, timestamp: u64) -> ContinuousEffect {
    continuous_effect(source, EffectPrimitive::DoublePT { power, toughness }, timestamp)
}

/// Regression: two independent `ModifyPT`s (no dependency possible between
/// them) still just sum in the new ordering path, same as before this slice.
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

/// CR 613.8a/b: `DoublePT` always depends on a same-field `ModifyPT` in the
/// same sublayer, so the modify must apply first even though it has the
/// *later* timestamp - 2/2 base, +2/+2 (applied first, by dependency, not
/// timestamp) = 4/4, then doubled = 8/8.
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

/// Same scenario, timestamps the other way around (modify already earlier
/// than double) - dependency and timestamp agree here, same 8/8 result,
/// confirming the prior test wasn't just an accident of tie-breaking.
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

/// A `DoublePT{power: true, toughness: false}` only depends on/affects
/// power - toughness is untouched by either the dependency ordering or the
/// doubling itself.
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

/// CR 613.8b's last sentence: two `DoublePT`s mutually depend on each other
/// (a 2-cycle) - dependency is ignored for them, falling back to timestamp
/// order. Doubling is commutative, so both orders give the same 2/2 -> 4/4
/// -> 8/8 result either way; the point of this test is that ordering
/// doesn't hang or panic on the cycle, not that the number differs.
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
