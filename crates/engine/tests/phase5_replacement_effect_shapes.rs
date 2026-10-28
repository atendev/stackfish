//! `Game.CreateReplacementEffect` for `DamageDealt` with `PreventDamage` (CR 615) and `WouldDie` with `ReplaceZoneDestination` (CR 614.13), proven end to end here. `RedirectDamage(Target)` stays unbound (see `ReplacementEffectBuilder` in script.rs).

use std::collections::HashMap;

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{card_object, hand_battlefield_graveyard_zones, zones, two_player_state as base_two_player_state};

const PREVENT_DAMAGE_TO_YOU_ONE_SHOT: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateReplacementEffect(c)
    e:SetFilter(Game.EventFilters.DamageDealt)
    e:SetPayload(Game.ReplacementPayloads.PreventDamage)
    e:SetConsumption(Game.Consumption.OneShot)
    e:SetAffected({ controller = "you" })
    e:Register()
end
return s
"#;

const EXILE_INSTEAD_OF_DYING: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateReplacementEffect(c)
    e:SetFilter(Game.EventFilters.WouldDie)
    e:SetPayload(Game.ReplacementPayloads.ReplaceZoneDestination("Exile"))
    e:SetAffected({ self_only = true })
    e:Register()
end
return s
"#;

const MISMATCHED_FILTER_PAYLOAD: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateReplacementEffect(c)
    e:SetFilter(Game.EventFilters.DamageDealt)
    e:SetPayload(Game.ReplacementPayloads.ReplaceZoneDestination("Exile"))
    e:Register()
end
return s
"#;

const LIMITED_USES_SHIELD: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateReplacementEffect(c)
    e:SetFilter(Game.EventFilters.DamageDealt)
    e:SetPayload(Game.ReplacementPayloads.PreventDamage)
    e:SetConsumption(Game.Consumption.LimitedUses(1))
    e:SetAffected({ controller = "you" })
    e:Register()
end
return s
"#;

const REGENERATE_DEFAULTS_UNCHANGED: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateReplacementEffect(c)
    e:SetAffected({ self_only = true })
    e:Register()
end
return s
"#;

fn creature_definition(name: &str, power: i32, toughness: i32, script: &str) -> CardDefinition {
    CardDefinition {
        definition_id: 1,
        name: name.to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::CREATURE,
        subtypes: Vec::new(),
        oracle_text: "Replacement-effect-shapes test fixture.".to_string(),
        power: Some(PTValue::Fixed(power)),
        toughness: Some(PTValue::Fixed(toughness)),
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::Inline(script.to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn vanilla_creature_definition(name: &str, power: i32, toughness: i32) -> CardDefinition {
    CardDefinition { script_ref: ScriptReference::ByKey(name.to_string()), ..creature_definition(name, power, toughness, "") }
}

fn permanent(id: ObjectId, definition: CardDefinition, owner: PlayerId) -> GameObject {
    card_object(id, definition, owner, ZoneKind::Battlefield)
}

fn put_on_battlefield(state: &mut GameState, object: GameObject) {
    let id = object.object_id;
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(id);
    state.objects.insert(id, object);
}

fn combat_damage_state(combat: CombatState) -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        phase: Phase::Combat,
        step: Step::CombatDamage,
        combat: Some(combat),
        last_active_turn_number: HashMap::from([(p0, 1), (p1, 0)]),
        ..base_two_player_state(zones(p0, p1, &[ZoneKind::Battlefield]))
    }
}

fn base_state() -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        step: Step::PreCombatMain,
        last_active_turn_number: HashMap::from([(p0, 1), (p1, 1)]),
        ..base_two_player_state({
            let mut z = hand_battlefield_graveyard_zones(p0, p1);
            z.push(Zone::new(ZoneKind::Exile, None));
            z
        })
    }
}

