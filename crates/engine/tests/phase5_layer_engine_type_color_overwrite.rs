//! CR 613.1a, 613.1d: `EffectPrimitive::SetCardTypes` and `SetColors` overwrite whatever came before in layers 4 and 5, like `SetPT` in 7b, and `AddSupertype` and `AddSubtype` fold into the layer 4 pass (CR 205.1b, 205.4b).
//! Effects are built directly by struct literal, and the Lua round-trip test goes through a real card script.
// TODO: the CR 205.1a carve-out that an Instant or Sorcery keeps that type through a card-type overwrite
// TODO: CR 205.3d, a subtype that does not correspond to one of the object's card types, is not validated
// TODO: full-overwrite `SetSupertypes` and `SetSubtypes`, since only the additive `Add*` form exists

use std::collections::HashMap;

use stackfish_engine::layers::effective_characteristics;
use stackfish_engine::sba::{check_state_based_actions, SbaOutcome};
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
        subtypes: vec![Subtype("Bear".to_string())],
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

fn land_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 2,
        name: "Test Land".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::LAND,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_land".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn land(id: ObjectId, owner: PlayerId) -> GameObject {
    card_object(id, land_definition(), owner, ZoneKind::Battlefield)
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

fn continuous_effect(source: ObjectId, layer: Layer, effect: EffectPrimitive, timestamp: u64) -> ContinuousEffect {
    ContinuousEffect {
        source,
        layer,
        affected: AffectedFilter::Primitive(no_filter()),
        effect: EffectPayload::Primitive(effect),
        timestamp: SequenceNo(timestamp),
        duration: EffectDuration::Permanent,
        condition: None,
    }
}

fn no_filter() -> EffectFilter {
    EffectFilter { controller: None, card_types: None, self_only: false, subtypes: None, excluded_subtypes: None, supertypes: None, excluded_supertypes: None }
}

#[test]
fn set_card_types_overwrites_existing_types_entirely() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let target_land = ObjectId(0);
    put_on_battlefield(&mut state, land(target_land, p0));
    state.continuous_effects.push(continuous_effect(
        target_land,
        Layer::TypeChanging,
        EffectPrimitive::SetCardTypes(CardTypeSet::CREATURE),
        0,
    ));

    let effective = effective_characteristics(&state, &ScriptRuntime::new(), target_land);

    assert_eq!(effective.card_types, CardTypeSet::CREATURE, "Land should be gone entirely, not just gained Creature alongside it");
}

#[test]
fn set_colors_overwrites_existing_colors_entirely() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let target = ObjectId(0);
    let mut white_creature = creature(target, p0, 2, 2);
    white_creature.definition.color_indicator = Some(ColorSet::WHITE);
    put_on_battlefield(&mut state, white_creature);
    state.continuous_effects.push(continuous_effect(target, Layer::ColorChanging, EffectPrimitive::SetColors(ColorSet::BLACK), 0));

    let effective = effective_characteristics(&state, &ScriptRuntime::new(), target);

    assert_eq!(effective.colors, ColorSet::BLACK, "White should be gone entirely, not just gained Black alongside it");
}

#[test]
fn later_timestamp_set_card_types_wins_over_an_earlier_add_card_type() {
    // CR 613.7: effects in one layer fold sequentially in timestamp order, so a later SetCardTypes wipes out an earlier AddCardType.
    let mut state = base_state();
    let p0 = PlayerId(0);
    let target_land = ObjectId(0);
    put_on_battlefield(&mut state, land(target_land, p0));
    state.continuous_effects.push(continuous_effect(
        target_land,
        Layer::TypeChanging,
        EffectPrimitive::AddCardType(CardTypeSet::CREATURE),
        0,
    ));
    state.continuous_effects.push(continuous_effect(
        target_land,
        Layer::TypeChanging,
        EffectPrimitive::SetCardTypes(CardTypeSet::ARTIFACT),
        1,
    ));

    let effective = effective_characteristics(&state, &ScriptRuntime::new(), target_land);

    assert_eq!(effective.card_types, CardTypeSet::ARTIFACT, "the later SetCardTypes replaces Land+Creature entirely, leaving only Artifact");
}

#[test]
fn an_earlier_set_card_types_is_still_built_on_by_a_later_add_card_type() {
    // The reverse order: an AddCardType after an earlier SetCardTypes stacks on top of it.
    let mut state = base_state();
    let p0 = PlayerId(0);
    let target_land = ObjectId(0);
    put_on_battlefield(&mut state, land(target_land, p0));
    state.continuous_effects.push(continuous_effect(
        target_land,
        Layer::TypeChanging,
        EffectPrimitive::SetCardTypes(CardTypeSet::ARTIFACT),
        0,
    ));
    state.continuous_effects.push(continuous_effect(
        target_land,
        Layer::TypeChanging,
        EffectPrimitive::AddCardType(CardTypeSet::CREATURE),
        1,
    ));

    let effective = effective_characteristics(&state, &ScriptRuntime::new(), target_land);

    assert_eq!(effective.card_types, CardTypeSet::ARTIFACT | CardTypeSet::CREATURE);
}

