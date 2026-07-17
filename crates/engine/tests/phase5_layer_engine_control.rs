//! Phase 5 vertical slice: CR 108.4/613.1 layer 2 (control-changing effects)
//! - backlog item 1.5b, query-only half. `layers::effective_controller`
//! mirrors `effective_characteristics`'s own 7b (`SetPT`) override loop:
//! `EffectPrimitive::GainControl(PlayerId)` replaces the object's controller
//! outright, later timestamp winning over an earlier one, not additive.
//! Deliberately not wired into any real gameplay consumer yet (see
//! `Layer`'s own doc comment) - the ~15 real `GameObject.controller`
//! read-sites (combat, targeting, cost filters, `has_summoning_sickness`,
//! `EffectFilter`/`FilterSpec`) still read the raw field, untouched by this
//! slice. Same "`ContinuousEffect`s built directly via struct literal"
//! bootstrap pattern as `phase5_layer_engine_switching.rs` - no
//! `Game.AddContinuousEffect`/`Game.Ops.GainControl` Lua binding yet.

use std::collections::{HashMap, VecDeque};

use stackfish_engine::layers::effective_controller;
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

fn gain_control(source: ObjectId, new_controller: PlayerId, filter: EffectFilter, timestamp: u64) -> ContinuousEffect {
    ContinuousEffect {
        source,
        layer: Layer::ControlChangingEffects,
        affected: AffectedFilter::Primitive(filter),
        effect: EffectPayload::Primitive(EffectPrimitive::GainControl(new_controller)),
        timestamp: SequenceNo(timestamp),
        duration: EffectDuration::Permanent,
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

/// CR 613.7: a later-timestamp `GainControl` overrides an earlier one
/// outright - not additive/combined, same as `SetPT`'s own override
/// semantics for layer 7b.
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
        EffectFilter { controller: Some(FilterController::Opponent), card_types: None, self_only: false },
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
