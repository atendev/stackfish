//! Phase 5 vertical slice: static ability self-registration (CR 604/611). A
//! card's own Lua `register_abilities(c)` can now call `Game.CreateStaticAbility(c)`
//! to attach a real `ContinuousEffect` to `GameState.continuous_effects` -
//! mirrors `Game.CreateTriggeredAbility`'s builder/queued-registration
//! mechanism exactly (`StaticAbilityBuilder` alongside
//! `TriggeredAbilityBuilder` in script.rs), just producing a `ContinuousEffect`
//! instead of a `RegisteredAbility`. Building one by hand (Rust struct
//! literal / TOML `[cards.continuous_effect]`) remains fully supported and
//! unaffected.
//!
//! Same self-registration call sites as the triggered-ability slice
//! (`PlayLand`, `ResolveStackTop`'s permanent-entry branch) - no new call
//! sites needed, since `ScriptRuntime::run_card_script` now drains both
//! `pending_triggered` and `pending_static` in one pass.

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::layers::effective_characteristics;
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{
    card_object, drain, hand_battlefield_graveyard_zones, minimal_card_definition, two_player_state as base_two_player_state,
};

const SELF_BUFF: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateStaticAbility(c)
    e:SetLayer(Game.Layers.PowerToughnessModifying)
    e:SetEffect(Game.Effects.ModifyPT(1, 1))
    e:SetAffected({ self_only = true })
    e:Register()
end
return s
"#;

const ANTHEM_YOUR_CREATURES: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateStaticAbility(c)
    e:SetLayer(Game.Layers.PowerToughnessModifying)
    e:SetEffect(Game.Effects.ModifyPT(1, 1))
    e:SetAffected({ controller = "you", card_types = { "Creature" } })
    e:Register()
end
return s
"#;

const NO_SET_LAYER: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateStaticAbility(c)
    e:SetEffect(Game.Effects.ModifyPT(1, 1))
    e:Register()
end
return s
"#;

const NO_SET_EFFECT: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateStaticAbility(c)
    e:SetLayer(Game.Layers.PowerToughnessModifying)
    e:Register()
end
return s
"#;

const MISMATCHED_LAYER_EFFECT: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateStaticAbility(c)
    e:SetLayer(Game.Layers.TypeChanging)
    e:SetEffect(Game.Effects.ModifyPT(1, 1))
    e:Register()
end
return s
"#;

const REMOVE_DEFENDER: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateStaticAbility(c)
    e:SetLayer(Game.Layers.AbilityAdding)
    e:SetEffect(Game.Effects.RemoveKeyword("Defender"))
    e:SetAffected({ self_only = true })
    e:Register()
end
return s
"#;

const MISMATCHED_REMOVE_KEYWORD: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateStaticAbility(c)
    e:SetLayer(Game.Layers.TypeChanging)
    e:SetEffect(Game.Effects.RemoveKeyword("Defender"))
    e:Register()
end
return s
"#;

const TWO_STATIC_ABILITIES: &str = r#"
local s = {}
function s.register_abilities(c)
    local e1 = Game.CreateStaticAbility(c)
    e1:SetLayer(Game.Layers.PowerToughnessModifying)
    e1:SetEffect(Game.Effects.ModifyPT(1, 0))
    e1:SetAffected({ self_only = true })
    e1:Register()

    local e2 = Game.CreateStaticAbility(c)
    e2:SetLayer(Game.Layers.AbilityAdding)
    e2:SetEffect(Game.Effects.AddKeyword("Flying"))
    e2:SetAffected({ self_only = true })
    e2:Register()
end
return s
"#;

fn bear_definition(script: &str) -> CardDefinition {
    CardDefinition {
        power: Some(PTValue::Fixed(2)),
        toughness: Some(PTValue::Fixed(2)),
        oracle_text: "Static ability self-registration test fixture.".to_string(),
        script_ref: ScriptReference::Inline(script.to_string()),
        ..minimal_card_definition(1, "Test Bear", "test_bear", ManaCost { symbols: Vec::new() }, CardTypeSet::CREATURE)
    }
}