#[test]
fn set_filter_damage_dealt_and_prevent_damage_registers_correctly() {
    let mut state = base_state();
    let source = ObjectId(0);
    let definition = creature_definition("Ward", 0, 1, PREVENT_DAMAGE_TO_YOU_ONE_SHOT);
    state.objects.insert(source, permanent(source, definition.clone(), PlayerId(0)));

    let mut runtime = ScriptRuntime::new();
    runtime.run_card_script(&mut state, &definition, source).expect("script self-registers cleanly");

    assert_eq!(state.replacement_effects.len(), 1);
    let effect = &state.replacement_effects[0];
    assert_eq!(effect.consumption, ConsumptionPolicy::OneShot);
    match &effect.spec {
        ReplacementSpec::Primitive {
            filter: EventFilter::DamageDealt { affected },
            payload: ReplacementPayload::PreventDamage,
        } => {
            assert_eq!(affected.controller, Some(FilterController::You));
        }
        other => panic!("expected a DamageDealt/PreventDamage primitive spec, got {other:?}"),
    }
}

#[test]
fn set_filter_would_die_and_replace_zone_destination_registers_correctly() {
    let mut state = base_state();
    let source = ObjectId(0);
    let definition = creature_definition("Vanishes", 2, 2, EXILE_INSTEAD_OF_DYING);
    state.objects.insert(source, permanent(source, definition.clone(), PlayerId(0)));

    let mut runtime = ScriptRuntime::new();
    runtime.run_card_script(&mut state, &definition, source).expect("script self-registers cleanly");

    assert_eq!(state.replacement_effects.len(), 1);
    match &state.replacement_effects[0].spec {
        ReplacementSpec::Primitive {
            filter: EventFilter::WouldDie { affected },
            payload: ReplacementPayload::ReplaceZoneDestination(ZoneKind::Exile),
        } => {
            assert!(affected.self_only);
        }
        other => panic!("expected a WouldDie/ReplaceZoneDestination(Exile) primitive spec, got {other:?}"),
    }
}

#[test]
fn mismatched_filter_and_payload_is_a_real_register_time_error() {
    let mut state = base_state();
    let source = ObjectId(0);
    let definition = creature_definition("Broken", 2, 2, MISMATCHED_FILTER_PAYLOAD);
    state.objects.insert(source, permanent(source, definition.clone(), PlayerId(0)));

    let mut runtime = ScriptRuntime::new();
    let result = runtime.run_card_script(&mut state, &definition, source);

    assert!(result.is_err(), "DamageDealt + ReplaceZoneDestination is a real mismatch, must be rejected");
    assert!(state.replacement_effects.is_empty(), "no partial effect should have been committed");
}

#[test]
fn omitting_every_new_setter_still_defaults_to_would_die_regenerate_repeatable() {
    let mut state = base_state();
    let source = ObjectId(0);
    let definition = creature_definition("Bear", 2, 2, REGENERATE_DEFAULTS_UNCHANGED);
    state.objects.insert(source, permanent(source, definition.clone(), PlayerId(0)));

    let mut runtime = ScriptRuntime::new();
    runtime.run_card_script(&mut state, &definition, source).expect("script self-registers cleanly");

    assert_eq!(state.replacement_effects.len(), 1);
    let effect = &state.replacement_effects[0];
    assert_eq!(effect.consumption, ConsumptionPolicy::Repeatable);
    assert!(matches!(
        &effect.spec,
        ReplacementSpec::Primitive { filter: EventFilter::WouldDie { .. }, payload: ReplacementPayload::Regenerate }
    ));
}

fn drain(engine: &mut Engine) -> StepStatus {
    loop {
        match engine.step().status {
            StepStatus::Progressed => continue,
            other => return other,
        }
    }
}

fn assign_combat_damage(
    engine: &mut Engine,
    player: PlayerId,
    assignments: Vec<(ObjectId, Vec<(ObjectId, u32)>)>,
) -> StepStatus {
    engine
        .submit_action(PlayerAction { player, kind: PlayerActionKind::AssignCombatDamage { assignments } })
        .expect("turn-based action, no priority required");
    drain(engine)
}

