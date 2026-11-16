//! CR 400.7j: `Game.Ops.ExileTopLinked(n)` exiles the top of a library face up and a `:SetExiledByThisEffect()` grant later in the same chain covers exactly those cards.
//! Impulse draw is the model ("exile the top three cards, you may play them this turn").

use std::collections::{HashMap, VecDeque};

use stackfish_engine::decision::Entry;
use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{
    card_object, cast_spell, drain, minimal_card_definition, pass_both_and_drain,
    pay_free_and_drain, try_cast_spell, two_player_state, zones,
};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const IMPULSE: ObjectId = ObjectId(20);
const KEEPER: ObjectId = ObjectId(1);

fn definition(id: u64, name: &str, types: CardTypeSet) -> CardDefinition {
    CardDefinition {
        effect: types
            .contains(CardTypeSet::SORCERY)
            .then_some(AbilityEffect::Shuffle),
        script_ref: ScriptReference::Inline("local s = {}\nreturn s".to_string()),
        ..minimal_card_definition(
            id,
            name,
            name,
            ManaCost {
                symbols: Vec::new(),
            },
            types,
        )
    }
}

fn grant(kind: ZonePermissionKind) -> AbilityEffect {
    AbilityEffect::GrantZonePermission(Box::new(ZonePermission {
        source: ObjectId(0),
        controller: P0,
        affected: FilterController::You,
        zone: ZoneKind::Exile,
        owners: FilterController::You,
        kind,
        cards: FilterSpec::ANY,
        library_top: false,
        linked: ExileLink::Cards(Vec::new()),
        reveals_top: false,
        duration: EffectDuration::UntilEndOfTurn,
        rider: None,
    }))
}

// "Exile the top `n` cards of a library. Until end of turn, you may play them."
fn impulse(n: u32, target: Option<TargetSpec>) -> CardDefinition {
    CardDefinition {
        target_requirement: target,
        effect: Some(AbilityEffect::Chain {
            entries: vec![
                AbilityEffect::ExileTopLinked(n),
                grant(ZonePermissionKind::PlayLand),
                grant(ZonePermissionKind::Cast),
            ],
            words: None,
        }),
        ..definition(10, "Test Impulse", CardTypeSet::SORCERY)
    }
}

fn player_target() -> TargetSpec {
    TargetSpec::Primitive(TargetRequirement {
        filter: TargetFilter::Player { controller: None },
        count: 1,
        up_to: false,
        at_least: false,
        exclude_source: false,
    })
}

fn put(state: &mut GameState, id: u64, card: CardDefinition, owner: PlayerId, zone: ZoneKind) {
    let id = ObjectId(id);
    let shared = matches!(zone, ZoneKind::Battlefield | ZoneKind::Exile);
    state
        .zone_mut(zone, (!shared).then_some(owner))
        .unwrap()
        .push_bottom(id);
    state.objects.insert(id, card_object(id, card, owner, zone));
}

// P0's library, top first: a land, a sorcery, a filler sorcery, then one more land.
// P1's library: a land, a sorcery.
fn state_with(spell: CardDefinition) -> GameState {
    let mut state = GameState {
        step: Step::PreCombatMain,
        last_active_turn_number: HashMap::from([(P0, 1), (P1, 1)]),
        ..two_player_state(zones(
            P0,
            P1,
            &[
                ZoneKind::Hand,
                ZoneKind::Library,
                ZoneKind::Battlefield,
                ZoneKind::Graveyard,
                ZoneKind::Exile,
            ],
        ))
    };
    put(&mut state, IMPULSE.0, spell, P0, ZoneKind::Hand);
    for (id, name, types) in [
        (40, "Top Land", CardTypeSet::LAND),
        (41, "Top Sorcery", CardTypeSet::SORCERY),
        (42, "Third Sorcery", CardTypeSet::SORCERY),
        (43, "Fourth Land", CardTypeSet::LAND),
    ] {
        put(
            &mut state,
            id,
            definition(id, name, types),
            P0,
            ZoneKind::Library,
        );
    }
    for (id, name, types) in [
        (50, "Their Land", CardTypeSet::LAND),
        (51, "Their Sorcery", CardTypeSet::SORCERY),
    ] {
        put(
            &mut state,
            id,
            definition(id, name, types),
            P1,
            ZoneKind::Library,
        );
    }
    state
}