fn vanilla_bear_definition() -> CardDefinition {
    CardDefinition {
        power: Some(PTValue::Fixed(2)),
        toughness: Some(PTValue::Fixed(2)),
        ..minimal_card_definition(2, "Vanilla Bear", "vanilla_bear", ManaCost { symbols: Vec::new() }, CardTypeSet::CREATURE)
    }
}

/// Mirrors `phase5_script_self_registration::two_player_state`.
fn two_player_state() -> GameState {
    base_two_player_state(hand_battlefield_graveyard_zones(PlayerId(0), PlayerId(1)))
}

// ---------------------------------------------------------------------
// Unit-level: ScriptRuntime::run_card_script's static self-registration
// mechanics, without driving the whole Engine pipeline.
// ---------------------------------------------------------------------

#[test]
fn run_card_script_self_registers_a_static_ability() {
    let mut state = two_player_state();
    let bear = ObjectId(0);
    let definition = bear_definition(SELF_BUFF);
    state.objects.insert(bear, card_object(bear, definition.clone(), PlayerId(0), ZoneKind::Battlefield));

    let mut runtime = ScriptRuntime::new();
    runtime.run_card_script(&mut state, &definition, bear).expect("script self-registers cleanly");

    assert_eq!(state.continuous_effects.len(), 1);
    let effect = &state.continuous_effects[0];
    assert_eq!(effect.source, bear);
    assert_eq!(effect.layer, Layer::PowerToughnessModifying);
    assert!(matches!(effect.effect, EffectPayload::Primitive(EffectPrimitive::ModifyPT { power: 1, toughness: 1 })));
    assert!(matches!(effect.affected, AffectedFilter::Primitive(EffectFilter { self_only: true, .. })));
}

#[test]
fn run_card_script_register_without_set_layer_is_an_error() {
    let mut state = two_player_state();
    let bear = ObjectId(0);
    let definition = bear_definition(NO_SET_LAYER);
    state.objects.insert(bear, card_object(bear, definition.clone(), PlayerId(0), ZoneKind::Battlefield));

    let mut runtime = ScriptRuntime::new();
    let result = runtime.run_card_script(&mut state, &definition, bear);

    assert!(result.is_err(), "Register() with no SetLayer() must fail");
    assert!(state.continuous_effects.is_empty(), "no partial effect should have been committed");
}

#[test]
fn run_card_script_register_without_set_effect_is_an_error() {
    let mut state = two_player_state();
    let bear = ObjectId(0);
    let definition = bear_definition(NO_SET_EFFECT);
    state.objects.insert(bear, card_object(bear, definition.clone(), PlayerId(0), ZoneKind::Battlefield));

    let mut runtime = ScriptRuntime::new();
    let result = runtime.run_card_script(&mut state, &definition, bear);

    assert!(result.is_err(), "Register() with no SetEffect() must fail");
    assert!(state.continuous_effects.is_empty());
}

#[test]
fn run_card_script_rejects_mismatched_layer_and_effect() {
    let mut state = two_player_state();
    let bear = ObjectId(0);
    let definition = bear_definition(MISMATCHED_LAYER_EFFECT);
    state.objects.insert(bear, card_object(bear, definition.clone(), PlayerId(0), ZoneKind::Battlefield));

    let mut runtime = ScriptRuntime::new();
    let result = runtime.run_card_script(&mut state, &definition, bear);

    assert!(
        result.is_err(),
        "SetLayer(TypeChanging) + SetEffect(ModifyPT(..)) is a real mismatch, must be rejected"
    );
    assert!(state.continuous_effects.is_empty());
}

