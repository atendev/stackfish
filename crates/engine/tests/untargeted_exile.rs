//! CR 406.2, 601.2c: `Game.Ops.ExileSelf` moves the ability's own source, and `ExileAll(filter, zone)` moves every matching object on the battlefield or in graveyards, both through the `Exile` zone move.

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{
    card_object, drain, minimal_card_definition, two_player_state as base_two_player_state, zones,
};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const SPELL: ObjectId = ObjectId(100);

const SELF_EXILE_FROM_GRAVEYARD: &str = r#"
local s = {}
function s.register_abilities(c)
    local a = Game.CreateActivatedAbility(c)
    a:SetFunctionsFrom("Graveyard")
    a:SetOperation(Game.Ops.ExileSelf)
    a:Register()
end
return s
"#;

const SELF_EXILE_FROM_BATTLEFIELD: &str = r#"
local s = {}
function s.register_abilities(c)
    local a = Game.CreateActivatedAbility(c)
    a:SetOperation(Game.Ops.ExileSelf)
    a:Register()
end
return s
"#;

const SACRIFICE_THEN_EXILE_SELF: &str = r#"
local s = {}
function s.register_abilities(c)
    local a = Game.CreateActivatedAbility(c)
    a:AddCost(Game.Costs.SacrificeSelf)
    a:SetOperation(Game.Ops.ExileSelf)
    a:Register()
end
return s
"#;

const EXILE_ALL_CREATURES: &str = r#"
local s = {}
function s.operation()
    return Game.Ops.ExileAll({ card_types = { "Creature" } })
end
return s
"#;

const BAD_ZONE_HAND: &str = r#"
local s = {}
function s.operation()
    return Game.Ops.ExileAll({}, "Hand")
end
return s
"#;

const BAD_ZONE_EXILE: &str = r#"
local s = {}
function s.operation()
    return Game.Ops.ExileAll({}, "Exile")
end
return s
"#;

const LEAVES_TRIGGER: &str = r#"
local s = {}
function s.register_abilities(c)
    local t = Game.CreateTriggeredAbility(c)
    t:SetCondition(Game.Conditions.LeavesBattlefield)
    t:SetOperation(Game.Ops.GainLife(1))
    t:Register()
end
return s
"#;

fn card(id: u64, name: &str, types: CardTypeSet, script: Option<&str>) -> CardDefinition {
    CardDefinition {
        script_ref: script
            .map(|s| ScriptReference::Inline(s.to_string()))
            .unwrap_or(ScriptReference::ByKey(name.to_string())),
        power: types
            .contains(CardTypeSet::CREATURE)
            .then_some(PTValue::Fixed(2)),
        toughness: types
            .contains(CardTypeSet::CREATURE)
            .then_some(PTValue::Fixed(2)),
        ..minimal_card_definition(
            id,
            name,
            "unused",
            ManaCost {
                symbols: Vec::new(),
            },
            types,
        )
    }
}

fn spell(effect: AbilityEffect) -> CardDefinition {
    CardDefinition {
        effect: Some(effect),
        ..minimal_card_definition(
            99,
            "Test Spell",
            "unused",
            ManaCost {
                symbols: Vec::new(),
            },
            CardTypeSet::INSTANT,
        )
    }
}

struct Placed {
    id: u64,
    name: &'static str,
    types: CardTypeSet,
    script: Option<&'static str>,
    owner: PlayerId,
    controller: PlayerId,
    zone: ZoneKind,
    token: bool,
}

fn placed(
    id: u64,
    name: &'static str,
    types: CardTypeSet,
    owner: PlayerId,
    zone: ZoneKind,
) -> Placed {
    Placed {
        id,
        name,
        types,
        script: None,
        owner,
        controller: owner,
        zone,
        token: false,
    }
}

