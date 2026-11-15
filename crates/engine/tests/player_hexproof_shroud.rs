//! CR 702.11c, 702.18a: `PlayerKeywordModifier` gives a player hexproof or shroud, built by `Game.CreatePlayerKeywordModifier` and enforced in `commands::target_is_legal` for player candidates.
//! Hexproof stops only the player's opponents, shroud stops everyone, and an Aura attached to a player is not targeting, so it stays (CR 115.1b).

use std::collections::HashMap;

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::sba::{SbaOutcome, check_state_based_actions};
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{
    card_object, drain, minimal_card_definition, permanent, put_on_battlefield,
    two_player_state as base_two_player_state, zones,
};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const SPELL: ObjectId = ObjectId(100);
const SECOND_SPELL: ObjectId = ObjectId(101);

fn keyword_script(affected: &str, keyword: &str) -> String {
    format!(
        r#"
local s = {{}}
function s.register_abilities(c)
    local m = Game.CreatePlayerKeywordModifier(c)
    m:SetAffected(Game.Affected.{affected})
    m:SetKeyword("{keyword}")
    m:Register()
end
return s
"#
    )
}

fn no_cost() -> ManaCost {
    ManaCost {
        symbols: Vec::new(),
    }
}

fn enchantment(id: u64, script: String) -> CardDefinition {
    CardDefinition {
        script_ref: ScriptReference::Inline(script),
        ..minimal_card_definition(
            id,
            "Test Leyline",
            "unused",
            no_cost(),
            CardTypeSet::ENCHANTMENT,
        )
    }
}

fn requirement(filter: TargetFilter) -> TargetSpec {
    TargetSpec::Primitive(TargetRequirement {
        filter,
        count: 1,
        up_to: false,
        at_least: false,
        exclude_source: false,
    })
}

fn any_player() -> TargetFilter {
    TargetFilter::Player { controller: None }
}

fn damage_spell(filter: TargetFilter) -> CardDefinition {
    CardDefinition {
        effect: Some(AbilityEffect::Damage(2)),
        target_requirement: Some(requirement(filter)),
        ..minimal_card_definition(99, "Test Shock", "unused", no_cost(), CardTypeSet::INSTANT)
    }
}

fn destroy_enchantment_spell() -> CardDefinition {
    CardDefinition {
        effect: Some(AbilityEffect::Destroy),
        target_requirement: Some(requirement(TargetFilter::Object {
            mana_value: None,
            excluded_card_types: None,
            excluded_subtypes: None,
            card_types: Some(CardTypeSet::ENCHANTMENT),
            subtypes: None,
            supertypes: None,
            excluded_supertypes: None,
            controller: None,
            zone: None,
        })),
        ..minimal_card_definition(
            98,
            "Test Disenchant",
            "unused",
            no_cost(),
            CardTypeSet::INSTANT,
        )
    }
}

fn enchant_player_aura() -> CardDefinition {
    CardDefinition {
        subtypes: vec![Subtype("Aura".to_string())],
        target_requirement: Some(requirement(any_player())),
        ..minimal_card_definition(
            97,
            "Test Curse",
            "unused",
            no_cost(),
            CardTypeSet::ENCHANTMENT,
        )
    }
}

fn main_phase_state() -> GameState {
    GameState {
        step: Step::PreCombatMain,
        last_active_turn_number: HashMap::from([(P0, 1), (P1, 1)]),
        ..base_two_player_state(zones(
            P0,
            P1,
            &[
                ZoneKind::Hand,
                ZoneKind::Library,
                ZoneKind::Battlefield,
                ZoneKind::Graveyard,
            ],
        ))
    }
}

// Each source is (id, controller, affected, keyword), registered through Lua.
fn state_with(
    sources: &[(u64, PlayerId, &str, &str)],
    hand: &[(ObjectId, CardDefinition)],
) -> GameState {
    let mut state = main_phase_state();
    let mut runtime = ScriptRuntime::new();
    for &(id, controller, affected, keyword) in sources {
        let definition = enchantment(id, keyword_script(affected, keyword));
        put_on_battlefield(
            &mut state,
            permanent(ObjectId(id), definition.clone(), controller),
        );
        runtime
            .run_card_script(&mut state, &definition, ObjectId(id))
            .expect("script registers");
    }
    for (id, definition) in hand {
        state.objects.insert(
            *id,
            card_object(*id, definition.clone(), P0, ZoneKind::Hand),
        );
        state
            .zone_mut(ZoneKind::Hand, Some(P0))
            .unwrap()
            .push_bottom(*id);
    }
    state
}

