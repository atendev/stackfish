//! Phase 5 vertical slice: `Game.CreateTriggeredAbility(c)` gains a real
//! `:SetTarget(table | function)` method, mirroring `Game.
//! CreateActivatedAbility`'s own `:SetTarget` exactly (same
//! `decode_target_filter_value` decode, same single-exact-count narrowing -
//! `count: 1, up_to: false`, no CR 601.2c multi-word form). CR 603.3d's real
//! target-selection pause (`ProcessState`/`StepStatus::AwaitingTriggerTargets`,
//! `Engine::submit_trigger_targets`) has been engine-complete since
//! `phase5/trigger-target-selection`, but was only reachable by constructing
//! a `RegisteredAbility` directly in Rust - this slice closes that gap so an
//! actual card script can declare a targeted triggered ability too.
//!
//! Mode declaration (CR 603.3c) stays out of scope - confirmed with the user
//! first, since unlike `:SetTarget` there's no existing Lua modal vocabulary
//! anywhere to mirror (modal *spells* don't have one either).

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, minimal_card_definition, two_player_state as base_two_player_state, zones};

const SELF_REGISTER_DESTROY_TARGET_CREATURE: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateTriggeredAbility(c)
    e:SetCondition(Game.Conditions.Enters)
    e:SetTarget({ type = "object", card_types = { "Creature" } })
    e:SetOperation(Game.Ops.Destroy)
    e:Register()
end
return s
"#;

const SELF_REGISTER_NO_TARGET: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateTriggeredAbility(c)
    e:SetCondition(Game.Conditions.Enters)
    e:SetOperation(Game.Ops.GainLife(1))
    e:Register()
end
return s
"#;

const SELF_REGISTER_SCRIPTED_TARGET: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateTriggeredAbility(c)
    e:SetCondition(Game.Conditions.Enters)
    e:SetTarget(function(object, source, controller, state)
        return true
    end)
    e:SetOperation(Game.Ops.Destroy)
    e:Register()
end
return s
"#;

fn land_definition(script: &str) -> CardDefinition {
    CardDefinition {
        oracle_text: "When this land enters the battlefield, destroy target creature.".to_string(),
        script_ref: ScriptReference::Inline(script.to_string()),
        ..minimal_card_definition(1, "Test Land", "test_land", ManaCost { symbols: Vec::new() }, CardTypeSet::LAND)
    }
}

fn creature_definition() -> CardDefinition {
    minimal_card_definition(2, "Test Bear", "test_bear", ManaCost { symbols: Vec::new() }, CardTypeSet::CREATURE)
}

fn two_player_state() -> GameState {
    base_two_player_state(zones(PlayerId(0), PlayerId(1), &[ZoneKind::Hand, ZoneKind::Library, ZoneKind::Battlefield, ZoneKind::Graveyard]))
}

fn pass(engine: &mut Engine, player: PlayerId) {
    engine.submit_action(PlayerAction { player, kind: PlayerActionKind::PassPriority }).unwrap();
}

// ---------------------------------------------------------------------
// Unit-level: run_card_script's own decode of :SetTarget, without driving
// the whole Engine pipeline.
// ---------------------------------------------------------------------

#[test]
fn set_target_with_a_table_decodes_into_a_real_target_requirement() {
    let mut state = two_player_state();
    let land = ObjectId(0);
    let definition = land_definition(SELF_REGISTER_DESTROY_TARGET_CREATURE);
    state.objects.insert(land, card_object(land, definition.clone(), PlayerId(0), ZoneKind::Battlefield));

    let mut runtime = ScriptRuntime::new();
    runtime.run_card_script(&mut state, &definition, land).expect("script self-registers cleanly");

    assert_eq!(state.abilities.len(), 1);
    let ability = &state.abilities[0];
    let Some(TargetSpec::Primitive(requirement)) = &ability.target else {
        panic!("expected a single-word TargetSpec::Primitive, got {:?}", ability.target)
    };
    assert_eq!(requirement.count, 1);
    assert!(!requirement.up_to);
    assert!(matches!(
        requirement.filter,
        TargetFilter::Object { mana_value: None, card_types: Some(types), .. } if types == CardTypeSet::CREATURE
    ));
}