#[test]
fn run_card_script_self_registers_a_remove_keyword_static_ability() {
    let mut state = two_player_state();
    let bear = ObjectId(0);
    let definition = bear_definition(REMOVE_DEFENDER);
    state.objects.insert(bear, card_object(bear, definition.clone(), PlayerId(0), ZoneKind::Battlefield));

    let mut runtime = ScriptRuntime::new();
    runtime.run_card_script(&mut state, &definition, bear).expect("script self-registers cleanly");

    assert_eq!(state.continuous_effects.len(), 1);
    let effect = &state.continuous_effects[0];
    assert_eq!(effect.layer, Layer::AbilityAdding);
    assert!(matches!(effect.effect, EffectPayload::Primitive(EffectPrimitive::RemoveKeyword(set)) if set == KeywordSet::DEFENDER));
}

#[test]
fn run_card_script_rejects_mismatched_layer_and_remove_keyword() {
    let mut state = two_player_state();
    let bear = ObjectId(0);
    let definition = bear_definition(MISMATCHED_REMOVE_KEYWORD);
    state.objects.insert(bear, card_object(bear, definition.clone(), PlayerId(0), ZoneKind::Battlefield));

    let mut runtime = ScriptRuntime::new();
    let result = runtime.run_card_script(&mut state, &definition, bear);

    assert!(
        result.is_err(),
        "SetLayer(TypeChanging) + SetEffect(RemoveKeyword(..)) is a real mismatch, must be rejected"
    );
    assert!(state.continuous_effects.is_empty());
}

#[test]
fn run_card_script_can_register_multiple_static_abilities_from_one_script_call() {
    let mut state = two_player_state();
    let bear = ObjectId(0);
    let definition = bear_definition(TWO_STATIC_ABILITIES);
    state.objects.insert(bear, card_object(bear, definition.clone(), PlayerId(0), ZoneKind::Battlefield));

    let mut runtime = ScriptRuntime::new();
    runtime.run_card_script(&mut state, &definition, bear).expect("both static abilities register cleanly");

    assert_eq!(state.continuous_effects.len(), 2);
    assert!(state.continuous_effects.iter().all(|e| e.source == bear));
}

// ---------------------------------------------------------------------
// SBA: a dying creature's self-registered static ability is purged, not
// left dangling in state.continuous_effects (retire_object extension).
// ---------------------------------------------------------------------

#[test]
fn dying_creatures_registered_static_ability_is_purged_by_sba() {
    let mut state = two_player_state();
    let bear = ObjectId(0);
    let mut object = card_object(bear, bear_definition(SELF_BUFF), PlayerId(0), ZoneKind::Battlefield);
    // Base 2/2 + self ModifyPT(1,1) = effective toughness 3 - 2 damage isn't
    // lethal *with* the effect applied, so seed 3 damage (lethal against the
    // buffed 3 toughness) to force death without needing combat machinery.
    object.damage_marked = 3;
    state.objects.insert(bear, object);
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(bear);
    // Simulates the continuous effect this creature would have
    // self-registered upon an earlier entrance to the battlefield.
    state.continuous_effects.push(ContinuousEffect {
        source: bear,
        layer: Layer::PowerToughnessModifying,
        affected: AffectedFilter::Primitive(EffectFilter {  controller: None, card_types: None, self_only: true, subtypes: None, excluded_subtypes: None, supertypes: None, excluded_supertypes: None, }),
        effect: EffectPayload::Primitive(EffectPrimitive::ModifyPT { power: 1, toughness: 1 }),
        timestamp: SequenceNo(0),
        duration: EffectDuration::Permanent,
        condition: None,
    });

    let script_runtime = ScriptRuntime::new();
    let _events = stackfish_engine::sba::check_state_based_actions(&mut state, &script_runtime);

    assert!(
        state.objects.get(&bear).is_none() || state.objects[&bear].zone != ZoneKind::Battlefield,
        "3 damage is lethal against the buffed 3 toughness - the creature should have died"
    );
    assert!(
        state.continuous_effects.iter().all(|e| e.source != bear),
        "the dead creature's self-registered continuous effect must be purged via retire_object, \
         not left dangling"
    );
}