fn engine_with(sources: &[(u64, PlayerId, &str, &str)], spell: CardDefinition) -> Engine {
    Engine::new(state_with(sources, &[(SPELL, spell)]))
}

fn pass_both(engine: &mut Engine) {
    for player in [P0, P1] {
        engine
            .submit_action(PlayerAction {
                player,
                kind: PlayerActionKind::PassPriority,
            })
            .unwrap();
        drain(engine);
    }
}

// P0 casts `card` and pays its free cost. `Err` means the targets were rejected.
fn cast(engine: &mut Engine, card: ObjectId, targets: Vec<Target>) -> Result<(), String> {
    engine
        .submit_action(PlayerAction {
            player: P0,
            kind: PlayerActionKind::CastSpell {
                card,
                modes: Vec::new(),
                targets,
                x_value: None,
                alternative_cost: None,
                pay_buyback: false,
                division: Vec::new(),
            },
        })
        .map_err(|e| format!("{e:?}"))?;
    let status = drain(engine);
    if let StepStatus::Error(e) = status {
        return Err(format!("{e:?}"));
    }
    assert!(
        matches!(status, StepStatus::AwaitingPayment { .. }),
        "{status:?}"
    );
    engine
        .submit_payment(PaymentPlan {
            payments: Vec::new(),
        })
        .unwrap();
    drain(engine);
    Ok(())
}

fn can_target(
    sources: &[(u64, PlayerId, &str, &str)],
    filter: TargetFilter,
    player: PlayerId,
) -> bool {
    let mut engine = engine_with(sources, damage_spell(filter));
    cast(&mut engine, SPELL, vec![Target::Player(player)]).is_ok()
}

fn life(engine: &Engine, player: PlayerId) -> i32 {
    engine.state().player(player).unwrap().life
}

fn script_error(script: &str) -> String {
    let mut state = main_phase_state();
    let definition = enchantment(1, script.to_string());
    put_on_battlefield(&mut state, permanent(ObjectId(1), definition.clone(), P0));
    let err = ScriptRuntime::new()
        .run_card_script(&mut state, &definition, ObjectId(1))
        .expect_err("the script should fail to load");
    format!("{err:?}")
}

fn sba_events(state: &mut GameState) -> Vec<GameEvent> {
    match check_state_based_actions(state, &ScriptRuntime::new()) {
        SbaOutcome::Progressed(events, _) => events,
        _ => panic!("fixture didn't expect an SBA choice pause"),
    }
}

// CR 702.11c
#[test]
fn lua_registers_a_player_keyword_modifier_stamped_with_its_controller() {
    let state = state_with(
        &[
            (1, P1, "You", "Hexproof"),
            (2, P0, "EnchantedPlayer", "Shroud"),
        ],
        &[],
    );
    let rows = &state.player_keyword_modifiers;
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].source, ObjectId(1));
    assert_eq!(rows[0].controller, P1);
    assert_eq!(
        rows[0].affected,
        PlayerAffected::Filter(FilterController::You)
    );
    assert_eq!(rows[0].keyword, PlayerKeyword::Hexproof);
    assert_eq!(rows[1].controller, P0);
    assert_eq!(rows[1].affected, PlayerAffected::EnchantedPlayer);
    assert_eq!(rows[1].keyword, PlayerKeyword::Shroud);
    assert!(rows[1].timestamp.0 > rows[0].timestamp.0);
}

#[test]
fn lua_rejects_an_unknown_keyword_an_object_tag_and_a_missing_setter() {
    let err = script_error(&keyword_script("You", "Flying"));
    assert!(
        err.contains("Flying") && err.contains(":SetKeyword takes"),
        "{err}"
    );

    let err = script_error(&keyword_script("Self", "Hexproof"));
    assert!(err.contains("selects objects, not players"), "{err}");

    let err = script_error(
        r#"
local s = {}
function s.register_abilities(c)
    local m = Game.CreatePlayerKeywordModifier(c)
    m:SetAffected(Game.Affected.You)
    m:Register()
end
return s
"#,
    );
    assert!(err.contains("no SetKeyword()"), "{err}");

    let err = script_error(
        r#"
local s = {}
function s.register_abilities(c)
    local m = Game.CreatePlayerKeywordModifier(c)
    m:SetKeyword("Shroud")
    m:Register()
end
return s
"#,
    );
    assert!(err.contains("no SetAffected()"), "{err}");
}

