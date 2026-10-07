//! Phase 5 vertical slice: static abilities, replacement effects and
//! player-level modifiers that function from the graveyard or exile
//! (CR 113.6, Anger-style cards).
//!
//! `:SetFunctionsFrom("Graveyard"|"Exile")` is accepted on the static,
//! replacement and modifier builders too. A declared registration is kept only
//! while its source is in that zone (and not on the battlefield); an
//! undeclared one still only functions on the battlefield. A zone change
//! mints a new object id (CR 400.7), so `retire_object`'s purge-by-source
//! removes the effect when the card leaves.

use stackfish_engine::engine::Engine;
use stackfish_engine::layers::effective_characteristics;
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, minimal_card_definition, two_player_state as base_two_player_state, zones};

fn haste_static(zone_call: &str) -> String {
    format!(
        r#"
local s = {{}}
function s.register_abilities(c)
    local e = Game.CreateStaticAbility(c)
    {zone_call}
    e:SetLayer(Game.Layers.AbilityAdding)
    e:SetEffect(Game.Effects.AddKeyword("Haste"))
    e:SetAffected({{ controller = "you", card_types = {{"Creature"}} }})
    e:Register()
end
return s
"#
    )
}

fn replacement_script(zone_call: &str) -> String {
    format!(
        r#"
local s = {{}}
function s.register_abilities(c)
    local e = Game.CreateReplacementEffect(c)
    {zone_call}
    e:SetFilter(Game.EventFilters.WouldDie)
    e:SetPayload(Game.ReplacementPayloads.Regenerate)
    e:SetAffected({{ self_only = true }})
    e:Register()
end
return s
"#
    )
}

fn modifier_script(zone_call: &str) -> String {
    format!(
        r#"
local s = {{}}
function s.register_abilities(c)
    local m = Game.CreateLandPlayModifier(c)
    {zone_call}
    m:SetAffected(Game.LandPlayAffected.You)
    m:SetAdditional(1)
    m:Register()
end
return s
"#
    )
}

fn scripted(script: &str) -> CardDefinition {
    CardDefinition {
        script_ref: ScriptReference::Inline(script.to_string()),
        ..minimal_card_definition(1, "Scripted", "unused", ManaCost { symbols: Vec::new() }, CardTypeSet::ENCHANTMENT)
    }
}

fn creature_definition() -> CardDefinition {
    minimal_card_definition(2, "Bear", "unused", ManaCost { symbols: Vec::new() }, CardTypeSet::CREATURE)
}

const SOURCE: ObjectId = ObjectId(0);
const CREATURE: ObjectId = ObjectId(1);

/// `script`'s card sits in `zone`; a vanilla creature sits on the battlefield.
fn state_with(zone: ZoneKind, script: &str) -> GameState {
    let p0 = PlayerId(0);
    let mut state = GameState {
        step: Step::PreCombatMain,
        ..base_two_player_state(zones(
            p0,
            PlayerId(1),
            &[ZoneKind::Hand, ZoneKind::Library, ZoneKind::Battlefield, ZoneKind::Graveyard, ZoneKind::Exile],
        ))
    };
    state.objects.insert(SOURCE, card_object(SOURCE, scripted(script), p0, zone));
    state.objects.insert(CREATURE, card_object(CREATURE, creature_definition(), p0, ZoneKind::Battlefield));
    for (id, z) in [(SOURCE, zone), (CREATURE, ZoneKind::Battlefield)] {
        match z {
            ZoneKind::Exile | ZoneKind::Battlefield => state.zone_mut(z, None),
            other => state.zone_mut(other, Some(p0)),
        }
        .expect("zone exists")
        .push_bottom(id);
    }
    state
}

fn run_script(state: &mut GameState) {
    let definition = state.object(SOURCE).unwrap().definition.clone();
    ScriptRuntime::new().run_card_script(state, &definition, SOURCE).unwrap();
}

fn has_haste(state: &GameState) -> bool {
    effective_characteristics(state, &ScriptRuntime::new(), CREATURE).keywords.contains(KeywordSet::HASTE)
}

fn pass_both(engine: &mut Engine) {
    for player in [PlayerId(0), PlayerId(1)] {
        engine.submit_action(PlayerAction { player, kind: PlayerActionKind::PassPriority }).unwrap();
        drain(engine);
    }
}

