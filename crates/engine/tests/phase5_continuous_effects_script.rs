//! Phase 5 vertical slice: backlog item 3a, script-backed continuous
//! effects. `AffectedFilter::Script`/`EffectPayload::Script` (dead code since
//! the layer-engine-cda slice split them out as their own follow-up) are now
//! real: `layers::applicable_effects_in_layer` evaluates a `Script` affects-
//! predicate via `ScriptRuntime::invoke_continuous_affects`
//! (`function(object, source, controller) -> bool`), and
//! `layers::resolve_primitive` decodes a `Script` effect payload via
//! `ScriptRuntime::invoke_continuous_effect` (`function(source, controller)
//! -> table`, the same "Lua asks, Rust decides" one-key-per-shape convention
//! `invoke_operation` already established, generalized to `set_pt`/
//! `modify_pt`/`add_keyword`/`add_card_type`/`add_color`/`switch_pt`).
//!
//! No `Game.*` state-query bindings exist yet (same limitation the CDA
//! slice's tests already live with) - every script below decides purely from
//! `object`/`source`/`controller` identity or a constant, never real board
//! state.

use std::collections::{HashMap, VecDeque};

use stackfish_engine::layers::effective_characteristics;
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;

fn creature_definition(power: i32, toughness: i32, script_ref: ScriptReference) -> CardDefinition {
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
        script_ref,
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
        lineage: ObjectLineage { previous: None },
        definition: creature_definition(power, toughness, ScriptReference::ByKey("vanilla".to_string())),
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
        visibility: VisibilityPolicy { public: true, visible_to: Vec::new() },
    }
}

fn scripted_creature(id: ObjectId, owner: PlayerId, source: &str) -> GameObject {
    GameObject {
        object_id: id,
        lineage: ObjectLineage { previous: None },
        definition: creature_definition(2, 2, ScriptReference::Inline(source.to_string())),
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
        phase: Phase::Main,
        step: Step::PreCombatMain,
        priority_player: p0,
        consecutive_passes: 0,
        lands_played_this_turn: HashMap::new(),
        combat: None,
        last_active_turn_number: HashMap::new(),
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
    }
}

fn card_object(id: ObjectId, definition: CardDefinition, owner: PlayerId) -> GameObject {
    GameObject {
        object_id: id,
        lineage: ObjectLineage { previous: None },
        definition,
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
        visibility: VisibilityPolicy { public: true, visible_to: Vec::new() },
    }
}

fn put_on_battlefield(state: &mut GameState, object: GameObject) {
    let id = object.object_id;
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(id);
    state.objects.insert(id, object);
}

fn script_effect(source: ObjectId, layer: Layer, affected: AffectedFilter, effect: EffectPayload) -> ContinuousEffect {
    ContinuousEffect { source, layer, affected, effect, timestamp: SequenceNo(0), duration: EffectDuration::Permanent }
}

// ---------------------------------------------------------------------
// Unit-level: ScriptRuntime::invoke_continuous_affects/invoke_continuous_effect
// directly, without going through the layer engine.
// ---------------------------------------------------------------------

#[test]
fn affects_predicate_matches_only_the_identical_object() {
    let mut runtime = ScriptRuntime::new();
    let handle = runtime
        .load_function("return function(object, source, controller) return object == source end")
        .expect("loads cleanly");

    let source = ObjectId(1);
    let controller = PlayerId(0);
    assert_eq!(runtime.invoke_continuous_affects(handle, source, source, controller).unwrap(), true);
    assert_eq!(runtime.invoke_continuous_affects(handle, ObjectId(2), source, controller).unwrap(), false);
}

#[test]
fn affects_predicate_erroring_script_is_an_error_not_a_panic() {
    let mut runtime = ScriptRuntime::new();
    let handle = runtime.load_function("return function(object, source, controller) error(\"boom\") end").unwrap();

    let result = runtime.invoke_continuous_affects(handle, ObjectId(1), ObjectId(1), PlayerId(0));
    assert!(result.is_err(), "a script error must not panic - caller treats Err as \"doesn't match\"");
}