#[test]
fn lua_registered_prevent_damage_stops_real_combat_damage_from_landing() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let shield_source = ObjectId(1);
    let mut state = combat_damage_state(CombatState {
        attackers: vec![(attacker, Target::Player(p1))],
        blocks: Vec::new(),
        declared_defenders: vec![p1],
        bands: Vec::new(), effect_blocked: Vec::new(), damage_assigned_this_step: false, pending_damage_splits: std::collections::HashMap::new(),
    });
    put_on_battlefield(&mut state, permanent(attacker, vanilla_creature_definition("Bear", 3, 3), p0));
    let shield_definition = creature_definition("Ward", 0, 1, PREVENT_DAMAGE_TO_YOU_ONE_SHOT);
    put_on_battlefield(&mut state, permanent(shield_source, shield_definition.clone(), p1));

    // A throwaway ScriptRuntime is enough, since ReplacementSpec::Primitive carries no runtime-bound handle. Self-registration is proven in phase5_static_ability_self_registration.rs.
    ScriptRuntime::new()
        .run_card_script(&mut state, &shield_definition, shield_source)
        .expect("script self-registers cleanly");
    assert_eq!(state.replacement_effects.len(), 1);

    let mut engine = Engine::new(state);
    assign_combat_damage(&mut engine, p0, Vec::new());

    assert_eq!(
        engine.state().player(p1).unwrap().life,
        20,
        "the Lua-registered PreventDamage effect must have stopped the damage from landing"
    );
    assert!(
        engine.state().replacement_effects.is_empty(),
        "a OneShot shield is consumed after use"
    );
}

#[test]
fn lua_registered_replace_zone_destination_exiles_instead_of_dying() {
    let p0 = PlayerId(0);
    let bear = ObjectId(0);
    let mut state = base_state();
    let definition = creature_definition("Vanishes", 2, 2, EXILE_INSTEAD_OF_DYING);
    put_on_battlefield(&mut state, permanent(bear, definition.clone(), p0));

    ScriptRuntime::new().run_card_script(&mut state, &definition, bear).expect("script self-registers cleanly");
    assert_eq!(state.replacement_effects.len(), 1);
    state.object_mut(bear).unwrap().damage_marked = 2;

    let script_runtime = ScriptRuntime::new();
    let outcome = stackfish_engine::sba::check_state_based_actions(&mut state, &script_runtime);
    let events = match outcome {
        stackfish_engine::sba::SbaOutcome::Progressed(events, _) => events,
        stackfish_engine::sba::SbaOutcome::NeedsOrderingChoice { .. } => {
            panic!("fixture didn't expect a replacement ordering-choice pause")
        }
        stackfish_engine::sba::SbaOutcome::NeedsLegendRuleChoice { .. } => {
            panic!("fixture didn't expect a legend-rule choice pause")
        }
        stackfish_engine::sba::SbaOutcome::NeedsCommanderReturnChoice { .. } => {
            panic!("fixture didn't expect a commander-return choice pause")
        }
    };

    // CR 400.7: a death mints a fresh ObjectId, and old_id never remaps.
    let new_id = events
        .iter()
        .find_map(|e| match e.kind {
            GameEventKind::ZoneChange { object, from: ZoneKind::Battlefield, to: ZoneKind::Exile, .. } => Some(object),
            _ => None,
        })
        .unwrap_or_else(|| panic!("expected the lethal-damage SBA to exile the creature instead of sending it to the graveyard, got {events:?}"));

    assert!(state.object(bear).is_none(), "the old id shouldn't resolve anymore");
    assert_eq!(
        state.object(new_id).map(|o| o.zone),
        Some(ZoneKind::Exile),
        "the creature should actually be in exile now, not just logged as such"
    );
}

#[test]
fn lua_registered_limited_uses_one_shield_stops_protecting_after_first_use() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let shield_source = ObjectId(1);
    let mut state = combat_damage_state(CombatState {
        attackers: vec![(attacker, Target::Player(p1))],
        blocks: Vec::new(),
        declared_defenders: vec![p1],
        bands: Vec::new(), effect_blocked: Vec::new(), damage_assigned_this_step: false, pending_damage_splits: std::collections::HashMap::new(),
    });
    put_on_battlefield(&mut state, permanent(attacker, vanilla_creature_definition("Bear", 3, 3), p0));
    let shield_definition = creature_definition("Ward", 0, 1, LIMITED_USES_SHIELD);
    put_on_battlefield(&mut state, permanent(shield_source, shield_definition.clone(), p1));
    ScriptRuntime::new()
        .run_card_script(&mut state, &shield_definition, shield_source)
        .expect("script self-registers cleanly");

    let mut engine = Engine::new(state);
    assign_combat_damage(&mut engine, p0, Vec::new());

    assert_eq!(engine.state().player(p1).unwrap().life, 20, "the first hit should be prevented");
    assert!(
        engine.state().replacement_effects.is_empty(),
        "LimitedUses(1) should be fully consumed after its one use, same as OneShot"
    );
}