fn resolve(state: GameState, targets: Vec<Target>) -> Engine {
    let mut engine = Engine::new(state);
    cast_spell(&mut engine, P0, IMPULSE, targets);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, P0, P1);
    engine
}

fn in_exile(engine: &Engine, name: &str) -> Option<ObjectId> {
    engine
        .state()
        .objects
        .values()
        .find(|o| o.definition.name == name && o.zone == ZoneKind::Exile)
        .map(|o| o.object_id)
}

fn exile_names(engine: &Engine) -> Vec<String> {
    let mut names: Vec<String> = engine
        .state()
        .objects
        .values()
        .filter(|o| o.zone == ZoneKind::Exile)
        .map(|o| o.definition.name.clone())
        .collect();
    names.sort();
    names
}

fn can_cast(engine: &Engine, card: ObjectId) -> bool {
    engine
        .legal_actions()
        .iter()
        .any(|entry| matches!(entry, Entry::Cast(cast) if cast.card == card))
}

fn can_play_land(engine: &Engine, card: ObjectId) -> bool {
    engine
        .legal_actions()
        .iter()
        .any(|entry| matches!(entry, Entry::PlayLand { card: c } if *c == card))
}

fn cards_of(engine: &Engine, index: usize) -> Vec<ObjectId> {
    match &engine.state().zone_permissions[index].linked {
        ExileLink::Cards(ids) => ids.clone(),
        other => panic!("expected a Cards row, got {other:?}"),
    }
}

fn pass_until_turn_of(engine: &mut Engine, player: PlayerId) {
    loop {
        match drain(engine) {
            StepStatus::AwaitingAction { player: acting } => {
                if engine.state().active_player == player && engine.state().step == Step::Upkeep {
                    return;
                }
                engine
                    .submit_action(PlayerAction {
                        player: acting,
                        kind: PlayerActionKind::PassPriority,
                    })
                    .unwrap();
            }
            other => panic!("unexpected pause while passing: {other:?}"),
        }
    }
}

// CR 400.7j
#[test]
fn act_on_impulse_covers_exactly_the_three_exiled_cards() {
    let mut engine = resolve(state_with(impulse(3, None)), Vec::new());
    assert_eq!(
        exile_names(&engine),
        ["Third Sorcery", "Top Land", "Top Sorcery"]
    );
    let land = in_exile(&engine, "Top Land").unwrap();
    let sorcery = in_exile(&engine, "Top Sorcery").unwrap();
    assert!(can_play_land(&engine, land));
    assert!(can_cast(&engine, sorcery));
    assert!(
        engine
            .state()
            .objects
            .values()
            .any(|o| o.definition.name == "Fourth Land" && o.zone == ZoneKind::Library),
        "the fourth card stays in the library"
    );
    assert!(engine.state().exiled_this_resolution.is_empty());

    engine
        .submit_action(PlayerAction {
            player: P0,
            kind: PlayerActionKind::PlayLand { card: land },
        })
        .expect("a land from among the exiled cards");
    drain(&mut engine);
    try_cast_spell(&mut engine, P0, sorcery, Vec::new()).expect("a spell from among them");
    pay_free_and_drain(&mut engine);
    assert!(!can_cast(&engine, sorcery));
    assert!(
        cards_of(&engine, 0).len() == 1 && cards_of(&engine, 1).len() == 1,
        "a played or cast card leaves each list"
    );
}

// CR 400.7j, 514.2
#[test]
fn the_permission_ends_at_cleanup_and_the_cards_stay_exiled() {
    let mut engine = resolve(state_with(impulse(3, None)), Vec::new());
    assert_eq!(engine.state().zone_permissions.len(), 2);
    pass_until_turn_of(&mut engine, P1);
    assert!(engine.state().zone_permissions.is_empty());
    assert_eq!(exile_names(&engine).len(), 3);
}

// CR 400.7j
#[test]
fn the_scratch_list_is_empty_whenever_a_player_has_priority() {
    let mut engine = Engine::new(state_with(impulse(3, None)));
    cast_spell(&mut engine, P0, IMPULSE, Vec::new());
    pay_free_and_drain(&mut engine);
    assert!(engine.state().exiled_this_resolution.is_empty());
    pass_both_and_drain(&mut engine, P0, P1);
    assert!(engine.state().exiled_this_resolution.is_empty());
}