#[test]
fn effect_decodes_modify_pt() {
    let mut runtime = ScriptRuntime::new();
    let handle = runtime
        .load_function("return function(source, controller) return { modify_pt = { power = 2, toughness = 3 } } end")
        .unwrap();

    let decoded = runtime.invoke_continuous_effect(handle, ObjectId(1), PlayerId(0)).unwrap();
    assert!(matches!(decoded, Some(EffectPrimitive::ModifyPT { power: 2, toughness: 3 })));
}

#[test]
fn effect_decodes_add_keyword() {
    let mut runtime = ScriptRuntime::new();
    let handle = runtime
        .load_function("return function(source, controller) return { add_keyword = { \"Flying\", \"Vigilance\" } } end")
        .unwrap();

    let decoded = runtime.invoke_continuous_effect(handle, ObjectId(1), PlayerId(0)).unwrap();
    assert_eq!(decoded, Some(EffectPrimitive::AddKeyword(KeywordSet::FLYING | KeywordSet::VIGILANCE)));
}

#[test]
fn effect_decodes_switch_pt() {
    let mut runtime = ScriptRuntime::new();
    let handle =
        runtime.load_function("return function(source, controller) return { switch_pt = true } end").unwrap();

    let decoded = runtime.invoke_continuous_effect(handle, ObjectId(1), PlayerId(0)).unwrap();
    assert!(matches!(decoded, Some(EffectPrimitive::SwitchPT)));
}

#[test]
fn effect_with_no_recognized_key_resolves_to_none() {
    let mut runtime = ScriptRuntime::new();
    let handle = runtime.load_function("return function(source, controller) return {} end").unwrap();

    let decoded = runtime.invoke_continuous_effect(handle, ObjectId(1), PlayerId(0)).unwrap();
    assert_eq!(decoded, None, "an empty table has no recognized shape - skip rather than guess");
}

#[test]
fn effect_erroring_script_is_an_error_not_a_panic() {
    let mut runtime = ScriptRuntime::new();
    let handle = runtime.load_function("return function(source, controller) error(\"boom\") end").unwrap();

    let result = runtime.invoke_continuous_effect(handle, ObjectId(1), PlayerId(0));
    assert!(result.is_err(), "a script error must not panic - caller (resolve_primitive) treats Err as None");
}

// ---------------------------------------------------------------------
// Game.CreateStaticAbility builder: :SetAffected/:SetEffect now accept a
// function, same two-shapes pattern :SetCondition/:SetOperation already use.
// ---------------------------------------------------------------------

const SCRIPT_AFFECTS_SELF_ONLY: &str = r#"
local s = {}
function s.initial_effect(c)
    local e = Game.CreateStaticAbility(c)
    e:SetLayer(Game.Layers.PowerToughnessModifying)
    e:SetEffect(Game.Effects.ModifyPT(1, 1))
    e:SetAffected(function(object, source, controller) return object == source end)
    e:Register()
end
return s
"#;

const SCRIPT_EFFECT_MODIFY_PT: &str = r#"
local s = {}
function s.initial_effect(c)
    local e = Game.CreateStaticAbility(c)
    e:SetLayer(Game.Layers.PowerToughnessModifying)
    e:SetEffect(function(source, controller) return { modify_pt = { power = 1, toughness = 1 } } end)
    e:SetAffected({ self_only = true })
    e:Register()
end
return s
"#;

#[test]
fn run_card_script_self_registers_a_script_backed_affects_predicate() {
    let mut state = base_state();
    let bear = ObjectId(0);
    let definition = creature_definition(2, 2, ScriptReference::Inline(SCRIPT_AFFECTS_SELF_ONLY.to_string()));
    put_on_battlefield(&mut state, card_object(bear, definition.clone(), PlayerId(0)));

    let mut runtime = ScriptRuntime::new();
    runtime.run_card_script(&mut state, &definition, bear).expect("Register() succeeds with a function :SetAffected");

    assert_eq!(state.continuous_effects.len(), 1);
    assert!(matches!(state.continuous_effects[0].affected, AffectedFilter::Script(_)));
    assert!(matches!(state.continuous_effects[0].effect, EffectPayload::Primitive(EffectPrimitive::ModifyPT { .. })));
}