fn engine_with(cards: Vec<Placed>, effect: Option<AbilityEffect>) -> Engine {
    let mut state = GameState {
        step: Step::PreCombatMain,
        ..base_two_player_state(zones(
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
    let mut runtime = ScriptRuntime::new();
    for c in cards {
        let mut object = card_object(
            ObjectId(c.id),
            card(c.id, c.name, c.types, c.script),
            c.owner,
            c.zone,
        );
        object.controller = c.controller;
        object.is_token = c.token;
        object.tapped = c.zone == ZoneKind::Battlefield;
        state.objects.insert(ObjectId(c.id), object);
        let owner = if matches!(c.zone, ZoneKind::Battlefield | ZoneKind::Exile) {
            None
        } else {
            Some(c.owner)
        };
        state
            .zone_mut(c.zone, owner)
            .unwrap()
            .push_bottom(ObjectId(c.id));
        // Fixture battlefield cards never "entered", so register their script by hand.
        if c.zone == ZoneKind::Battlefield && c.script.is_some() {
            let definition = state.objects[&ObjectId(c.id)].definition.clone();
            runtime
                .run_card_script(&mut state, &definition, ObjectId(c.id))
                .expect("script registers");
        }
    }
    if let Some(effect) = effect {
        state
            .objects
            .insert(SPELL, card_object(SPELL, spell(effect), P0, ZoneKind::Hand));
        state
            .zone_mut(ZoneKind::Hand, Some(P0))
            .unwrap()
            .push_bottom(SPELL);
    }
    Engine::new(state)
}

fn all_filter(controller: Option<FilterController>) -> EffectFilter {
    EffectFilter {
        controller,
        card_types: Some(CardTypeSet::CREATURE),
        subtypes: None,
        excluded_subtypes: None,
        supertypes: None,
        excluded_supertypes: None,
        self_only: false,
    }
}

fn cast_and_resolve(engine: &mut Engine) {
    engine
        .submit_action(PlayerAction {
            player: P0,
            kind: PlayerActionKind::CastSpell {
                card: SPELL,
                modes: Vec::new(),
                targets: Vec::new(),
                x_value: None,
                alternative_cost: None,
                pay_buyback: false,
            },
        })
        .expect("p0 holds priority");
    let status = drain(engine);
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
    pass_both(engine);
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

fn activate_and_resolve(engine: &mut Engine, source: u64, payments: Vec<CostPayment>) {
    engine
        .submit_action(PlayerAction {
            player: P0,
            kind: PlayerActionKind::ActivateAbility {
                source: ObjectId(source),
                ability_index: 0,
                targets: Vec::new(),
                chosen_colors: Vec::new(),
                x_value: None,
            },
        })
        .expect("activation is legal");
    let status = drain(engine);
    assert!(
        matches!(status, StepStatus::AwaitingPayment { .. }),
        "{status:?}"
    );
    engine.submit_payment(PaymentPlan { payments }).unwrap();
    drain(engine);
    pass_both(engine);
}

fn names_in(engine: &Engine, zone: ZoneKind, owner: Option<PlayerId>) -> Vec<String> {
    let mut names: Vec<String> = engine
        .state()
        .zone(zone, owner)
        .unwrap()
        .contents
        .iter()
        .filter_map(|id| engine.state().object(*id))
        .map(|o| o.definition.name.clone())
        .collect();
    names.sort();
    names
}

fn exile_all(zone: ZoneKind, controller: Option<FilterController>) -> AbilityEffect {
    AbilityEffect::ExileAll {
        filter: all_filter(controller),
        zone,
    }
}

fn exile_to_graveyard_instead() -> ReplacementEffect {
    ReplacementEffect {
        source: ObjectId(900),
        spec: ReplacementSpec::Primitive {
            filter: EventFilter::WouldMoveZone {
                from: Some(ZoneKind::Battlefield),
                to: Some(ZoneKind::Exile),
                affected: all_filter(None),
            },
            payload: ReplacementPayload::ReplaceZoneDestination(ZoneKind::Graveyard),
        },
        consumption: ConsumptionPolicy::Repeatable,
    }
}

#[test]
fn a_graveyard_card_exiles_itself() {
    let mut dead = placed(
        1,
        "Gravecrawler",
        CardTypeSet::CREATURE,
        P0,
        ZoneKind::Graveyard,
    );
    dead.script = Some(SELF_EXILE_FROM_GRAVEYARD);
    let mut engine = engine_with(vec![dead], None);
    pass_both(&mut engine); // the sweep registers the graveyard ability
    activate_and_resolve(&mut engine, 1, Vec::new());

    assert!(names_in(&engine, ZoneKind::Graveyard, Some(P0)).is_empty());
    assert_eq!(
        names_in(&engine, ZoneKind::Exile, None),
        vec!["Gravecrawler"]
    );
    let exiled = engine
        .state()
        .objects
        .values()
        .find(|o| o.zone == ZoneKind::Exile)
        .unwrap();
    assert_ne!(exiled.object_id, ObjectId(1), "CR 400.7: new object");
}

#[test]
fn a_battlefield_permanent_exiles_itself_and_resets_its_state() {
    let mut bear = placed(1, "Bear", CardTypeSet::CREATURE, P0, ZoneKind::Battlefield);
    bear.script = Some(SELF_EXILE_FROM_BATTLEFIELD);
    let mut engine = engine_with(vec![bear], None);
    engine
        .state_mut()
        .objects
        .get_mut(&ObjectId(1))
        .unwrap()
        .damage_marked = 1;
    pass_both(&mut engine);
    activate_and_resolve(&mut engine, 1, Vec::new());

    assert!(names_in(&engine, ZoneKind::Battlefield, None).is_empty());
    let exiled = engine
        .state()
        .objects
        .values()
        .find(|o| o.zone == ZoneKind::Exile)
        .unwrap();
    assert_eq!(exiled.definition.name, "Bear");
    assert!(!exiled.tapped);
    assert_eq!(
        exiled.damage_marked, 0,
        "CR 400.7: the new object carries no damage"
    );
}

#[test]
fn a_source_that_already_left_does_nothing() {
    let mut bear = placed(1, "Bear", CardTypeSet::CREATURE, P0, ZoneKind::Battlefield);
    bear.script = Some(SACRIFICE_THEN_EXILE_SELF);
    let mut engine = engine_with(vec![bear], None);
    pass_both(&mut engine);
    activate_and_resolve(&mut engine, 1, vec![CostPayment::Sacrifice(ObjectId(1))]);

    assert_eq!(
        names_in(&engine, ZoneKind::Graveyard, Some(P0)),
        vec!["Bear"],
        "CR 400.7: the new graveyard object isn't the source"
    );
    assert!(names_in(&engine, ZoneKind::Exile, None).is_empty());
}

#[test]
fn exile_all_exiles_every_matching_permanent_and_leaves_the_rest() {
    let mut engine = engine_with(
        vec![
            placed(1, "Mine", CardTypeSet::CREATURE, P0, ZoneKind::Battlefield),
            placed(
                2,
                "Theirs",
                CardTypeSet::CREATURE,
                P1,
                ZoneKind::Battlefield,
            ),
            placed(3, "Forest", CardTypeSet::LAND, P0, ZoneKind::Battlefield),
        ],
        Some(exile_all(ZoneKind::Battlefield, None)),
    );
    cast_and_resolve(&mut engine);

    assert_eq!(
        names_in(&engine, ZoneKind::Battlefield, None),
        vec!["Forest"]
    );
    assert_eq!(
        names_in(&engine, ZoneKind::Exile, None),
        vec!["Mine", "Theirs"]
    );
}

#[test]
fn exile_all_respects_the_controller_filter() {
    let mut engine = engine_with(
        vec![
            placed(1, "Mine", CardTypeSet::CREATURE, P0, ZoneKind::Battlefield),
            placed(
                2,
                "Theirs",
                CardTypeSet::CREATURE,
                P1,
                ZoneKind::Battlefield,
            ),
        ],
        Some(exile_all(
            ZoneKind::Battlefield,
            Some(FilterController::Opponent),
        )),
    );
    cast_and_resolve(&mut engine);

    assert_eq!(names_in(&engine, ZoneKind::Battlefield, None), vec!["Mine"]);
    assert_eq!(names_in(&engine, ZoneKind::Exile, None), vec!["Theirs"]);
}

#[test]
fn exile_all_makes_tokens_cease_to_exist() {
    let mut token = placed(
        1,
        "Spirit",
        CardTypeSet::CREATURE,
        P0,
        ZoneKind::Battlefield,
    );
    token.token = true;
    let mut engine = engine_with(vec![token], Some(exile_all(ZoneKind::Battlefield, None)));
    cast_and_resolve(&mut engine);

    assert!(
        engine.state().objects.values().all(|o| !o.is_token),
        "CR 111.7"
    );
    assert!(names_in(&engine, ZoneKind::Exile, None).is_empty());
}

#[test]
fn exile_all_fires_a_leaves_the_battlefield_trigger_for_each_object() {
    let mut a = placed(1, "A", CardTypeSet::CREATURE, P0, ZoneKind::Battlefield);
    a.script = Some(LEAVES_TRIGGER);
    let mut b = placed(2, "B", CardTypeSet::CREATURE, P0, ZoneKind::Battlefield);
    b.script = Some(LEAVES_TRIGGER);
    let mut engine = engine_with(vec![a, b], Some(exile_all(ZoneKind::Battlefield, None)));
    let before = engine.state().player(P0).unwrap().life;
    cast_and_resolve(&mut engine);
    let StepStatus::AwaitingTriggerOrderChoice { group, .. } = drain(&mut engine) else {
        panic!("two simultaneous triggers ask for an order (CR 603.3b)")
    };
    engine.submit_trigger_order(group).unwrap();
    drain(&mut engine);
    for _ in 0..4 {
        if engine.state().stack.is_empty() {
            break;
        }
        pass_both(&mut engine);
    }

    assert_eq!(engine.state().player(P0).unwrap().life, before + 2);
}

#[test]
fn exile_all_from_graveyards_takes_only_cards_the_filter_matches() {
    let mut engine = engine_with(
        vec![
            placed(1, "Mine", CardTypeSet::CREATURE, P0, ZoneKind::Graveyard),
            placed(2, "Theirs", CardTypeSet::CREATURE, P1, ZoneKind::Graveyard),
            placed(3, "Bolt", CardTypeSet::INSTANT, P0, ZoneKind::Graveyard),
            placed(
                4,
                "OnBoard",
                CardTypeSet::CREATURE,
                P0,
                ZoneKind::Battlefield,
            ),
        ],
        Some(exile_all(ZoneKind::Graveyard, None)),
    );
    cast_and_resolve(&mut engine);

    assert_eq!(
        names_in(&engine, ZoneKind::Exile, None),
        vec!["Mine", "Theirs"]
    );
    assert_eq!(
        names_in(&engine, ZoneKind::Graveyard, Some(P0)),
        vec!["Bolt", "Test Spell"]
    );
    assert!(names_in(&engine, ZoneKind::Graveyard, Some(P1)).is_empty());
    assert_eq!(
        names_in(&engine, ZoneKind::Battlefield, None),
        vec!["OnBoard"]
    );
}

#[test]
fn exile_all_can_be_redirected_by_a_replacement_effect() {
    let mut engine = engine_with(
        vec![
            placed(1, "Bear", CardTypeSet::CREATURE, P0, ZoneKind::Battlefield),
            placed(
                900,
                "Redirector",
                CardTypeSet::ENCHANTMENT,
                P0,
                ZoneKind::Battlefield,
            ),
        ],
        Some(exile_all(ZoneKind::Battlefield, None)),
    );
    engine
        .state_mut()
        .replacement_effects
        .push(exile_to_graveyard_instead());
    cast_and_resolve(&mut engine);

    assert!(
        names_in(&engine, ZoneKind::Exile, None).is_empty(),
        "CR 614: the exile was replaced"
    );
    assert_eq!(
        names_in(&engine, ZoneKind::Graveyard, Some(P0)),
        vec!["Bear", "Test Spell"]
    );
}

#[test]
fn exile_all_with_no_match_resolves_cleanly() {
    let mut engine = engine_with(
        vec![placed(
            1,
            "Forest",
            CardTypeSet::LAND,
            P0,
            ZoneKind::Battlefield,
        )],
        Some(exile_all(ZoneKind::Battlefield, None)),
    );
    cast_and_resolve(&mut engine);

    assert!(engine.state().stack.is_empty());
    assert_eq!(
        names_in(&engine, ZoneKind::Battlefield, None),
        vec!["Forest"]
    );
}

#[test]
fn lua_exile_all_defaults_to_the_battlefield() {
    let runtime = stackfish_engine::script::ScriptRuntime::new();
    let (effect, _) = runtime
        .build_operation(EXILE_ALL_CREATURES)
        .unwrap()
        .unwrap();
    match effect {
        Some(AbilityEffect::ExileAll { filter, zone }) => {
            assert_eq!(zone, ZoneKind::Battlefield);
            assert_eq!(filter.card_types, Some(CardTypeSet::CREATURE));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn lua_exile_all_rejects_a_zone_it_cannot_source_from() {
    let runtime = stackfish_engine::script::ScriptRuntime::new();
    assert!(runtime.build_operation(BAD_ZONE_HAND).is_err());
    assert!(runtime.build_operation(BAD_ZONE_EXILE).is_err());
}
