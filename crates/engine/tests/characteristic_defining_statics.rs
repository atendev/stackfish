//! CR 604.3: `StaticAbilityBuilder:SetCharacteristicDefining()` marks a layer 4 subtype or layer 5 color static as a CDA that registers in every zone, validated against CR 604.3a at `:Register()`.
//! The last test adds the "as long as you control a Mountain" condition to a graveyard static.

use stackfish_engine::engine::Engine;
use stackfish_engine::layers::effective_characteristics;
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, minimal_card_definition, two_player_state as base_two_player_state, zones};

fn cda_script(extra: &str, layer_and_effect: &str) -> String {
    format!(
        r#"
local s = {{}}
function s.register_abilities(c)
    local e = Game.CreateStaticAbility(c)
    {extra}
    {layer_and_effect}
    e:SetAffected(Game.Affected.Self)
    e:Register()
end
return s
"#
    )
}

const ALL_COLORS: &str = r#"e:SetLayer(Game.Layers.ColorChanging) e:SetEffect(Game.Effects.SetColors("WUBRG"))"#;
const ADD_CHANGELING_SUBTYPE: &str = r#"e:SetLayer(Game.Layers.TypeChanging) e:SetEffect(Game.Effects.AddSubtype("Shapeshifter"))"#;

fn scripted(script: &str) -> CardDefinition {
    CardDefinition {
        script_ref: ScriptReference::Inline(script.to_string()),
        ..minimal_card_definition(1, "Scripted", "unused", ManaCost { symbols: Vec::new() }, CardTypeSet::CREATURE)
    }
}

const SOURCE: ObjectId = ObjectId(0);

fn all_zones() -> Vec<ZoneKind> {
    vec![ZoneKind::Hand, ZoneKind::Library, ZoneKind::Battlefield, ZoneKind::Graveyard, ZoneKind::Exile]
}

fn state_with_source(zone: ZoneKind, script: &str) -> GameState {
    let p0 = PlayerId(0);
    let mut state = GameState {
        step: Step::PreCombatMain,
        ..base_two_player_state(zones(p0, PlayerId(1), &all_zones()))
    };
    state.objects.insert(SOURCE, card_object(SOURCE, scripted(script), p0, zone));
    // The stack has no zone container; every other zone does.
    if zone != ZoneKind::Stack {
        match zone {
            ZoneKind::Exile | ZoneKind::Battlefield => state.zone_mut(zone, None),
            other => state.zone_mut(other, Some(p0)),
        }
        .expect("zone exists")
        .push_bottom(SOURCE);
    }
    state
}

fn run_script(state: &mut GameState) -> Result<(), stackfish_engine::script::ScriptError> {
    let definition = state.object(SOURCE).unwrap().definition.clone();
    ScriptRuntime::new().run_card_script(state, &definition, SOURCE)
}

fn colors(state: &GameState) -> ColorSet {
    effective_characteristics(state, &ScriptRuntime::new(), SOURCE).colors
}

#[test]
fn a_cda_color_static_registers_and_applies_in_every_zone() {
    for zone in [ZoneKind::Hand, ZoneKind::Library, ZoneKind::Battlefield, ZoneKind::Graveyard, ZoneKind::Exile, ZoneKind::Stack] {
        let mut state = state_with_source(zone, &cda_script("e:SetCharacteristicDefining()", ALL_COLORS));
        run_script(&mut state).unwrap();
        assert_eq!(state.continuous_effects.len(), 1, "CDA must register in {zone:?}");
        assert_eq!(colors(&state), ColorSet::all(), "CDA must apply in {zone:?}");
    }
}

#[test]
fn a_cda_subtype_static_applies_off_the_battlefield() {
    let mut state = state_with_source(ZoneKind::Graveyard, &cda_script("e:SetCharacteristicDefining()", ADD_CHANGELING_SUBTYPE));
    run_script(&mut state).unwrap();
    let subtypes = effective_characteristics(&state, &ScriptRuntime::new(), SOURCE).subtypes;
    assert!(subtypes.iter().any(|s| s.0 == "Shapeshifter"));
}

#[test]
fn an_ordinary_static_is_still_discarded_off_the_battlefield() {
    for zone in [ZoneKind::Hand, ZoneKind::Graveyard, ZoneKind::Exile, ZoneKind::Stack] {
        let mut state = state_with_source(zone, &cda_script("", ALL_COLORS));
        run_script(&mut state).unwrap();
        assert!(state.continuous_effects.is_empty(), "a plain static must not register in {zone:?}");
    }
    let mut on_battlefield = state_with_source(ZoneKind::Battlefield, &cda_script("", ALL_COLORS));
    run_script(&mut on_battlefield).unwrap();
    assert_eq!(on_battlefield.continuous_effects.len(), 1);
}