// CR 702.11c
#[test]
fn you_have_hexproof_stops_an_opponents_spell_but_not_your_own() {
    assert!(!can_target(&[(1, P1, "You", "Hexproof")], any_player(), P1));

    let mut engine = engine_with(&[(1, P0, "You", "Hexproof")], damage_spell(any_player()));
    cast(&mut engine, SPELL, vec![Target::Player(P0)]).expect("hexproof allows your own spell");
    pass_both(&mut engine);
    assert_eq!(life(&engine, P0), 18);
}

// CR 702.18a
#[test]
fn you_have_shroud_stops_your_own_spell_and_an_opponents() {
    assert!(!can_target(&[(1, P0, "You", "Shroud")], any_player(), P0));
    assert!(!can_target(&[(1, P1, "You", "Shroud")], any_player(), P1));
    assert!(can_target(&[(1, P1, "You", "Shroud")], any_player(), P0));
}

// CR 702.11c, 702.18a
#[test]
fn opponent_and_any_reach_the_right_players() {
    // P1's "each opponent has shroud" covers P0 only.
    let opponent_shroud = [(1, P1, "Opponent", "Shroud")];
    assert!(!can_target(&opponent_shroud, any_player(), P0));
    assert!(can_target(&opponent_shroud, any_player(), P1));

    // Every player has hexproof, so P0 can still target only themselves.
    let any_hexproof = [(1, P1, "Any", "Hexproof")];
    assert!(can_target(&any_hexproof, any_player(), P0));
    assert!(!can_target(&any_hexproof, any_player(), P1));
}

// CR 303.4m
#[test]
fn enchanted_player_covers_only_the_player_the_aura_is_attached_to() {
    let mut state = state_with(
        &[(1, P0, "EnchantedPlayer", "Shroud")],
        &[(SPELL, damage_spell(any_player()))],
    );
    state
        .objects
        .get_mut(&ObjectId(1))
        .unwrap()
        .attached_to_player = Some(P1);
    let mut engine = Engine::new(state.clone());
    assert!(cast(&mut engine, SPELL, vec![Target::Player(P1)]).is_err());
    let mut engine = Engine::new(state);
    assert!(cast(&mut engine, SPELL, vec![Target::Player(P0)]).is_ok());

    // Not attached to anyone, so no one has shroud.
    assert!(can_target(
        &[(1, P0, "EnchantedPlayer", "Shroud")],
        any_player(),
        P1
    ));
}

// CR 115.4, 702.11c
#[test]
fn any_target_skips_a_hexproof_player() {
    let sources = [(1, P1, "You", "Hexproof")];
    assert!(!can_target(&sources, TargetFilter::AnyTarget, P1));
    assert!(can_target(&sources, TargetFilter::AnyTarget, P0));
}

// CR 115.1b, 702.11c
#[test]
fn an_aura_spell_cannot_target_a_hexproof_player() {
    let sources = [(1, P1, "You", "Hexproof")];
    let mut engine = engine_with(&sources, enchant_player_aura());
    assert!(cast(&mut engine, SPELL, vec![Target::Player(P1)]).is_err());

    let mut engine = engine_with(&sources, enchant_player_aura());
    cast(&mut engine, SPELL, vec![Target::Player(P0)]).expect("hexproof is P1's only");
    pass_both(&mut engine);
    assert_eq!(
        engine.state().object(SPELL).map(|o| o.attached_to_player),
        None,
        "the Aura spell became a new object on resolution"
    );
    assert!(
        engine
            .state()
            .objects
            .values()
            .any(|o| o.zone == ZoneKind::Battlefield && o.attached_to_player == Some(P0))
    );
}