// CR 400.7j, 614
#[test]
fn a_card_a_replacement_sends_elsewhere_is_not_covered() {
    let mut state = state_with(impulse(3, None));
    let redirect = definition(99, "Test Redirect", CardTypeSet::ENCHANTMENT);
    put(&mut state, 99, redirect, P0, ZoneKind::Battlefield);
    state.replacement_effects.push(ReplacementEffect {
        source: ObjectId(99),
        spec: ReplacementSpec::Primitive {
            filter: EventFilter::WouldMoveZone {
                from: Some(ZoneKind::Library),
                to: Some(ZoneKind::Exile),
                affected: EffectFilter {
                    controller: None,
                    card_types: Some(CardTypeSet::LAND),
                    subtypes: None,
                    excluded_subtypes: None,
                    supertypes: None,
                    excluded_supertypes: None,
                    self_only: false,
                },
            },
            payload: ReplacementPayload::ReplaceZoneDestination(ZoneKind::Graveyard),
        },
        consumption: ConsumptionPolicy::Repeatable,
    });
    let engine = resolve(state, Vec::new());
    assert_eq!(exile_names(&engine), ["Third Sorcery", "Top Sorcery"]);
    assert_eq!(cards_of(&engine, 0).len(), 2);
}

// CR 400.7j
#[test]
fn a_player_target_exiles_their_library_and_the_controller_plays_the_cards() {
    let engine = resolve(
        state_with(impulse(2, Some(player_target()))),
        vec![Target::Player(P1)],
    );
    assert_eq!(exile_names(&engine), ["Their Land", "Their Sorcery"]);
    let land = in_exile(&engine, "Their Land").unwrap();
    let sorcery = in_exile(&engine, "Their Sorcery").unwrap();
    assert!(can_play_land(&engine, land));
    assert!(can_cast(&engine, sorcery));
}

// CR 400.7
#[test]
fn a_card_that_leaves_exile_and_returns_is_not_covered() {
    let mut engine = resolve(state_with(impulse(3, None)), Vec::new());
    let sorcery = in_exile(&engine, "Top Sorcery").unwrap();
    assert!(cards_of(&engine, 1).contains(&sorcery));
    try_cast_spell(&mut engine, P0, sorcery, Vec::new()).unwrap();
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, P0, P1);
    assert!(!cards_of(&engine, 1).contains(&sorcery));
    let mut state = engine.state().clone();
    let returned = ObjectId(300);
    state.objects.insert(
        returned,
        card_object(
            returned,
            definition(41, "Top Sorcery", CardTypeSet::SORCERY),
            P0,
            ZoneKind::Exile,
        ),
    );
    state
        .zone_mut(ZoneKind::Exile, None)
        .unwrap()
        .contents
        .push_back(returned);
    let engine = Engine::new(state);
    assert!(!can_cast(&engine, returned));
    assert!(can_cast(
        &engine,
        in_exile(&engine, "Third Sorcery").unwrap()
    ));
}

// CR 400.7j
#[test]
fn a_short_library_exiles_what_it_has_and_an_empty_one_nothing() {
    let mut state = state_with(impulse(3, Some(player_target())));
    let drained: VecDeque<ObjectId> = std::mem::take(
        &mut state
            .zone_mut(ZoneKind::Library, Some(P1))
            .unwrap()
            .contents,
    );
    let first = *drained.front().unwrap();
    state
        .zone_mut(ZoneKind::Library, Some(P1))
        .unwrap()
        .contents
        .push_back(first);
    let engine = resolve(state.clone(), vec![Target::Player(P1)]);
    assert_eq!(exile_names(&engine), ["Their Land"]);

    state
        .zone_mut(ZoneKind::Library, Some(P1))
        .unwrap()
        .contents
        .clear();
    let mut engine = resolve(state, vec![Target::Player(P1)]);
    assert!(exile_names(&engine).is_empty());
    assert!(cards_of(&engine, 0).is_empty());
    assert!(
        !matches!(drain(&mut engine), StepStatus::GameOver { .. }),
        "exiling is not drawing"
    );
}

const KEEPER_SCRIPT: &str = r#"
local s = {}
function s.register_abilities(c)
    local a = Game.CreateActivatedAbility(c)
    a:SetOperation(Game.Ops.ExileTopLinked(1))
    a:Register()
end
return s
"#;

