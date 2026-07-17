//! Phase 5 vertical slice: CR 613 continuous-effect layer engine, layer 7e
//! (`PowerToughnessSwitching`, CR 613.4d) — the last of backlog item 1's
//! real-P/T sublayers (7a CDA, 7b set, 7c modify, 7e switch; 7d/counters
//! stays the pre-existing base-level simplification, untouched). Same
//! "`ContinuousEffect`s built directly via struct literal" bootstrap pattern
//! as `phase5_layer_engine_types_colors.rs` — no `Game.AddContinuousEffect`
//! Lua binding yet.

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
    EffectFilter { controller: None, card_types: None, self_only: false }
}

fn switch_effect(source: ObjectId, timestamp: u64) -> ContinuousEffect {
    continuous_effect(source, Layer::PowerToughnessSwitching, EffectPrimitive::SwitchPT, no_filter(), timestamp)
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

/// CR 613.4d's own first worked example: a 1/3 creature gets +0/+1 (layer
/// 7c), then an effect switches its power and toughness (layer 7e) - new P/T
/// is 4/1. A later effect gives it +5/+0 (also layer 7c, but a *later*
/// timestamp than the switch). Its "unswitched" P/T would be 6/4, so its
/// actual P/T is 4/6 - the +5/+0 still lands in the pre-switch sum, because
/// 7c fully resolves as one pass before 7e's swap ever applies, regardless of
/// the modify effect's timestamp relative to the switch.
#[test]
fn a_later_modify_effect_still_contributes_to_the_pre_switch_sum() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let bear = ObjectId(0);
    put_on_battlefield(&mut state, creature(bear, p0, 1, 3));
    state.continuous_effects.push(continuous_effect(
        bear,
        Layer::PowerToughnessModifying,
        EffectPrimitive::ModifyPT { power: 0, toughness: 1 },
        no_filter(),
        0,
    ));
    state.continuous_effects.push(switch_effect(bear, 1));
    state.continuous_effects.push(continuous_effect(
        bear,
        Layer::PowerToughnessModifying,
        EffectPrimitive::ModifyPT { power: 5, toughness: 0 },
        no_filter(),
        2,
    ));

    let effective = effective_characteristics(&state, &ScriptRuntime::new(), bear);

    assert_eq!(effective.power, Some(4));
    assert_eq!(effective.toughness, Some(6));
}

/// CR 613.4d's own third worked example: two switch effects on the same
/// object cancel out exactly, leaving the unswitched value.
#[test]
fn two_switch_effects_on_the_same_object_cancel_out() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let bear = ObjectId(0);
    put_on_battlefield(&mut state, creature(bear, p0, 1, 3));
    state.continuous_effects.push(continuous_effect(
        bear,
        Layer::PowerToughnessModifying,
        EffectPrimitive::ModifyPT { power: 0, toughness: 1 },
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
        EffectFilter { controller: Some(FilterController::You), card_types: None, self_only: false },
        0,
    ));

    let own = effective_characteristics(&state, &ScriptRuntime::new(), own_creature);
    let opposing = effective_characteristics(&state, &ScriptRuntime::new(), opposing_creature);

    assert_eq!((own.power, own.toughness), (Some(3), Some(1)), "controller's own creature should be switched");
    assert_eq!((opposing.power, opposing.toughness), (Some(1), Some(3)), "an opponent's creature should be untouched");
}

#[test]
fn a_switch_effect_is_a_noop_on_an_object_with_no_resolvable_power_or_toughness() {
    // A noncreature permanent with no printed P/T and no SetPT applying to it
    // - power/toughness are both None at this point in the query, so the
    // switch has nothing to swap and must not panic or fabricate a value.
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