#[test]
fn omitting_set_target_still_registers_with_no_target_requirement() {
    let mut state = two_player_state();
    let land = ObjectId(0);
    let definition = land_definition(SELF_REGISTER_NO_TARGET);
    state.objects.insert(land, card_object(land, definition.clone(), PlayerId(0), ZoneKind::Battlefield));

    let mut runtime = ScriptRuntime::new();
    runtime.run_card_script(&mut state, &definition, land).expect("script self-registers cleanly");

    assert_eq!(state.abilities.len(), 1);
    assert!(state.abilities[0].target.is_none(), "no :SetTarget call means target: None, unchanged from before this slice");
}

#[test]
fn set_target_with_a_function_decodes_into_a_scripted_filter() {
    let mut state = two_player_state();
    let land = ObjectId(0);
    let definition = land_definition(SELF_REGISTER_SCRIPTED_TARGET);
    state.objects.insert(land, card_object(land, definition.clone(), PlayerId(0), ZoneKind::Battlefield));

    let mut runtime = ScriptRuntime::new();
    runtime.run_card_script(&mut state, &definition, land).expect("script self-registers cleanly");

    assert_eq!(state.abilities.len(), 1);
    let Some(TargetSpec::Primitive(requirement)) = &state.abilities[0].target else {
        panic!("expected a single-word TargetSpec::Primitive")
    };
    assert!(
        matches!(requirement.filter, TargetFilter::Script(_)),
        "a bare function passed to :SetTarget must decode to TargetFilter::Script, same as ActivatedAbilityBuilder's own :SetTarget"
    );
}

// ---------------------------------------------------------------------
// End-to-end: a real card script's targeted triggered ability actually
// fires, pauses for a real CR 603.3d choice, and resolves against it.
// ---------------------------------------------------------------------

#[test]
fn play_land_self_registering_targeted_ability_fires_pauses_and_resolves_end_to_end() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let land = ObjectId(0);
    let bear = ObjectId(1);
    let mut state = two_player_state();
    state.objects.insert(land, card_object(land, land_definition(SELF_REGISTER_DESTROY_TARGET_CREATURE), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(land);
    state.objects.insert(bear, card_object(bear, creature_definition(), p1, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(bear);

    let mut engine = Engine::new(state);
    engine
        .submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PlayLand { card: land } })
        .expect("p0 holds priority, land is in hand");
    let status = drain(&mut engine);

    // CR 603.3d: the self-registered ability's own real target requirement
    // (declared via :SetTarget, not hand-built in Rust) pauses for a real
    // choice before it ever reaches the stack.
    let StepStatus::AwaitingTriggerTargets { player, candidates, .. } = status else {
        panic!("expected the script-declared target requirement to raise a real CR 603.3d pause, got {status:?}")
    };
    assert_eq!(player, p0);
    assert_eq!(candidates, vec![vec![Target::Object(bear)]]);
    assert_eq!(engine.state().stack.len(), 0, "not pushed yet - still waiting on the choice");

    engine.submit_trigger_targets(vec![Target::Object(bear)]).expect("Bear was the offered candidate");
    drain(&mut engine);
    pass(&mut engine, p0);
    drain(&mut engine);
    let status = pass_and_drain(&mut engine, p1);

    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0), "got {status:?}");
    assert!(
        engine.state().object(bear).is_none() || engine.state().object(bear).unwrap().zone != ZoneKind::Battlefield,
        "the script-declared ability's own real target was destroyed"
    );
}

fn pass_and_drain(engine: &mut Engine, player: PlayerId) -> StepStatus {
    pass(engine, player);
    drain(engine)
}