#[test]
fn add_supertype_unions_a_supertype_in_without_removing_existing_ones() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let target = ObjectId(0);
    let mut snow_creature = creature(target, p0, 2, 2);
    snow_creature.definition.supertypes = SupertypeSet::SNOW;
    put_on_battlefield(&mut state, snow_creature);
    state.continuous_effects.push(continuous_effect(
        target,
        Layer::TypeChanging,
        EffectPrimitive::AddSupertype(SupertypeSet::LEGENDARY),
        0,
    ));

    let effective = effective_characteristics(&state, &ScriptRuntime::new(), target);

    assert!(effective.supertypes.contains(SupertypeSet::SNOW), "should keep Snow, not lose it");
    assert!(effective.supertypes.contains(SupertypeSet::LEGENDARY), "should gain Legendary");
}

#[test]
fn add_subtype_unions_a_subtype_in_without_removing_existing_ones() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let target = ObjectId(0);
    put_on_battlefield(&mut state, creature(target, p0, 2, 2));
    state.continuous_effects.push(continuous_effect(
        target,
        Layer::TypeChanging,
        EffectPrimitive::AddSubtype(vec![Subtype("Elemental".to_string())]),
        0,
    ));

    let effective = effective_characteristics(&state, &ScriptRuntime::new(), target);

    assert!(effective.subtypes.contains(&Subtype("Bear".to_string())), "should keep Bear, not lose it");
    assert!(effective.subtypes.contains(&Subtype("Elemental".to_string())), "should gain Elemental");
    assert_eq!(effective.subtypes.len(), 2, "no duplicate entries");
}

#[test]
fn add_subtype_does_not_duplicate_a_subtype_already_present() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let target = ObjectId(0);
    put_on_battlefield(&mut state, creature(target, p0, 2, 2));
    state.continuous_effects.push(continuous_effect(
        target,
        Layer::TypeChanging,
        EffectPrimitive::AddSubtype(vec![Subtype("Bear".to_string())]),
        0,
    ));

    let effective = effective_characteristics(&state, &ScriptRuntime::new(), target);

    assert_eq!(effective.subtypes, vec![Subtype("Bear".to_string())]);
}

#[test]
fn legend_rule_recognizes_a_continuous_effect_granted_legendary_supertype() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let printed_legend = ObjectId(0);
    let granted_legend = ObjectId(1);
    let mut a = creature(printed_legend, p0, 2, 2);
    a.definition.name = "Test Legend".to_string();
    a.definition.supertypes = SupertypeSet::LEGENDARY;
    put_on_battlefield(&mut state, a);
    let mut b = creature(granted_legend, p0, 3, 3);
    b.definition.name = "Test Legend".to_string();
    put_on_battlefield(&mut state, b);
    // An unrelated static ability grants Legendary to the second copy, which is not printed on it.
    state.continuous_effects.push(continuous_effect(
        granted_legend,
        Layer::TypeChanging,
        EffectPrimitive::AddSupertype(SupertypeSet::LEGENDARY),
        0,
    ));

    let outcome = check_state_based_actions(&mut state, &ScriptRuntime::new());

    match outcome {
        SbaOutcome::NeedsLegendRuleChoice { candidates, .. } => {
            assert_eq!(candidates.len(), 2, "both same-named legendaries (one printed, one granted) should be offered");
        }
        _ => panic!("expected a legend-rule choice pause now that both copies are effectively legendary"),
    }
}

const ANIMATE_LAND_SCRIPT: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateStaticAbility(c)
    e:SetLayer(Game.Layers.TypeChanging)
    e:SetEffect(Game.Effects.SetCardTypes({"Artifact", "Creature"}))
    e:SetAffected({ self_only = true })
    e:Register()

    local supertype = Game.CreateStaticAbility(c)
    supertype:SetLayer(Game.Layers.TypeChanging)
    supertype:SetEffect(Game.Effects.AddSupertype("Legendary"))
    supertype:SetAffected({ self_only = true })
    supertype:Register()

    local subtype = Game.CreateStaticAbility(c)
    subtype:SetLayer(Game.Layers.TypeChanging)
    subtype:SetEffect(Game.Effects.AddSubtype("Golem"))
    subtype:SetAffected({ self_only = true })
    subtype:Register()

    local color = Game.CreateStaticAbility(c)
    color:SetLayer(Game.Layers.ColorChanging)
    color:SetEffect(Game.Effects.SetColors("U"))
    color:SetAffected({ self_only = true })
    color:Register()
end
return s
"#;

#[test]
fn game_effects_set_card_types_and_friends_self_register_and_resolve_for_real() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let target_land = ObjectId(0);
    let mut definition = land_definition();
    definition.script_ref = ScriptReference::Inline(ANIMATE_LAND_SCRIPT.to_string());
    state.objects.insert(target_land, card_object(target_land, definition.clone(), p0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(target_land);

    let mut runtime = ScriptRuntime::new();
    runtime.run_card_script(&mut state, &definition, target_land).expect("script self-registers cleanly");

    let effective = effective_characteristics(&state, &runtime, target_land);
    assert_eq!(effective.card_types, CardTypeSet::ARTIFACT | CardTypeSet::CREATURE);
    assert!(effective.supertypes.contains(SupertypeSet::LEGENDARY));
    assert!(effective.subtypes.contains(&Subtype("Golem".to_string())));
    assert_eq!(effective.colors, ColorSet::BLUE);
}
