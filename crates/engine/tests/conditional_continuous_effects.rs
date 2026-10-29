//! CR 611.2c: conditional ("as long as ...") continuous effects, scoped to `InterveningIfPrimitive::ControllerControlsAtLeast` and reusing CR 603.4's `InterveningIfSpec`.
//! `layers::applicable_effects_in_layer` drops an effect whose `condition` does not hold, checked fresh on every call.
//! The Lua side is `Game.Conditions.ControlsAtLeast` with `StaticAbilityBuilder:SetCondition`.

use std::collections::HashMap;

use stackfish_engine::layers::effective_characteristics;
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{card_object, zones, two_player_state as base_two_player_state};

fn creature_definition(power: i32, toughness: i32, card_types: CardTypeSet) -> CardDefinition {
    CardDefinition {
        definition_id: 1,
        name: "Test Permanent".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: Some(PTValue::Fixed(power)),
        toughness: Some(PTValue::Fixed(toughness)),
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_permanent".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn permanent(id: ObjectId, owner: PlayerId, power: i32, toughness: i32, card_types: CardTypeSet) -> GameObject {
    card_object(id, creature_definition(power, toughness, card_types), owner, ZoneKind::Battlefield)
}

fn base_state() -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        step: Step::PreCombatMain,
        last_active_turn_number: HashMap::from([(p0, 1)]),
        ..base_two_player_state(zones(p0, p1, &[ZoneKind::Battlefield, ZoneKind::Graveyard]))
    }
}

fn put_on_battlefield(state: &mut GameState, object: GameObject) {
    let id = object.object_id;
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(id);
    state.objects.insert(id, object);
}

fn conditional_anthem(source: ObjectId, count: u32) -> ContinuousEffect {
    ContinuousEffect {
        source,
        layer: Layer::PowerToughnessModifying,
        affected: AffectedFilter::Primitive(EffectFilter { 
            controller: Some(FilterController::You),
            card_types: None,
            self_only: false, subtypes: None, excluded_subtypes: None, supertypes: None, excluded_supertypes: None, }),
        effect: EffectPayload::Primitive(EffectPrimitive::ModifyPT { power: 1, toughness: 1 }),
        timestamp: SequenceNo(0),
        duration: EffectDuration::Permanent,
        condition: Some(InterveningIfSpec::Primitive(InterveningIfPrimitive::ControllerControlsAtLeast {
            filter: FilterSpec {
                card_types: Some(CardTypeSet::ARTIFACT),
                supertypes: None,
                subtypes: None,
                controller: Some(FilterController::You),
                random: false,
                extra: None,
            },
            count,
        })),
    }
}

#[test]
fn conditional_effect_applies_when_the_control_count_is_met() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let anthem = ObjectId(0);
    let bear = ObjectId(1);
    put_on_battlefield(&mut state, permanent(anthem, p0, 0, 0, CardTypeSet::ENCHANTMENT));
    put_on_battlefield(&mut state, permanent(bear, p0, 2, 2, CardTypeSet::CREATURE));
    put_on_battlefield(&mut state, permanent(ObjectId(2), p0, 0, 0, CardTypeSet::ARTIFACT));
    put_on_battlefield(&mut state, permanent(ObjectId(3), p0, 0, 0, CardTypeSet::ARTIFACT));
    put_on_battlefield(&mut state, permanent(ObjectId(4), p0, 0, 0, CardTypeSet::ARTIFACT));
    state.continuous_effects.push(conditional_anthem(anthem, 3));

    let effective = effective_characteristics(&state, &ScriptRuntime::new(), bear);

    assert_eq!(effective.power, Some(3), "3 artifacts meets the threshold - the anthem applies");
    assert_eq!(effective.toughness, Some(3));
}

#[test]
fn conditional_effect_does_not_apply_when_the_control_count_is_not_met() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let anthem = ObjectId(0);
    let bear = ObjectId(1);
    put_on_battlefield(&mut state, permanent(anthem, p0, 0, 0, CardTypeSet::ENCHANTMENT));
    put_on_battlefield(&mut state, permanent(bear, p0, 2, 2, CardTypeSet::CREATURE));
    put_on_battlefield(&mut state, permanent(ObjectId(2), p0, 0, 0, CardTypeSet::ARTIFACT));
    state.continuous_effects.push(conditional_anthem(anthem, 3));

    let effective = effective_characteristics(&state, &ScriptRuntime::new(), bear);

    assert_eq!(effective.power, Some(2), "only 1 artifact - below the threshold, no boost");
    assert_eq!(effective.toughness, Some(2));
}

