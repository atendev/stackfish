//! CR 601.2c, 602.2b: `TargetFilter::Script` is a custom Lua legality predicate for a target requirement, for shapes the flat `TargetFilter::Object` cannot express ("target creature with power 4 or greater"). It lives in `TargetRequirement.filter`, where `count` and `up_to` work.
//! `invoke_target_legal` is a static per-candidate predicate checked once at cast or activation from `target_is_legal`, and not a `Game.SelectTargets` suspension. CR 702.16e protection still gates a script-legal candidate.
// TODO: a player-target script predicate, since candidates are `Target::Object` only

use std::collections::HashMap;

use stackfish_engine::engine::ActionError;
use stackfish_engine::commands::Rejected;
use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, hand_battlefield_graveyard_zones, two_player_state as base_two_player_state};

const SCRIPTED_POWER_FOUR_TARGET: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateActivatedAbility(c)
    e:SetTarget(function(object, source, controller, state)
        for _, card in ipairs(state.battlefield) do
            if card.id == object then
                return card.power ~= nil and card.power >= 4
            end
        end
        return false
    end)
    e:SetOperation(Game.Ops.GainLife(1))
    e:Register()
end
return s
"#;

fn land_definition(id: u64, script: &str) -> CardDefinition {
    CardDefinition {
        definition_id: id,
        name: "Test Ability Land".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::LAND,
        subtypes: Vec::new(),
        oracle_text: "{T}: Gain 1 life (scripted target).".to_string(),
        power: None,
        toughness: None,
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

fn creature_definition(id: u64, name: &str, power: i32, toughness: i32) -> CardDefinition {
    CardDefinition {
        definition_id: id,
        name: name.to_string(),
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
        script_ref: ScriptReference::ByKey(name.to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn permanent(id: ObjectId, definition: CardDefinition, owner: PlayerId) -> GameObject {
    card_object(id, definition, owner, ZoneKind::Battlefield)
}

fn put_on_battlefield(state: &mut GameState, obj: GameObject) {
    let id = obj.object_id;
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(id);
    state.objects.insert(id, obj);
}

fn put_in_hand(state: &mut GameState, mut obj: GameObject, owner: PlayerId) {
    obj.zone = ZoneKind::Hand;
    let id = obj.object_id;
    state.zone_mut(ZoneKind::Hand, Some(owner)).unwrap().push_bottom(id);
    state.objects.insert(id, obj);
}

// CR 702.16e
// Protection from Land collides with target_has_protection_from, since the source in every test is a Land.
fn protection_from_land(source: ObjectId) -> ContinuousEffect {
    ContinuousEffect {
        source,
        layer: Layer::AbilityAdding,
        affected: AffectedFilter::Primitive(EffectFilter {  controller: None, card_types: None, self_only: true, subtypes: None, excluded_subtypes: None, supertypes: None, excluded_supertypes: None, }),
        effect: EffectPayload::Primitive(EffectPrimitive::AddProtectionCardType(CardTypeSet::LAND)),
        timestamp: SequenceNo(0),
        duration: EffectDuration::Permanent,
        condition: None,
    }
}

fn two_player_state() -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        step: Step::PreCombatMain,
        last_active_turn_number: HashMap::from([(p0, 1), (p1, 1)]),
        ..base_two_player_state(hand_battlefield_graveyard_zones(p0, p1))
    }
}

#[test]
fn set_target_with_a_function_self_registers_a_scripted_target_filter() {
    let mut state = two_player_state();
    let land = ObjectId(0);
    let definition = land_definition(1, SCRIPTED_POWER_FOUR_TARGET);
    put_on_battlefield(&mut state, permanent(land, definition.clone(), PlayerId(0)));

    let mut runtime = ScriptRuntime::new();
    runtime.run_card_script(&mut state, &definition, land).expect("script self-registers cleanly");

    assert_eq!(state.abilities.len(), 1);
    let ability = &state.abilities[0];
    assert!(matches!(ability.effect, Some(AbilityEffect::GainLife(1))));
    assert!(matches!(
        &ability.target,
        Some(TargetSpec::Primitive(req))
            if matches!(req.filter, TargetFilter::Script(_)) && req.count == 1 && !req.up_to
    ));
}

// Self-registration must run inside the same Engine's ScriptRuntime for the ScriptHandle to resolve, so end-to-end tests play the land through the real PlayLand pipeline.
fn try_play_land_and_activate(
    creatures: &[(ObjectId, &str, i32, i32)],
    target: ObjectId,
    protect_target_from_land: bool,
) -> (Engine, Result<(), ActionError>) {
    let p0 = PlayerId(0);
    let land = ObjectId(0);
    let mut state = two_player_state();
    put_in_hand(&mut state, permanent(land, land_definition(1, SCRIPTED_POWER_FOUR_TARGET), p0), p0);
    for (id, name, power, toughness) in creatures {
        put_on_battlefield(&mut state, permanent(*id, creature_definition(id.0, name, *power, *toughness), p0));
    }
    if protect_target_from_land {
        state.continuous_effects.push(protection_from_land(target));
    }

    let mut engine = Engine::new(state);
    engine
        .submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PlayLand { card: land } })
        .expect("p0 holds priority, land is in hand");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0), "expected AwaitingAction after PlayLand, got {status:?}");
    assert_eq!(engine.state().abilities.len(), 1, "the land's activated ability self-registered");

    // The land's new battlefield id is read off the self-registered ability's source, since creatures already on the battlefield make zone order unreliable.
    let new_land_id = engine.state().abilities[0].source;
    let result = engine.submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::ActivateAbility {
                source: new_land_id,
                ability_index: 0,
                targets: vec![Target::Object(target)],
                chosen_colors: Vec::new(),
                x_value: None,
            },
        });
    (engine, result)
}

