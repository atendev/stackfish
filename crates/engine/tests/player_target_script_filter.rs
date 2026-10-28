//! CR 601.2c, 602.2b: `TargetFilter::PlayerScript` is the player twin of `TargetFilter::Script`, for a legality plain `TargetFilter::Player` cannot express ("target player with 10 or less life").
//! `invoke_player_target_legal` has the same per-candidate predicate shape as `invoke_target_legal`, with no protection check, as for plain `Player`. The Lua entry is `:SetTarget({ type = "player", filter = function(player, source, controller, state) ... end })`.

use std::collections::HashMap;

use stackfish_engine::engine::ActionError;
use stackfish_engine::commands::Rejected;
use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, hand_battlefield_graveyard_zones, player_state, two_player_state as base_two_player_state};

const SCRIPTED_LOW_LIFE_TARGET: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateActivatedAbility(c)
    e:SetTarget({ type = "player", filter = function(player, source, controller, state)
        return state.life[player] ~= nil and state.life[player] <= 10
    end })
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
        oracle_text: "{T}: Gain 1 life (scripted player target).".to_string(),
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

fn permanent(id: ObjectId, definition: CardDefinition, owner: PlayerId) -> GameObject {
    card_object(id, definition, owner, ZoneKind::Battlefield)
}

fn put_in_hand(state: &mut GameState, mut obj: GameObject, owner: PlayerId) {
    obj.zone = ZoneKind::Hand;
    let id = obj.object_id;
    state.zone_mut(ZoneKind::Hand, Some(owner)).unwrap().push_bottom(id);
    state.objects.insert(id, obj);
}

fn two_player_state(p1_life: i32) -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        players: vec![player_state(p0), PlayerState { life: p1_life, ..player_state(p1) }],
        step: Step::PreCombatMain,
        last_active_turn_number: HashMap::from([(p0, 1), (p1, 1)]),
        ..base_two_player_state(hand_battlefield_graveyard_zones(p0, p1))
    }
}

#[test]
fn set_target_with_a_player_table_self_registers_a_player_scripted_target_filter() {
    let mut state = two_player_state(20);
    let land = ObjectId(0);
    let definition = land_definition(1, SCRIPTED_LOW_LIFE_TARGET);
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(land);
    state.objects.insert(land, permanent(land, definition.clone(), PlayerId(0)));

    let mut runtime = ScriptRuntime::new();
    runtime.run_card_script(&mut state, &definition, land).expect("script self-registers cleanly");

    assert_eq!(state.abilities.len(), 1);
    let ability = &state.abilities[0];
    assert!(matches!(
        &ability.target,
        Some(TargetSpec::Primitive(req))
            if matches!(req.filter, TargetFilter::PlayerScript(_)) && req.count == 1 && !req.up_to
    ));
}

fn try_play_land_and_activate(p1_life: i32, target: Target) -> (Engine, Result<(), ActionError>) {
    let p0 = PlayerId(0);
    let land = ObjectId(0);
    let mut state = two_player_state(p1_life);
    put_in_hand(&mut state, permanent(land, land_definition(1, SCRIPTED_LOW_LIFE_TARGET), p0), p0);

    let mut engine = Engine::new(state);
    engine
        .submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PlayLand { card: land } })
        .expect("p0 holds priority, land is in hand");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0), "expected AwaitingAction after PlayLand, got {status:?}");
    assert_eq!(engine.state().abilities.len(), 1, "the land's activated ability self-registered");

    let new_land_id = engine.state().abilities[0].source;
    let result = engine.submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::ActivateAbility {
                source: new_land_id,
                ability_index: 0,
                targets: vec![target],
                chosen_colors: Vec::new(),
                x_value: None,
            },
        });
    (engine, result)
}

fn play_land_and_activate(p1_life: i32, target: Target) -> Engine {
    let (engine, result) = try_play_land_and_activate(p1_life, target);
    result.expect("p0 controls the land and holds priority");
    engine
}

#[test]
fn a_player_at_or_below_the_life_threshold_satisfies_the_script_predicate() {
    let p1 = PlayerId(1);
    let mut engine = play_land_and_activate(10, Target::Player(p1));
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }), "10-life player should satisfy the script predicate, got {status:?}");
}

#[test]
fn a_player_above_the_life_threshold_fails_the_script_predicate() {
    let p1 = PlayerId(1);
    let (_engine, result) = try_play_land_and_activate(20, Target::Player(p1));
    let err = result.unwrap_err();
    assert!(
        matches!(err, ActionError::Rejected(Rejected::InvalidTarget)),
        "20-life player should fail the script predicate, got {err:?}"
    );
}

#[test]
fn an_object_target_is_rejected_outright_by_a_player_script_filter() {
    let big = ObjectId(1);
    let (_engine, result) = try_play_land_and_activate(10, Target::Object(big));
    let err = result.unwrap_err();
    assert!(
        matches!(err, ActionError::Rejected(Rejected::InvalidTarget)),
        "TargetFilter::PlayerScript is player-only by this slice's own narrowing, got {err:?}"
    );
}