#[test]
fn the_engine_sweep_registers_a_graveyard_cda_once() {
    let mut engine = Engine::new(state_with_source(ZoneKind::Graveyard, &cda_script("e:SetCharacteristicDefining()", ALL_COLORS)));
    for _ in 0..2 {
        for player in [PlayerId(0), PlayerId(1)] {
            engine.submit_action(PlayerAction { player, kind: PlayerActionKind::PassPriority }).unwrap();
            drain(&mut engine);
        }
    }
    assert_eq!(engine.state().continuous_effects.len(), 1);
    assert_eq!(colors(engine.state()), ColorSet::all());
}

#[test]
fn cr_604_3a_validation_rejects_non_cda_shapes() {
    let cases: [(&str, &str, &str); 5] = [
        ("keyword grant", "e:SetLayer(Game.Layers.AbilityAdding) e:SetEffect(Game.Effects.AddKeyword(\"Flying\"))", ""),
        ("condition", ALL_COLORS, "e:SetCondition(Game.Conditions.AtOrBelowLife(5))"),
        ("function zone", ALL_COLORS, "e:SetFunctionsFrom(\"Graveyard\")"),
        ("duration", ALL_COLORS, "e:SetDuration(Game.Durations.UntilEndOfTurn)"),
        ("card type", "e:SetLayer(Game.Layers.TypeChanging) e:SetEffect(Game.Effects.AddCardType(\"Artifact\"))", ""),
    ];
    for (name, layer_and_effect, extra) in cases {
        let mut state = state_with_source(ZoneKind::Battlefield, &cda_script(&format!("e:SetCharacteristicDefining() {extra}"), layer_and_effect));
        assert!(run_script(&mut state).is_err(), "{name} must be rejected as a CDA");
        assert!(state.continuous_effects.is_empty(), "{name}");
    }
}

#[test]
fn a_cda_must_affect_only_itself() {
    let script = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateStaticAbility(c)
    e:SetCharacteristicDefining()
    e:SetLayer(Game.Layers.ColorChanging)
    e:SetEffect(Game.Effects.SetColors("R"))
    e:SetAffected(Game.Filter({ controller = "you" }))
    e:Register()
end
return s
"#;
    let mut state = state_with_source(ZoneKind::Battlefield, script);
    assert!(run_script(&mut state).is_err());
}

const ANGER_LIKE: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateStaticAbility(c)
    e:SetFunctionsFrom("Graveyard")
    e:SetLayer(Game.Layers.AbilityAdding)
    e:SetEffect(Game.Effects.AddKeyword("Haste"))
    e:SetAffected(Game.Filter({ controller = "you", card_types = { "Creature" } }))
    e:SetCondition(Game.Conditions.ControlsAtLeast({ subtypes = { "Mountain" } }, 1))
    e:Register()
end
return s
"#;

const ANGER: ObjectId = ObjectId(10);
const BEAR: ObjectId = ObjectId(11);
const MOUNTAIN: ObjectId = ObjectId(12);

fn mountain_definition() -> CardDefinition {
    CardDefinition {
        subtypes: vec![Subtype("Mountain".to_string())],
        ..minimal_card_definition(3, "Mountain", "unused", ManaCost { symbols: Vec::new() }, CardTypeSet::LAND)
    }
}

fn anger_state() -> GameState {
    let p0 = PlayerId(0);
    let mut state = GameState {
        step: Step::PreCombatMain,
        ..base_two_player_state(zones(p0, PlayerId(1), &all_zones()))
    };
    let bear = minimal_card_definition(2, "Bear", "unused", ManaCost { symbols: Vec::new() }, CardTypeSet::CREATURE);
    state.objects.insert(ANGER, card_object(ANGER, scripted(ANGER_LIKE), p0, ZoneKind::Graveyard));
    state.zone_mut(ZoneKind::Graveyard, Some(p0)).unwrap().push_bottom(ANGER);
    state.objects.insert(BEAR, card_object(BEAR, bear, p0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(BEAR);
    state
}

fn bear_has_haste(state: &GameState) -> bool {
    effective_characteristics(state, &ScriptRuntime::new(), BEAR).keywords.contains(KeywordSet::HASTE)
}

#[test]
fn a_graveyard_static_with_a_mountain_condition_tracks_the_board() {
    let p0 = PlayerId(0);
    let mut engine = Engine::new(anger_state());
    for player in [PlayerId(0), PlayerId(1)] {
        engine.submit_action(PlayerAction { player, kind: PlayerActionKind::PassPriority }).unwrap();
        drain(&mut engine);
    }
    assert_eq!(engine.state().continuous_effects.len(), 1, "registered from the graveyard");
    assert!(!bear_has_haste(engine.state()), "no Mountain yet: the condition gate holds the effect off");

    // A Mountain arrives: the same registered effect now applies.
    let state = engine.state_mut();
    state.objects.insert(MOUNTAIN, card_object(MOUNTAIN, mountain_definition(), p0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(MOUNTAIN);
    assert!(bear_has_haste(engine.state()), "a Mountain satisfies the condition");

    // The Mountain leaves: the effect stops applying but stays registered.
    let state = engine.state_mut();
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().remove(MOUNTAIN);
    state.objects.remove(&MOUNTAIN);
    assert!(!bear_has_haste(engine.state()));
    assert_eq!(engine.state().continuous_effects.len(), 1);
}