#[test]
fn run_card_script_self_registers_a_script_backed_effect_payload() {
    let mut state = base_state();
    let bear = ObjectId(0);
    let definition = creature_definition(2, 2, ScriptReference::Inline(SCRIPT_EFFECT_MODIFY_PT.to_string()));
    put_on_battlefield(&mut state, card_object(bear, definition.clone(), PlayerId(0)));

    let mut runtime = ScriptRuntime::new();
    runtime.run_card_script(&mut state, &definition, bear).expect(
        "Register() succeeds with a function :SetEffect even though validate_layer_effect_pairing can't \
         check its real shape yet",
    );

    assert_eq!(state.continuous_effects.len(), 1);
    assert!(matches!(state.continuous_effects[0].effect, EffectPayload::Script(_)));
}

// ---------------------------------------------------------------------
// Real consumer: effective_characteristics (and, transitively, sba.rs's
// lethal-toughness check / combat damage) actually apply a script-backed
// affects-predicate or effect payload, not just decode it in isolation.
// ---------------------------------------------------------------------

#[test]
fn script_backed_affects_predicate_applies_only_to_the_matching_object() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let anthem = ObjectId(0);
    let other = ObjectId(1);
    put_on_battlefield(&mut state, scripted_creature(anthem, p0, "return {}"));
    put_on_battlefield(&mut state, creature(other, p0, 2, 2));

    let mut script_runtime = ScriptRuntime::new();
    let handle = script_runtime
        .load_function("return function(object, source, controller) return object == source end")
        .unwrap();
    state.continuous_effects.push(script_effect(
        anthem,
        Layer::PowerToughnessModifying,
        AffectedFilter::Script(handle),
        EffectPayload::Primitive(EffectPrimitive::ModifyPT { power: 1, toughness: 1 }),
    ));

    let anthem_effective = effective_characteristics(&state, &script_runtime, anthem);
    let other_effective = effective_characteristics(&state, &script_runtime, other);

    assert_eq!(anthem_effective.power, Some(3), "self_only-equivalent predicate should affect its own source");
    assert_eq!(other_effective.power, Some(2), "a different object must not match the identity predicate");
}

#[test]
fn script_backed_effect_payload_applies_through_effective_characteristics() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let source = ObjectId(0);
    put_on_battlefield(&mut state, creature(source, p0, 2, 2));

    let mut script_runtime = ScriptRuntime::new();
    let handle = script_runtime
        .load_function("return function(source, controller) return { add_keyword = { \"Flying\" } } end")
        .unwrap();
    state.continuous_effects.push(script_effect(
        source,
        Layer::AbilityAdding,
        AffectedFilter::Primitive(EffectFilter { controller: None, card_types: None, self_only: true }),
        EffectPayload::Script(handle),
    ));

    let effective = effective_characteristics(&state, &script_runtime, source);
    assert!(effective.keywords.contains(KeywordSet::FLYING), "script-decoded add_keyword must actually apply");
}

#[test]
fn mismatched_layer_and_decoded_shape_is_skipped_not_applied() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let source = ObjectId(0);
    put_on_battlefield(&mut state, creature(source, p0, 2, 2));

    let mut script_runtime = ScriptRuntime::new();
    // Declared layer is PowerToughnessModifying, but the script decodes to
    // AddKeyword - a real shape mismatch that validate_layer_effect_pairing
    // could catch for a Primitive effect at :Register() time, but can't for
    // Script (see validate_layer_effect_pairing's own doc comment).
    let handle = script_runtime
        .load_function("return function(source, controller) return { add_keyword = { \"Flying\" } } end")
        .unwrap();
    state.continuous_effects.push(script_effect(
        source,
        Layer::PowerToughnessModifying,
        AffectedFilter::Primitive(EffectFilter { controller: None, card_types: None, self_only: true }),
        EffectPayload::Script(handle),
    ));

    let effective = effective_characteristics(&state, &script_runtime, source);
    assert_eq!(effective.power, Some(2), "a mismatched decoded shape must be skipped, not applied as garbage");
    assert_eq!(effective.toughness, Some(2));
    assert!(!effective.keywords.contains(KeywordSet::FLYING), "AddKeyword decoded in the wrong layer's loop never runs");
}