// CR 608.2b, 608.3b
#[test]
fn an_aura_spell_does_not_resolve_when_its_player_gains_shroud_first() {
    let mut state = state_with(&[], &[(SPELL, enchant_player_aura())]);
    put_on_battlefield(
        &mut state,
        permanent(ObjectId(1), enchantment(1, String::new()), P1),
    );
    let mut engine = Engine::new(state);
    cast(&mut engine, SPELL, vec![Target::Player(P1)]).expect("no shroud yet");
    engine
        .state_mut()
        .player_keyword_modifiers
        .push(PlayerKeywordModifier {
            source: ObjectId(1),
            controller: P1,
            affected: PlayerAffected::Filter(FilterController::You),
            keyword: PlayerKeyword::Shroud,
            timestamp: SequenceNo(0),
        });
    pass_both(&mut engine);

    let state = engine.state();
    assert!(
        state
            .ledger
            .iter()
            .any(|e| matches!(e.kind, GameEventKind::Fizzled { .. }))
    );
    assert!(
        state
            .objects
            .values()
            .all(|o| o.attached_to_player.is_none())
    );
    assert!(
        state
            .objects
            .values()
            .any(|o| o.definition.name == "Test Curse" && o.zone == ZoneKind::Graveyard)
    );
}

// CR 704.5m, 115.1b
#[test]
fn an_attached_aura_stays_on_a_player_who_gains_hexproof_and_shroud() {
    let mut state = state_with(&[(1, P1, "You", "Hexproof"), (2, P1, "You", "Shroud")], &[]);
    let mut aura = permanent(ObjectId(3), enchant_player_aura(), P0);
    aura.attached_to_player = Some(P1);
    put_on_battlefield(&mut state, aura);

    let events = sba_events(&mut state);
    assert!(events.is_empty(), "{events:?}");
    assert_eq!(
        state.object(ObjectId(3)).unwrap().attached_to_player,
        Some(P1)
    );
}

// CR 611.3b
#[test]
fn the_modifier_ends_when_its_source_leaves_the_battlefield() {
    let state = state_with(
        &[(1, P1, "You", "Hexproof")],
        &[
            (SPELL, destroy_enchantment_spell()),
            (SECOND_SPELL, damage_spell(any_player())),
        ],
    );
    let mut engine = Engine::new(state);
    assert!(cast(&mut engine, SECOND_SPELL, vec![Target::Player(P1)]).is_err());

    cast(&mut engine, SPELL, vec![Target::Object(ObjectId(1))]).expect("the source is targetable");
    pass_both(&mut engine);
    assert!(engine.state().player_keyword_modifiers.is_empty());

    cast(&mut engine, SECOND_SPELL, vec![Target::Player(P1)]).expect("P1 lost hexproof");
    pass_both(&mut engine);
    assert_eq!(life(&engine, P1), 18);
}

// CR 109.5
#[test]
fn you_follows_the_sources_current_controller() {
    let mut state = state_with(
        &[(1, P1, "You", "Shroud")],
        &[(SPELL, damage_spell(any_player()))],
    );
    state.objects.get_mut(&ObjectId(1)).unwrap().controller = P0;
    assert_eq!(state.player_keyword_modifiers[0].controller, P1);

    let mut engine = Engine::new(state.clone());
    assert!(cast(&mut engine, SPELL, vec![Target::Player(P0)]).is_err());
    let mut engine = Engine::new(state);
    assert!(cast(&mut engine, SPELL, vec![Target::Player(P1)]).is_ok());
}

// CR 702.11h, 702.18b
#[test]
fn a_second_source_keeps_the_player_protected_when_one_is_removed() {
    let mut state = state_with(
        &[(1, P1, "You", "Hexproof"), (2, P1, "You", "Hexproof")],
        &[(SPELL, damage_spell(any_player()))],
    );
    state
        .player_keyword_modifiers
        .retain(|m| m.source != ObjectId(1));
    assert_eq!(state.player_keyword_modifiers.len(), 1);
    let mut engine = Engine::new(state);
    assert!(cast(&mut engine, SPELL, vec![Target::Player(P1)]).is_err());
}

const PLAYER_TARGET_ABILITY: &str = r#"
local s = {}
function s.register_abilities(c)
    local a = Game.CreateActivatedAbility(c)
    a:SetTarget({ type = "player" })
    a:SetOperation(Game.Ops.Damage(1))
    a:Register()
end
return s
"#;

const SCRIPTED_PLAYER_TARGET_ABILITY: &str = r#"
local s = {}
function s.register_abilities(c)
    local a = Game.CreateActivatedAbility(c)
    a:SetTarget({ type = "player", filter = function(player, source, controller, state)
        return true
    end })
    a:SetOperation(Game.Ops.Damage(1))
    a:Register()
end
return s
"#;