#[test]
fn a_graveyard_declared_static_functions_from_the_graveyard() {
    let mut state = state_with(ZoneKind::Graveyard, &haste_static(r#"e:SetFunctionsFrom("Graveyard")"#));
    run_script(&mut state);
    assert_eq!(state.continuous_effects.len(), 1);
    assert!(has_haste(&state), "the graveyard static grants haste to a creature you control");
}

#[test]
fn the_engine_sweep_registers_a_graveyard_static_and_a_second_pass_does_not_duplicate_it() {
    let mut engine = Engine::new(state_with(ZoneKind::Graveyard, &haste_static(r#"e:SetFunctionsFrom("Graveyard")"#)));
    pass_both(&mut engine);
    assert_eq!(engine.state().continuous_effects.len(), 1);
    assert!(has_haste(engine.state()));
    pass_both(&mut engine);
    assert_eq!(engine.state().continuous_effects.len(), 1);
}

#[test]
fn an_exile_declared_static_functions_from_exile() {
    let mut state = state_with(ZoneKind::Exile, &haste_static(r#"e:SetFunctionsFrom("Exile")"#));
    run_script(&mut state);
    assert!(has_haste(&state));
}

#[test]
fn an_undeclared_static_is_still_discarded_in_the_graveyard() {
    let mut state = state_with(ZoneKind::Graveyard, &haste_static(""));
    run_script(&mut state);
    assert!(state.continuous_effects.is_empty());
    assert!(!has_haste(&state));
}

#[test]
fn a_declared_static_does_not_register_off_its_zone() {
    for zone in [ZoneKind::Hand, ZoneKind::Battlefield, ZoneKind::Exile] {
        let mut state = state_with(zone, &haste_static(r#"e:SetFunctionsFrom("Graveyard")"#));
        run_script(&mut state);
        assert!(state.continuous_effects.is_empty(), "a Graveyard-only static must not register in {zone:?}");
    }
}

#[test]
fn an_undeclared_static_still_registers_on_the_battlefield() {
    let mut state = state_with(ZoneKind::Battlefield, &haste_static(""));
    run_script(&mut state);
    assert_eq!(state.continuous_effects.len(), 1);
}

const EXILER: &str = r#"
local s = {}
function s.register_abilities(c)
    local a = Game.CreateActivatedAbility(c)
    a:SetFunctionsFrom("Graveyard")
    a:AddCost(Game.Costs.ExileFromGraveyard({}, 1))
    a:SetOperation(Game.Ops.GainLife(1))
    a:Register()
end
return s
"#;

#[test]
fn the_static_is_gone_once_the_source_leaves_the_graveyard() {
    // CR 400.7: exiling the static's card (as another graveyard card's cost)
    // makes a new object; the old id's effect is purged and, since the script
    // declared only Graveyard, nothing re-registers for the exile copy.
    let exiler = ObjectId(2);
    let mut state = state_with(ZoneKind::Graveyard, &haste_static(r#"e:SetFunctionsFrom("Graveyard")"#));
    state.objects.insert(exiler, card_object(exiler, scripted(EXILER), PlayerId(0), ZoneKind::Graveyard));
    state.zone_mut(ZoneKind::Graveyard, Some(PlayerId(0))).unwrap().push_bottom(exiler);
    let mut engine = Engine::new(state);
    pass_both(&mut engine);
    assert_eq!(engine.state().continuous_effects.len(), 1);
    assert!(has_haste(engine.state()));

    engine
        .submit_action(PlayerAction {
            player: PlayerId(0),
            kind: PlayerActionKind::ActivateAbility {
                source: exiler,
                ability_index: 0,
                targets: Vec::new(),
                chosen_colors: Vec::new(),
                x_value: None,
            },
        })
        .expect("legal");
    drain(&mut engine);
    engine.submit_payment(PaymentPlan { payments: vec![CostPayment::Exile(SOURCE)] }).expect("exile the static's card");
    drain(&mut engine);

    assert!(engine.state().object(SOURCE).is_none());
    assert!(engine.state().continuous_effects.is_empty());
    assert!(!has_haste(engine.state()));
}

#[test]
fn a_graveyard_declared_replacement_effect_registers_in_the_graveyard_only() {
    let script = replacement_script(r#"e:SetFunctionsFrom("Graveyard")"#);
    let mut graveyard = state_with(ZoneKind::Graveyard, &script);
    run_script(&mut graveyard);
    assert_eq!(graveyard.replacement_effects.len(), 1);
    assert_eq!(graveyard.replacement_effects[0].source, SOURCE);

    let mut battlefield = state_with(ZoneKind::Battlefield, &script);
    run_script(&mut battlefield);
    assert!(battlefield.replacement_effects.is_empty());

    let mut undeclared = state_with(ZoneKind::Graveyard, &replacement_script(""));
    run_script(&mut undeclared);
    assert!(undeclared.replacement_effects.is_empty());
}

#[test]
fn a_graveyard_declared_modifier_registers_with_the_real_controller() {
    let script = modifier_script(r#"m:SetFunctionsFrom("Graveyard")"#);
    let mut state = state_with(ZoneKind::Graveyard, &script);
    run_script(&mut state);
    assert_eq!(state.land_play_modifiers.len(), 1);
    assert_eq!(state.land_play_modifiers[0].controller, PlayerId(0));
    assert_eq!(effective_land_play_limit(&state.land_play_modifiers, PlayerId(0)), 2);

    let mut undeclared = state_with(ZoneKind::Graveyard, &modifier_script(""));
    run_script(&mut undeclared);
    assert!(undeclared.land_play_modifiers.is_empty());
}

#[test]
fn a_hand_function_zone_is_still_a_clear_error() {
    let mut state = state_with(ZoneKind::Graveyard, &haste_static(r#"e:SetFunctionsFrom("Hand")"#));
    let definition = state.object(SOURCE).unwrap().definition.clone();
    assert!(ScriptRuntime::new().run_card_script(&mut state, &definition, SOURCE).is_err());
    assert!(state.continuous_effects.is_empty());
}
