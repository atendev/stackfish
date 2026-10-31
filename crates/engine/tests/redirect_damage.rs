//! CR 614.9: `Game.ReplacementPayloads.RedirectDamageToSource` is the Lua path for `ReplacementPayload::RedirectDamage(Target)`, narrowed to redirecting to the effect's own source (Pariah).
//! The CR 614.9 destination-validity clause is tested at `matches()` level in `types/effect.rs`, since an eliminated-player destination needs a `Target::Player` this binding never produces.

use std::collections::HashMap;

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{
    card_object, drain, hand_battlefield_graveyard_zones,
    two_player_state as base_two_player_state, zones,
};

const REDIRECT_DAMAGE_TO_SELF: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateReplacementEffect(c)
    e:SetFilter(Game.EventFilters.DamageDealt)
    e:SetPayload(Game.ReplacementPayloads.RedirectDamageToSource)
    e:SetAffected(Game.Filter({ controller = "you" }))
    e:Register()
end
return s
"#;

const MISMATCHED_WOULD_DIE_REDIRECT: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateReplacementEffect(c)
    e:SetFilter(Game.EventFilters.WouldDie)
    e:SetPayload(Game.ReplacementPayloads.RedirectDamageToSource)
    e:Register()
end
return s
"#;

fn creature_definition(name: &str, power: i32, toughness: i32, script: &str) -> CardDefinition {
    CardDefinition {
        definition_id: 1,
        name: name.to_string(),
        mana_cost: ManaCost {
            symbols: Vec::new(),
        },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::CREATURE,
        subtypes: Vec::new(),
        oracle_text: "Redirect-damage test fixture.".to_string(),
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
    CardDefinition {
        script_ref: ScriptReference::ByKey(name.to_string()),
        ..creature_definition(name, power, toughness, "")
    }
}

fn permanent(id: ObjectId, definition: CardDefinition, owner: PlayerId) -> GameObject {
    card_object(id, definition, owner, ZoneKind::Battlefield)
}

fn put_on_battlefield(state: &mut GameState, object: GameObject) {
    let id = object.object_id;
    state
        .zone_mut(ZoneKind::Battlefield, None)
        .unwrap()
        .push_bottom(id);
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
fn set_payload_redirect_damage_to_source_registers_with_own_source_as_target() {
    let mut state = base_state();
    let source = ObjectId(0);
    let definition = creature_definition("Pariah", 0, 4, REDIRECT_DAMAGE_TO_SELF);
    state
        .objects
        .insert(source, permanent(source, definition.clone(), PlayerId(0)));

    let mut runtime = ScriptRuntime::new();
    runtime
        .run_card_script(&mut state, &definition, source)
        .expect("script self-registers cleanly");

    assert_eq!(state.replacement_effects.len(), 1);
    match &state.replacement_effects[0].spec {
        ReplacementSpec::Primitive {
            filter: EventFilter::DamageDealt { affected },
            payload: ReplacementPayload::RedirectDamage(Target::Object(target)),
        } => {
            assert_eq!(
                *target, source,
                "RedirectDamageToSource must resolve to the registering permanent's own id"
            );
            assert_eq!(affected.controller, Some(FilterController::You));
        }
        other => panic!(
            "expected a DamageDealt/RedirectDamage(Object(source)) primitive spec, got {other:?}"
        ),
    }
}

#[test]
fn mismatched_would_die_filter_and_redirect_damage_payload_is_a_real_register_time_error() {
    let mut state = base_state();
    let source = ObjectId(0);
    let definition = creature_definition("Broken", 2, 2, MISMATCHED_WOULD_DIE_REDIRECT);
    state
        .objects
        .insert(source, permanent(source, definition.clone(), PlayerId(0)));

    let mut runtime = ScriptRuntime::new();
    let result = runtime.run_card_script(&mut state, &definition, source);

    assert!(
        result.is_err(),
        "WouldDie + RedirectDamage is a real mismatch, must be rejected"
    );
    assert!(
        state.replacement_effects.is_empty(),
        "no partial effect should have been committed"
    );
}

fn assign_combat_damage(
    engine: &mut Engine,
    player: PlayerId,
    assignments: Vec<(ObjectId, Vec<(ObjectId, u32)>)>,
) -> StepStatus {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::AssignCombatDamage { assignments },
        })
        .expect("turn-based action, no priority required");
    drain(engine)
}

#[test]
fn lua_registered_redirect_damage_sends_combat_damage_to_shield_instead_of_player() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let shield_source = ObjectId(1);
    let mut state = combat_damage_state(CombatState {
        attackers: vec![(attacker, Target::Player(p1))],
        blocks: Vec::new(),
        declared_defenders: vec![p1],
        bands: Vec::new(),
        effect_blocked: Vec::new(),
        damage_assigned_this_step: false,
        pending_damage_splits: std::collections::HashMap::new(),
    });
    put_on_battlefield(
        &mut state,
        permanent(attacker, vanilla_creature_definition("Bear", 3, 3), p0),
    );
    let shield_definition = creature_definition("Pariah", 0, 4, REDIRECT_DAMAGE_TO_SELF);
    put_on_battlefield(
        &mut state,
        permanent(shield_source, shield_definition.clone(), p1),
    );

    ScriptRuntime::new()
        .run_card_script(&mut state, &shield_definition, shield_source)
        .expect("script self-registers cleanly");
    assert_eq!(state.replacement_effects.len(), 1);

    let mut engine = Engine::new(state);
    assign_combat_damage(&mut engine, p0, Vec::new());

    assert_eq!(
        engine.state().player(p1).unwrap().life,
        20,
        "the Lua-registered redirect must have kept the damage off the player entirely"
    );
    assert_eq!(
        engine
            .state()
            .object(shield_source)
            .map(|o| o.damage_marked),
        Some(3),
        "the redirected damage must have actually landed on the shield instead"
    );
    assert!(
        !engine.state().replacement_effects.is_empty(),
        "Repeatable (the default consumption) must not be consumed by one redirect"
    );
}
