//! Phase 5 vertical slice: CR 613 continuous-effect layer engine, sub-slice
//! C — layer 7a (characteristic-defining power/toughness, CR 604.3/208.2a).
//! `PTValue::CharacteristicDefining` gains a `script: Option<ScriptReference>`
//! field (was display-text-only before this slice); `layers.rs`'s
//! `resolve_base_pt` invokes it (`ScriptRuntime::invoke_cda`) before layers
//! 7b/7c apply, same CR 613.4a→b→c ordering as everywhere else in this
//! engine's layer system.
//!
//! No `Game.*` state-query bindings exist yet (checked: neither
//! `invoke_condition` nor `invoke_operation` passes `GameState` into Lua
//! either) — a real "count cards in your graveyard"-style CDA isn't
//! buildable until that lands (docs/implementation.md's slice backlog,
//! script API expansion). Every CDA script below is a constant or
//! id-derived, same limitation `invoke_condition`'s own existing tests
//! already live with.

use std::collections::{HashMap, VecDeque};

use stackfish_engine::layers::effective_characteristics;
use stackfish_engine::sba::{check_state_based_actions, SbaOutcome};
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;

/// This slice's fixtures never register a `ReplacementEffect` on a dying
/// creature - a real CR 616.1 pause here would mean the fixture itself is
/// wrong, not something to handle gracefully.
fn progressed(outcome: SbaOutcome) -> Vec<GameEvent> {
    match outcome {
        SbaOutcome::Progressed(events) => events,
        SbaOutcome::NeedsOrderingChoice { .. } => {
            panic!("fixture didn't expect a replacement ordering-choice pause")
        }
    }
}

/// `script: None` means no CDA callback was ever supplied — `layers.rs`
/// resolves this exactly like `PTValue::as_fixed` already did (`None`, skip
/// rather than guess).
fn cda_value(script: Option<&str>) -> PTValue {
    PTValue::CharacteristicDefining {
        display: "*".to_string(),
        script: script.map(|src| ScriptReference::Inline(src.to_string())),
    }
}

fn cda_creature_definition(power_script: Option<&str>, toughness_script: Option<&str>) -> CardDefinition {
    CardDefinition {
        definition_id: 1,
        name: "Test CDA Creature".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::CREATURE,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: Some(cda_value(power_script)),
        toughness: Some(cda_value(toughness_script)),
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_cda_creature".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
    }
}

fn cda_creature(id: ObjectId, owner: PlayerId, power_script: Option<&str>, toughness_script: Option<&str>) -> GameObject {
    GameObject {
        object_id: id,
        lineage: ObjectLineage { previous: None },
        definition: cda_creature_definition(power_script, toughness_script),
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
        visibility: VisibilityPolicy { public: true, visible_to: Vec::new() },
    }
}

fn creature_definition(power: i32, toughness: i32) -> CardDefinition {
    CardDefinition {
        definition_id: 2,
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
    }
}

fn creature(id: ObjectId, owner: PlayerId, power: i32, toughness: i32) -> GameObject {
    GameObject {
        object_id: id,
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
        mulligans_taken: 0,
        hand_kept: false,
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
    }
}

fn no_filter() -> EffectFilter {
    EffectFilter { controller: None, card_types: None, self_only: false }
}

const RETURNS_THREE: &str = "return function(source, controller) return 3 end";
const RETURNS_WRONG_TYPE: &str = "return function(source, controller) return \"not a number\" end";

#[test]
fn no_script_still_resolves_to_none() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let star = ObjectId(0);
    put_on_battlefield(&mut state, cda_creature(star, p0, None, None));

    let script_runtime = ScriptRuntime::new();
    let effective = effective_characteristics(&state, &script_runtime, star);

    assert_eq!(effective.power, None, "no script - can't resolve, skip rather than guess");
    assert_eq!(effective.toughness, None);
}

#[test]
fn a_constant_returning_script_resolves_to_a_real_number() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let star = ObjectId(0);
    put_on_battlefield(&mut state, cda_creature(star, p0, Some(RETURNS_THREE), Some(RETURNS_THREE)));

    let script_runtime = ScriptRuntime::new();
    let effective = effective_characteristics(&state, &script_runtime, star);

    assert_eq!(effective.power, Some(3));
    assert_eq!(effective.toughness, Some(3));
}

