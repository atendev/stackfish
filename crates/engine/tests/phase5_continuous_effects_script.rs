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

use stackfish_engine::layers::effective_characteristics;
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{card_object, zones, two_player_state as base_two_player_state};

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
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn creature(id: ObjectId, owner: PlayerId, power: i32, toughness: i32) -> GameObject {
    let definition = creature_definition(power, toughness, ScriptReference::ByKey("vanilla".to_string()));
    card_object(id, definition, owner, ZoneKind::Battlefield)
}

fn scripted_creature(id: ObjectId, owner: PlayerId, source: &str) -> GameObject {
    let definition = creature_definition(2, 2, ScriptReference::Inline(source.to_string()));
    card_object(id, definition, owner, ZoneKind::Battlefield)
}

fn base_state() -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState { step: Step::PreCombatMain, ..base_two_player_state(zones(p0, p1, &[ZoneKind::Battlefield, ZoneKind::Graveyard])) }
}

fn put_on_battlefield(state: &mut GameState, object: GameObject) {
    let id = object.object_id;
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(id);
    state.objects.insert(id, object);
}

fn script_effect(source: ObjectId, layer: Layer, affected: AffectedFilter, effect: EffectPayload) -> ContinuousEffect {
    ContinuousEffect { source, layer, affected, effect, timestamp: SequenceNo(0), duration: EffectDuration::Permanent, condition: None }
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

    let state = base_state();
    let source = ObjectId(1);
    let controller = PlayerId(0);
    assert_eq!(runtime.invoke_continuous_affects(handle, &state, source, source, controller, None).unwrap(), true);
    assert_eq!(runtime.invoke_continuous_affects(handle, &state, ObjectId(2), source, controller, None).unwrap(), false);
}

#[test]
fn affects_predicate_erroring_script_is_an_error_not_a_panic() {
    let mut runtime = ScriptRuntime::new();
    let handle = runtime.load_function("return function(object, source, controller) error(\"boom\") end").unwrap();

    let state = base_state();
    let result = runtime.invoke_continuous_affects(handle, &state, ObjectId(1), ObjectId(1), PlayerId(0), None);
    assert!(result.is_err(), "a script error must not panic - caller treats Err as \"doesn't match\"");
}

#[test]
fn effect_decodes_modify_pt() {
    let mut runtime = ScriptRuntime::new();
    let handle = runtime
        .load_function("return function(source, controller) return { modify_pt = { power = 2, toughness = 3 } } end")
        .unwrap();

    let state = base_state();
    let decoded = runtime.invoke_continuous_effect(handle, &state, ObjectId(1), PlayerId(0), None).unwrap();
    assert!(matches!(decoded, Some(EffectPrimitive::ModifyPT { power: 2, toughness: 3 })));
}

#[test]
fn effect_decodes_add_keyword() {
    let mut runtime = ScriptRuntime::new();
    let handle = runtime
        .load_function("return function(source, controller) return { add_keyword = { \"Flying\", \"Vigilance\" } } end")
        .unwrap();

    let state = base_state();
    let decoded = runtime.invoke_continuous_effect(handle, &state, ObjectId(1), PlayerId(0), None).unwrap();
    assert_eq!(decoded, Some(EffectPrimitive::AddKeyword(KeywordSet::FLYING | KeywordSet::VIGILANCE)));
}

#[test]
fn effect_decodes_remove_keyword() {
    let mut runtime = ScriptRuntime::new();
    let handle = runtime
        .load_function("return function(source, controller) return { remove_keyword = { \"Defender\" } } end")
        .unwrap();

    let state = base_state();
    let decoded = runtime.invoke_continuous_effect(handle, &state, ObjectId(1), PlayerId(0), None).unwrap();
    assert_eq!(decoded, Some(EffectPrimitive::RemoveKeyword(KeywordSet::DEFENDER)));
}

#[test]
fn effect_decodes_switch_pt() {
    let mut runtime = ScriptRuntime::new();
    let handle =
        runtime.load_function("return function(source, controller) return { switch_pt = true } end").unwrap();

    let state = base_state();
    let decoded = runtime.invoke_continuous_effect(handle, &state, ObjectId(1), PlayerId(0), None).unwrap();
    assert!(matches!(decoded, Some(EffectPrimitive::SwitchPT)));
}

#[test]
fn effect_decodes_double_pt() {
    let mut runtime = ScriptRuntime::new();
    let handle = runtime
        .load_function("return function(source, controller) return { double_pt = { power = true, toughness = false } } end")
        .unwrap();

    let state = base_state();
    let decoded = runtime.invoke_continuous_effect(handle, &state, ObjectId(1), PlayerId(0), None).unwrap();
    assert_eq!(decoded, Some(EffectPrimitive::DoublePT { power: true, toughness: false }));
}

#[test]
fn effect_with_no_recognized_key_resolves_to_none() {
    let mut runtime = ScriptRuntime::new();
    let handle = runtime.load_function("return function(source, controller) return {} end").unwrap();

    let state = base_state();
    let decoded = runtime.invoke_continuous_effect(handle, &state, ObjectId(1), PlayerId(0), None).unwrap();
    assert_eq!(decoded, None, "an empty table has no recognized shape - skip rather than guess");
}

#[test]
fn effect_erroring_script_is_an_error_not_a_panic() {
    let mut runtime = ScriptRuntime::new();
    let handle = runtime.load_function("return function(source, controller) error(\"boom\") end").unwrap();

    let state = base_state();
    let result = runtime.invoke_continuous_effect(handle, &state, ObjectId(1), PlayerId(0), None);
    assert!(result.is_err(), "a script error must not panic - caller (resolve_primitive) treats Err as None");
}

// ---------------------------------------------------------------------
// Game.CreateStaticAbility builder: :SetAffected/:SetEffect now accept a
// function, same two-shapes pattern :SetCondition/:SetOperation already use.
// ---------------------------------------------------------------------

const SCRIPT_AFFECTS_SELF_ONLY: &str = r#"
local s = {}
function s.register_abilities(c)
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
function s.register_abilities(c)
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
    put_on_battlefield(&mut state, card_object(bear, definition.clone(), PlayerId(0), ZoneKind::Battlefield));

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
    put_on_battlefield(&mut state, card_object(bear, definition.clone(), PlayerId(0), ZoneKind::Battlefield));

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
        AffectedFilter::Primitive(EffectFilter {  controller: None, card_types: None, self_only: true, subtypes: None, excluded_subtypes: None, supertypes: None, excluded_supertypes: None, }),
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
        AffectedFilter::Primitive(EffectFilter {  controller: None, card_types: None, self_only: true, subtypes: None, excluded_subtypes: None, supertypes: None, excluded_supertypes: None, }),
        EffectPayload::Script(handle),
    ));

    let effective = effective_characteristics(&state, &script_runtime, source);
    assert_eq!(effective.power, Some(2), "a mismatched decoded shape must be skipped, not applied as garbage");
    assert_eq!(effective.toughness, Some(2));
    assert!(!effective.keywords.contains(KeywordSet::FLYING), "AddKeyword decoded in the wrong layer's loop never runs");
}