// ---------------------------------------------------------------------
// End-to-end: real Engine pipeline, the self-registered static effect
// actually changes effective_characteristics once the source resolves.
// ---------------------------------------------------------------------

fn pay_free_and_drain(engine: &mut Engine) -> StepStatus {
    let status = drain(engine);
    assert!(
        matches!(status, StepStatus::AwaitingPayment { .. }),
        "expected a pause for the cast's (free) cost, got {status:?}"
    );
    engine.submit_payment(PaymentPlan { payments: Vec::new() }).expect("empty plan pays a {0} cost");
    drain(engine)
}

fn pass(engine: &mut Engine, player: PlayerId) {
    engine.submit_action(PlayerAction { player, kind: PlayerActionKind::PassPriority }).unwrap();
}

#[test]
fn cast_self_buffing_creature_resolves_with_effective_pt_boosted() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let bear = ObjectId(0);
    let mut state = two_player_state();
    state.objects.insert(bear, card_object(bear, bear_definition(SELF_BUFF), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(bear);

    let mut engine = Engine::new(state);
    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::CastSpell { card: bear, modes: Vec::new(), targets: Vec::new(), x_value: None, alternative_cost: None },
        })
        .expect("p0 holds priority, bear is in hand, main phase, empty stack");
    pay_free_and_drain(&mut engine);

    assert!(engine.state().continuous_effects.is_empty(), "self-registration hasn't run yet - still on the stack");

    pass(&mut engine, p0);
    drain(&mut engine);
    pass(&mut engine, p1);
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));

    assert!(engine.state().stack.is_empty(), "no triggered ability this time - only the static effect registered");
    assert_eq!(engine.state().continuous_effects.len(), 1, "self-registration ran exactly once");

    let new_id = *engine.state().zone(ZoneKind::Battlefield, None).unwrap().contents.front().unwrap();
    let script_runtime = ScriptRuntime::new();
    let effective = effective_characteristics(engine.state(), &script_runtime, new_id);
    assert_eq!(effective.power, Some(3), "base 2 + self ModifyPT(1,1) = 3");
    assert_eq!(effective.toughness, Some(3));
}

#[test]
fn play_land_self_registering_anthem_buffs_other_creatures_you_control() {
    let p0 = PlayerId(0);
    let anthem_land = ObjectId(0);
    let other_bear = ObjectId(1);
    let mut state = two_player_state();
    state.objects.insert(anthem_land, card_object(anthem_land, bear_definition_as_land(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(anthem_land);
    state.objects.insert(other_bear, card_object(other_bear, vanilla_bear_definition(), p0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(other_bear);

    let mut engine = Engine::new(state);
    engine
        .submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PlayLand { card: anthem_land } })
        .expect("p0 holds priority, land is in hand");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));

    assert_eq!(engine.state().continuous_effects.len(), 1, "the anthem land's static ability self-registered");

    let script_runtime = ScriptRuntime::new();
    let effective = effective_characteristics(engine.state(), &script_runtime, other_bear);
    assert_eq!(effective.power, Some(3), "vanilla 2/2 + anthem's ModifyPT(1,1) = 3/3");
    assert_eq!(effective.toughness, Some(3));
}

/// A land version of the anthem script, so `play_land_self_registering_anthem_buffs_other_creatures_you_control`
/// can self-register via `PlayLand` (which only accepts `CardTypeSet::LAND`)
/// rather than `CastSpell`.
fn bear_definition_as_land() -> CardDefinition {
    CardDefinition {
        oracle_text: "Creatures you control get +1/+1.".to_string(),
        script_ref: ScriptReference::Inline(ANTHEM_YOUR_CREATURES.to_string()),
        ..minimal_card_definition(3, "Test Anthem Land", "test_anthem_land", ManaCost { symbols: Vec::new() }, CardTypeSet::LAND)
    }
}