#[test]
fn cda_base_is_overridden_by_a_later_set_pt() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let star = ObjectId(0);
    let setter = ObjectId(1);
    put_on_battlefield(&mut state, cda_creature(star, p0, Some(RETURNS_THREE), Some(RETURNS_THREE)));
    put_on_battlefield(&mut state, creature(setter, p0, 0, 0));
    // CR 613.4a->b: layer 7a resolves the CDA base (3/3) first, then layer
    // 7b's SetPT still overrides it entirely - the 3/3 base never "wins" by
    // virtue of resolving first.
    state.continuous_effects.push(continuous_effect(
        setter,
        Layer::PowerToughnessSetting,
        EffectPrimitive::SetPT { power: 5, toughness: 5 },
        no_filter(),
        0,
    ));

    let script_runtime = ScriptRuntime::new();
    let effective = effective_characteristics(&state, &script_runtime, star);

    assert_eq!(effective.power, Some(5), "SetPT should override the CDA base, not stack with it");
    assert_eq!(effective.toughness, Some(5));
}

#[test]
fn cda_base_has_modify_pt_stacked_on_top() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let star = ObjectId(0);
    let anthem = ObjectId(1);
    put_on_battlefield(&mut state, cda_creature(star, p0, Some(RETURNS_THREE), Some(RETURNS_THREE)));
    put_on_battlefield(&mut state, creature(anthem, p0, 0, 0));
    // CR 613.4a->c: layer 7c is additive on top of whatever 7a/7b produced -
    // the CDA-resolved 3/3 base plus a +2/+2 anthem should be 5/5, not just
    // the anthem's +2/+2 alone.
    state.continuous_effects.push(continuous_effect(
        anthem,
        Layer::PowerToughnessModifying,
        EffectPrimitive::ModifyPT { power: 2, toughness: 2 },
        no_filter(),
        0,
    ));

    let script_runtime = ScriptRuntime::new();
    let effective = effective_characteristics(&state, &script_runtime, star);

    assert_eq!(effective.power, Some(5));
    assert_eq!(effective.toughness, Some(5));
}

#[test]
fn cda_base_nets_against_plus_one_counters() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let star = ObjectId(0);
    let mut object = cda_creature(star, p0, Some(RETURNS_THREE), Some(RETURNS_THREE));
    object.counters.add(CounterKind::PlusOnePlusOne, 2);
    put_on_battlefield(&mut state, object);

    let script_runtime = ScriptRuntime::new();
    let effective = effective_characteristics(&state, &script_runtime, star);

    assert_eq!(effective.power, Some(5), "CR 121.3: +1/+1 counters net against a CDA base too, not just Fixed");
    assert_eq!(effective.toughness, Some(5));
}

#[test]
fn an_erroring_script_resolves_to_none_not_a_panic() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let star = ObjectId(0);
    put_on_battlefield(&mut state, cda_creature(star, p0, Some(RETURNS_WRONG_TYPE), Some(RETURNS_WRONG_TYPE)));

    let script_runtime = ScriptRuntime::new();
    let effective = effective_characteristics(&state, &script_runtime, star);

    assert_eq!(effective.power, None, "a script that errors should skip, not panic or guess");
    assert_eq!(effective.toughness, None);
}

// ---------------------------------------------------------------------
// Real consumer: sba::check_state_based_actions's existing lethal-toughness
// check (real since layer-engine-basics) now actually resolves a scripted
// CDA creature's toughness instead of always getting None for it.
// ---------------------------------------------------------------------

#[test]
fn a_script_less_cda_creature_never_dies_to_the_lethal_damage_sba() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let star = ObjectId(0);
    let mut object = cda_creature(star, p0, None, None);
    object.damage_marked = 999;
    put_on_battlefield(&mut state, object);

    let script_runtime = ScriptRuntime::new();
    let events = progressed(check_state_based_actions(&mut state, &script_runtime));

    assert!(events.is_empty(), "toughness is unresolved (None) - 704.5g can't apply without a number");
    assert!(state.object(star).is_some());
}

#[test]
fn a_scripted_cda_creature_dies_to_lethal_damage_via_sba() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let star = ObjectId(0);
    // toughness resolves to 3 via RETURNS_THREE; 3 marked damage is lethal.
    let mut object = cda_creature(star, p0, None, Some(RETURNS_THREE));
    object.damage_marked = 3;
    put_on_battlefield(&mut state, object);

    let script_runtime = ScriptRuntime::new();
    let events = progressed(check_state_based_actions(&mut state, &script_runtime));

    assert_eq!(events.len(), 1, "a real resolved toughness should make 704.5g apply");
    assert!(state.object(star).is_none());
}