// CR 607.2a
#[test]
fn a_battlefield_source_also_records_the_card_as_exiled_with_it() {
    let mut state = state_with(definition(10, "Test Blank", CardTypeSet::SORCERY));
    let keeper = CardDefinition {
        script_ref: ScriptReference::Inline(KEEPER_SCRIPT.to_string()),
        ..definition(1, "Test Keeper", CardTypeSet::ARTIFACT)
    };
    put(
        &mut state,
        KEEPER.0,
        keeper.clone(),
        P0,
        ZoneKind::Battlefield,
    );
    state
        .objects
        .get_mut(&KEEPER)
        .unwrap()
        .controlled_since_turn = 0;
    ScriptRuntime::new()
        .run_card_script(&mut state, &keeper, KEEPER)
        .expect("script registers");
    let mut engine = Engine::new(state);
    engine
        .submit_action(PlayerAction {
            player: P0,
            kind: PlayerActionKind::ActivateAbility {
                source: KEEPER,
                ability_index: 0,
                modes: Vec::new(),
                targets: Vec::new(),
                chosen_colors: Vec::new(),
                division: Vec::new(),
                x_value: None,
            },
        })
        .expect("activation is legal");
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, P0, P1);
    let exiled = in_exile(&engine, "Top Land").expect("the top card is exiled");
    assert_eq!(
        engine.state().exiled_with,
        vec![LinkedExile {
            source: KEEPER,
            exiled
        }]
    );
    assert!(engine.state().exiled_this_resolution.is_empty());
}

fn load(source: &str) -> Result<AbilityEffect, String> {
    ScriptRuntime::new()
        .load_operation(source)
        .map(|(effect, _)| effect.expect("a Game.Ops value"))
        .map_err(|e| format!("{e:?}"))
}

const BUILDER: &str = "local p = Game.CreateZonePermission(7)\np:SetAffected(Game.Affected.You)\n\
    p:SetKind(\"Cast\")\np:SetExiledByThisEffect()\n";

// CR 400.7j
#[test]
fn the_lua_builder_makes_a_grant_with_an_empty_cards_row() {
    let effect = load(&format!(
        "{BUILDER}p:SetZone(\"Exile\")\nreturn Game.Ops.GrantZonePermission(p, Game.Durations.UntilEndOfTurn)"
    ))
    .unwrap();
    let AbilityEffect::GrantZonePermission(permission) = effect else {
        panic!("expected a grant");
    };
    assert_eq!(permission.linked, ExileLink::Cards(Vec::new()));
}

// CR 400.7j
#[test]
fn the_lua_builder_refuses_the_setter_where_no_resolution_exists() {
    let mut registered = ScriptRuntime::new();
    let mut state = state_with(definition(10, "Test Blank", CardTypeSet::SORCERY));
    let card = CardDefinition {
        script_ref: ScriptReference::Inline(format!(
            "local s = {{}}\nfunction s.register_abilities(c)\n{BUILDER}p:SetZone(\"Exile\")\np:Register()\nend\nreturn s"
        )),
        ..definition(1, "Test Static", CardTypeSet::ARTIFACT)
    };
    put(&mut state, 5, card.clone(), P0, ZoneKind::Battlefield);
    let error = registered
        .run_card_script(&mut state, &card, ObjectId(5))
        .expect_err("a static has no resolution");
    assert!(
        format!("{error:?}").contains("SetExiledByThisEffect"),
        "{error:?}"
    );

    let linked = load(&format!(
        "{BUILDER}p:SetZone(\"Exile\")\np:SetLinked()\nreturn Game.Ops.GrantZonePermission(p, Game.Durations.UntilEndOfTurn)"
    ))
    .unwrap_err();
    assert!(linked.contains("SetLinked"), "{linked}");
    let zone = load(&format!(
        "{BUILDER}p:SetZone(\"Graveyard\")\nreturn Game.Ops.GrantZonePermission(p, Game.Durations.UntilEndOfTurn)"
    ))
    .unwrap_err();
    assert!(zone.contains("Exile"), "{zone}");
}

// CR 400.7j
#[test]
fn exile_top_linked_loads_as_an_operation_but_not_inside_for_each_player() {
    assert_eq!(
        load("return Game.Ops.ExileTopLinked(3)").unwrap(),
        AbilityEffect::ExileTopLinked(3)
    );
    let error = load("return Game.Ops.ForEachPlayer(Game.Ops.ExileTopLinked(1))")
        .expect_err("a per-player wrap is not built yet");
    assert!(error.contains("ForEachPlayer"), "{error}");
}