fn play_land_and_activate(
    creatures: &[(ObjectId, &str, i32, i32)],
    target: ObjectId,
    protect_target_from_land: bool,
) -> Engine {
    let (engine, result) = try_play_land_and_activate(creatures, target, protect_target_from_land);
    result.expect("p0 controls the land and holds priority");
    engine
}

#[test]
fn a_target_that_satisfies_the_script_predicate_is_accepted() {
    let big = ObjectId(1);
    let small = ObjectId(2);
    let mut engine = play_land_and_activate(&[(big, "Big", 4, 4), (small, "Small", 2, 2)], big, false);
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }), "power-4 target should satisfy the script predicate, got {status:?}");
}

#[test]
fn a_target_that_fails_the_script_predicate_is_rejected() {
    let big = ObjectId(1);
    let small = ObjectId(2);
    let (_engine, result) = try_play_land_and_activate(&[(big, "Big", 4, 4), (small, "Small", 2, 2)], small, false);
    let err = result.unwrap_err();
    assert!(
        matches!(err, ActionError::Rejected(Rejected::InvalidTarget)),
        "power-2 target should fail the script predicate, got {err:?}"
    );
}

#[test]
fn protection_still_blocks_a_script_legal_target() {
    let big = ObjectId(1);
    let (_engine, result) = try_play_land_and_activate(&[(big, "Big", 4, 4)], big, true);
    let err = result.unwrap_err();
    assert!(
        matches!(err, ActionError::Rejected(Rejected::InvalidTarget)),
        "CR 702.16e protection from Land must still block a target the script itself would accept, got {err:?}"
    );
}

#[test]
fn a_player_target_is_rejected_outright_by_a_script_filter() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let land = ObjectId(0);
    let mut state = two_player_state();
    put_in_hand(&mut state, permanent(land, land_definition(1, SCRIPTED_POWER_FOUR_TARGET), p0), p0);

    let mut engine = Engine::new(state);
    engine
        .submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PlayLand { card: land } })
        .expect("p0 holds priority, land is in hand");
    drain(&mut engine);
    let new_land_id = *engine.state().zone(ZoneKind::Battlefield, None).unwrap().contents.front().unwrap();

    let err = engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::ActivateAbility {
                source: new_land_id,
                ability_index: 0,
                targets: vec![Target::Player(p1)],
                chosen_colors: Vec::new(),
                x_value: None,
            },
        })
        .unwrap_err();
    assert!(
        matches!(err, ActionError::Rejected(Rejected::InvalidTarget)),
        "TargetFilter::Script is object-only by this slice's own narrowing, got {err:?}"
    );
}

#[test]
fn a_nonexistent_object_target_is_rejected() {
    let ghost = ObjectId(9999);
    let (_engine, result) = try_play_land_and_activate(&[], ghost, false);
    let err = result.unwrap_err();
    assert!(
        matches!(err, ActionError::Rejected(Rejected::InvalidTarget)),
        "a target id with no matching object must fail before the script ever runs, got {err:?}"
    );
}