#[test]
fn conditional_effect_stops_applying_the_instant_a_matching_permanent_leaves() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let anthem = ObjectId(0);
    let bear = ObjectId(1);
    put_on_battlefield(&mut state, permanent(anthem, p0, 0, 0, CardTypeSet::ENCHANTMENT));
    put_on_battlefield(&mut state, permanent(bear, p0, 2, 2, CardTypeSet::CREATURE));
    put_on_battlefield(&mut state, permanent(ObjectId(2), p0, 0, 0, CardTypeSet::ARTIFACT));
    put_on_battlefield(&mut state, permanent(ObjectId(3), p0, 0, 0, CardTypeSet::ARTIFACT));
    put_on_battlefield(&mut state, permanent(ObjectId(4), p0, 0, 0, CardTypeSet::ARTIFACT));
    state.continuous_effects.push(conditional_anthem(anthem, 3));

    assert_eq!(
        effective_characteristics(&state, &ScriptRuntime::new(), bear).power,
        Some(3),
        "starts with 3 artifacts - the anthem applies"
    );

    // A plain zone removal and re-query is the whole proof of continuous re-evaluation (CR 611.2c).
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().remove(ObjectId(4));
    state.objects.remove(&ObjectId(4));

    assert_eq!(
        effective_characteristics(&state, &ScriptRuntime::new(), bear).power,
        Some(2),
        "down to 2 artifacts - the anthem stops applying, re-derived fresh"
    );
}

#[test]
fn an_unconditional_effect_is_completely_unaffected() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let anthem = ObjectId(0);
    let bear = ObjectId(1);
    put_on_battlefield(&mut state, permanent(anthem, p0, 0, 0, CardTypeSet::ENCHANTMENT));
    put_on_battlefield(&mut state, permanent(bear, p0, 2, 2, CardTypeSet::CREATURE));
    state.continuous_effects.push(ContinuousEffect {
        source: anthem,
        layer: Layer::PowerToughnessModifying,
        affected: AffectedFilter::Primitive(EffectFilter {  controller: None, card_types: None, self_only: false, subtypes: None, excluded_subtypes: None, supertypes: None, excluded_supertypes: None, }),
        effect: EffectPayload::Primitive(EffectPrimitive::ModifyPT { power: 1, toughness: 1 }),
        timestamp: SequenceNo(0),
        duration: EffectDuration::Permanent,
        condition: None,
    });

    let effective = effective_characteristics(&state, &ScriptRuntime::new(), bear);

    assert_eq!(effective.power, Some(3), "condition: None means unconditional, same as before this slice");
    assert_eq!(effective.toughness, Some(3));
}

// Returns the ScriptRuntime too, because a Script condition's ScriptHandle resolves only against the runtime that loaded it.
fn run_static_ability_script(source: ObjectId, script: &str) -> (GameState, ScriptRuntime) {
    let mut state = base_state();
    let p0 = PlayerId(0);
    put_on_battlefield(&mut state, permanent(source, p0, 0, 0, CardTypeSet::ENCHANTMENT));
    put_on_battlefield(&mut state, permanent(ObjectId(1), p0, 2, 2, CardTypeSet::CREATURE));

    let mut runtime = ScriptRuntime::new();
    let definition = CardDefinition { script_ref: ScriptReference::Inline(script.to_string()), ..creature_definition(0, 0, CardTypeSet::ENCHANTMENT) };
    runtime.run_card_script(&mut state, &definition, source).expect("card script registers a static ability");
    (state, runtime)
}

#[test]
fn lua_controls_at_least_primitive_round_trips_through_set_condition() {
    let source = ObjectId(0);
    let script = r#"
        local s = {}
        function s.register_abilities(c)
            local e = Game.CreateStaticAbility(c)
            e:SetLayer(Game.Layers.PowerToughnessModifying)
            e:SetEffect(Game.Effects.ModifyPT(1, 1))
            e:SetAffected(Game.Filter({ controller = "you" }))
            e:SetCondition(Game.Conditions.ControlsAtLeast({ card_types = { "Artifact" } }, 3))
            e:Register()
        end
        return s
    "#;
    let (mut state, runtime) = run_static_ability_script(source, script);
    let bear = ObjectId(1);

    // Below the threshold - no boost yet.
    assert_eq!(effective_characteristics(&state, &runtime, bear).power, Some(2));

    // Add 3 artifacts and re-query: the condition is re-evaluated fresh, with no re-registration.
    put_on_battlefield(&mut state, permanent(ObjectId(2), PlayerId(0), 0, 0, CardTypeSet::ARTIFACT));
    put_on_battlefield(&mut state, permanent(ObjectId(3), PlayerId(0), 0, 0, CardTypeSet::ARTIFACT));
    put_on_battlefield(&mut state, permanent(ObjectId(4), PlayerId(0), 0, 0, CardTypeSet::ARTIFACT));
    assert_eq!(effective_characteristics(&state, &runtime, bear).power, Some(3));
}

#[test]
fn lua_set_condition_accepts_a_custom_function_as_a_script_backed_condition() {
    let source = ObjectId(0);
    // A custom condition function, through the same InterveningIfSpec::Script path as CR 603.4's SetInterveningIf.
    let script = r#"
        local s = {}
        function s.register_abilities(c)
            local e = Game.CreateStaticAbility(c)
            e:SetLayer(Game.Layers.PowerToughnessModifying)
            e:SetEffect(Game.Effects.ModifyPT(1, 1))
            e:SetAffected(Game.Filter({ controller = "you" }))
            e:SetCondition(function(src, controller, snapshot)
                return true
            end)
            e:Register()
        end
        return s
    "#;
    let (state, runtime) = run_static_ability_script(source, script);
    let bear = ObjectId(1);

    assert_eq!(
        effective_characteristics(&state, &runtime, bear).power,
        Some(3),
        "the Script-backed condition always returns true, so the effect applies"
    );
}