// The land is played through the engine so its ability registers with the engine's own
// runtime, which a `PlayerScript` handle needs.
fn activate_at(script: &str, sources: &[(u64, PlayerId, &str, &str)], target: PlayerId) -> bool {
    let land = ObjectId(50);
    let definition = CardDefinition {
        script_ref: ScriptReference::Inline(script.to_string()),
        ..minimal_card_definition(
            50,
            "Test Ability Land",
            "unused",
            no_cost(),
            CardTypeSet::LAND,
        )
    };
    let mut engine = Engine::new(state_with(sources, &[(land, definition)]));
    engine
        .submit_action(PlayerAction {
            player: P0,
            kind: PlayerActionKind::PlayLand { card: land },
        })
        .unwrap();
    drain(&mut engine);
    let ability = engine
        .state()
        .abilities
        .iter()
        .find(|a| a.kind == AbilityKind::Activated)
        .expect("the land's ability registered")
        .source;
    let result = engine.submit_action(PlayerAction {
        player: P0,
        kind: PlayerActionKind::ActivateAbility {
            source: ability,
            ability_index: 0,
            modes: Vec::new(),
            targets: vec![Target::Player(target)],
            chosen_colors: Vec::new(),
            division: Vec::new(),
            x_value: None,
        },
    });
    result.is_ok() && !matches!(drain(&mut engine), StepStatus::Error(_))
}

// CR 602.2b, 702.11c
#[test]
fn an_activated_ability_cannot_target_an_opponent_with_hexproof() {
    let sources = [(1, P1, "You", "Hexproof")];
    assert!(!activate_at(PLAYER_TARGET_ABILITY, &sources, P1));
    assert!(activate_at(PLAYER_TARGET_ABILITY, &sources, P0));
}

// CR 602.2b, 702.18a
#[test]
fn a_scripted_player_target_still_respects_shroud() {
    assert!(!activate_at(
        SCRIPTED_PLAYER_TARGET_ABILITY,
        &[(1, P0, "You", "Shroud")],
        P0
    ));
    assert!(activate_at(
        SCRIPTED_PLAYER_TARGET_ABILITY,
        &[(1, P0, "You", "Shroud")],
        P1
    ));
    assert!(!activate_at(
        SCRIPTED_PLAYER_TARGET_ABILITY,
        &[(1, P1, "You", "Hexproof")],
        P1
    ));
}

fn upkeep_damage_opponent_trigger(source: ObjectId) -> RegisteredAbility {
    RegisteredAbility {
        text: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
        id: AbilityId(0),
        source,
        kind: AbilityKind::Triggered,
        condition: Some(ConditionSpec::Primitive(
            ConditionPrimitive::BeginningOfYourUpkeep,
        )),
        cost: None,
        target: Some(requirement(TargetFilter::Player {
            controller: Some(FilterController::Opponent),
        })),
        operation: ScriptHandle(0),
        effect: Some(AbilityEffect::Damage(1)),
        intervening_if: None,
    }
}

// Starts at PreGame, so passing once each lands on P0's upkeep.
fn upkeep_trigger_status(sources: &[(u64, PlayerId, &str, &str)]) -> (Engine, StepStatus) {
    let mut state = GameState {
        phase: Phase::PreGame,
        ..state_with(sources, &[])
    };
    put_on_battlefield(
        &mut state,
        permanent(ObjectId(60), enchantment(60, String::new()), P0),
    );
    state
        .abilities
        .push(upkeep_damage_opponent_trigger(ObjectId(60)));
    let mut engine = Engine::new(state);
    engine
        .submit_action(PlayerAction {
            player: P0,
            kind: PlayerActionKind::PassPriority,
        })
        .unwrap();
    drain(&mut engine);
    engine
        .submit_action(PlayerAction {
            player: P1,
            kind: PlayerActionKind::PassPriority,
        })
        .unwrap();
    let status = drain(&mut engine);
    (engine, status)
}

// CR 603.3d, 702.11c
#[test]
fn a_trigger_with_no_legal_player_target_is_removed() {
    let (_, status) = upkeep_trigger_status(&[]);
    let StepStatus::AwaitingTriggerTargets { candidates, .. } = status else {
        panic!("control: expected a target-choice pause, got {status:?}")
    };
    assert_eq!(candidates, vec![vec![Target::Player(P1)]]);

    let (engine, status) = upkeep_trigger_status(&[(1, P1, "You", "Hexproof")]);
    assert!(
        matches!(status, StepStatus::AwaitingAction { player } if player == P0),
        "{status:?}"
    );
    assert!(engine.state().stack.is_empty());
}
