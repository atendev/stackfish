//! CR 400.3, 601.2c: `Game.Ops.ReturnSelfToHand` moves the ability's own source, and `ReturnAllToHand(filter, zone)` moves every matching object in a battlefield, graveyard or exile, both through the `Bounce` zone move.

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, minimal_card_definition, two_player_state as base_two_player_state, zones};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const SPELL: ObjectId = ObjectId(100);

const SELF_RETURN_FROM_GRAVEYARD: &str = r#"
local s = {}
function s.register_abilities(c)
    local a = Game.CreateActivatedAbility(c)
    a:SetFunctionsFrom("Graveyard")
    a:SetOperation(Game.Ops.ReturnSelfToHand)
    a:Register()
end
return s
"#;

const SELF_RETURN_FROM_BATTLEFIELD: &str = r#"
local s = {}
function s.register_abilities(c)
    local a = Game.CreateActivatedAbility(c)
    a:SetOperation(Game.Ops.ReturnSelfToHand)
    a:Register()
end
return s
"#;

const SACRIFICE_THEN_RETURN_SELF: &str = r#"
local s = {}
function s.register_abilities(c)
    local a = Game.CreateActivatedAbility(c)
    a:AddCost(Game.Costs.SacrificeSelf)
    a:SetOperation(Game.Ops.ReturnSelfToHand)
    a:Register()
end
return s
"#;

const RETURN_ALL_CREATURES: &str = r#"
local s = {}
function s.operation()
    return Game.Ops.ReturnAllToHand({ card_types = { "Creature" } })
end
return s
"#;

const BAD_ZONE: &str = r#"
local s = {}
function s.operation()
    return Game.Ops.ReturnAllToHand({}, "Hand")
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
        script_ref: script.map(|s| ScriptReference::Inline(s.to_string())).unwrap_or(ScriptReference::ByKey(name.to_string())),
        power: types.contains(CardTypeSet::CREATURE).then_some(PTValue::Fixed(2)),
        toughness: types.contains(CardTypeSet::CREATURE).then_some(PTValue::Fixed(2)),
        ..minimal_card_definition(id, name, "unused", ManaCost { symbols: Vec::new() }, types)
    }
}

fn spell(effect: AbilityEffect) -> CardDefinition {
    CardDefinition { effect: Some(effect), ..minimal_card_definition(99, "Test Spell", "unused", ManaCost { symbols: Vec::new() }, CardTypeSet::INSTANT) }
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

fn placed(id: u64, name: &'static str, types: CardTypeSet, owner: PlayerId, zone: ZoneKind) -> Placed {
    Placed { id, name, types, script: None, owner, controller: owner, zone, token: false }
}

fn engine_with(cards: Vec<Placed>, effect: Option<AbilityEffect>) -> Engine {
    let mut state = GameState {
        step: Step::PreCombatMain,
        ..base_two_player_state(zones(P0, P1, &[ZoneKind::Hand, ZoneKind::Library, ZoneKind::Battlefield, ZoneKind::Graveyard, ZoneKind::Exile]))
    };
    let mut runtime = ScriptRuntime::new();
    for c in cards {
        let mut object = card_object(ObjectId(c.id), card(c.id, c.name, c.types, c.script), c.owner, c.zone);
        object.controller = c.controller;
        object.is_token = c.token;
        object.tapped = c.zone == ZoneKind::Battlefield;
        state.objects.insert(ObjectId(c.id), object);
        let owner = if matches!(c.zone, ZoneKind::Battlefield | ZoneKind::Exile) { None } else { Some(c.owner) };
        state.zone_mut(c.zone, owner).unwrap().push_bottom(ObjectId(c.id));
        // Fixture battlefield cards never "entered", so register their script by hand.
        if c.zone == ZoneKind::Battlefield && c.script.is_some() {
            let definition = state.objects[&ObjectId(c.id)].definition.clone();
            runtime.run_card_script(&mut state, &definition, ObjectId(c.id)).expect("script registers");
        }
    }
    if let Some(effect) = effect {
        state.objects.insert(SPELL, card_object(SPELL, spell(effect), P0, ZoneKind::Hand));
        state.zone_mut(ZoneKind::Hand, Some(P0)).unwrap().push_bottom(SPELL);
    }
    Engine::new(state)
}

fn all_filter(controller: Option<FilterController>) -> EffectFilter {
    EffectFilter { controller, card_types: Some(CardTypeSet::CREATURE), subtypes: None, excluded_subtypes: None, supertypes: None, excluded_supertypes: None, self_only: false }
}

fn cast_and_resolve(engine: &mut Engine) {
    engine
        .submit_action(PlayerAction {
            player: P0,
            kind: PlayerActionKind::CastSpell { card: SPELL, modes: Vec::new(), targets: Vec::new(), x_value: None, alternative_cost: None, pay_buyback: false },
        })
        .expect("p0 holds priority");
    let status = drain(engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }), "{status:?}");
    engine.submit_payment(PaymentPlan { payments: Vec::new() }).unwrap();
    drain(engine);
    pass_both(engine);
}

fn pass_both(engine: &mut Engine) {
    for player in [P0, P1] {
        engine.submit_action(PlayerAction { player, kind: PlayerActionKind::PassPriority }).unwrap();
        drain(engine);
    }
}

fn activate_and_resolve(engine: &mut Engine, source: u64, payments: Vec<CostPayment>) {
    engine
        .submit_action(PlayerAction {
            player: P0,
            kind: PlayerActionKind::ActivateAbility { source: ObjectId(source), ability_index: 0, targets: Vec::new(), chosen_colors: Vec::new(), x_value: None },
        })
        .expect("activation is legal");
    let status = drain(engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }), "{status:?}");
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

fn return_all(zone: ZoneKind, controller: Option<FilterController>) -> AbilityEffect {
    AbilityEffect::ReturnAllToHand { filter: all_filter(controller), zone }
}

#[test]
fn a_graveyard_card_returns_itself_to_its_owners_hand() {
    let mut dead = placed(1, "Gravecrawler", CardTypeSet::CREATURE, P0, ZoneKind::Graveyard);
    dead.script = Some(SELF_RETURN_FROM_GRAVEYARD);
    let mut engine = engine_with(vec![dead], None);
    pass_both(&mut engine); // the sweep registers the graveyard ability
    activate_and_resolve(&mut engine, 1, Vec::new());

    assert!(names_in(&engine, ZoneKind::Graveyard, Some(P0)).is_empty());
    assert_eq!(names_in(&engine, ZoneKind::Hand, Some(P0)), vec!["Gravecrawler"]);
    let returned = engine.state().objects.values().find(|o| o.zone == ZoneKind::Hand).unwrap();
    assert_ne!(returned.object_id, ObjectId(1), "CR 400.7: new object");
    assert!(!returned.tapped);
    assert_eq!(returned.controller, returned.owner);
}

#[test]
fn a_battlefield_permanent_returns_itself_to_its_owners_hand() {
    let mut stolen = placed(1, "Bear", CardTypeSet::CREATURE, P1, ZoneKind::Battlefield);
    stolen.controller = P0;
    stolen.script = Some(SELF_RETURN_FROM_BATTLEFIELD);
    let mut engine = engine_with(vec![stolen], None);
    pass_both(&mut engine);
    activate_and_resolve(&mut engine, 1, Vec::new());

    assert!(names_in(&engine, ZoneKind::Battlefield, None).is_empty());
    assert!(names_in(&engine, ZoneKind::Hand, Some(P0)).is_empty(), "CR 400.3: not the controller's hand");
    assert_eq!(names_in(&engine, ZoneKind::Hand, Some(P1)), vec!["Bear"]);
}

#[test]
fn a_source_that_already_left_does_nothing() {
    let mut bear = placed(1, "Bear", CardTypeSet::CREATURE, P0, ZoneKind::Battlefield);
    bear.script = Some(SACRIFICE_THEN_RETURN_SELF);
    let mut engine = engine_with(vec![bear], None);
    pass_both(&mut engine);
    activate_and_resolve(&mut engine, 1, vec![CostPayment::Sacrifice(ObjectId(1))]);

    assert_eq!(names_in(&engine, ZoneKind::Graveyard, Some(P0)), vec!["Bear"], "CR 400.7: the new graveyard object isn't the source");
    assert!(names_in(&engine, ZoneKind::Hand, Some(P0)).is_empty());
}

#[test]
fn return_all_bounces_every_matching_permanent_and_leaves_the_rest() {
    let mut engine = engine_with(
        vec![
            placed(1, "Mine", CardTypeSet::CREATURE, P0, ZoneKind::Battlefield),
            placed(2, "Theirs", CardTypeSet::CREATURE, P1, ZoneKind::Battlefield),
            placed(3, "Forest", CardTypeSet::LAND, P0, ZoneKind::Battlefield),
        ],
        Some(return_all(ZoneKind::Battlefield, None)),
    );
    cast_and_resolve(&mut engine);

    assert_eq!(names_in(&engine, ZoneKind::Battlefield, None), vec!["Forest"]);
    assert_eq!(names_in(&engine, ZoneKind::Hand, Some(P0)), vec!["Mine"]);
    assert_eq!(names_in(&engine, ZoneKind::Hand, Some(P1)), vec!["Theirs"]);
}

#[test]
fn return_all_goes_to_each_owners_hand_and_respects_the_controller_filter() {
    let mut stolen = placed(1, "Stolen", CardTypeSet::CREATURE, P1, ZoneKind::Battlefield);
    stolen.controller = P0;
    let mut engine = engine_with(
        vec![stolen, placed(2, "Theirs", CardTypeSet::CREATURE, P1, ZoneKind::Battlefield)],
        Some(return_all(ZoneKind::Battlefield, Some(FilterController::You))),
    );
    cast_and_resolve(&mut engine);

    assert_eq!(names_in(&engine, ZoneKind::Battlefield, None), vec!["Theirs"]);
    assert_eq!(names_in(&engine, ZoneKind::Hand, Some(P1)), vec!["Stolen"], "to the owner, not the controller");
}

#[test]
fn return_all_makes_tokens_cease_to_exist() {
    let mut token = placed(1, "Spirit", CardTypeSet::CREATURE, P0, ZoneKind::Battlefield);
    token.token = true;
    let mut engine = engine_with(vec![token], Some(return_all(ZoneKind::Battlefield, None)));
    cast_and_resolve(&mut engine);

    assert!(engine.state().objects.values().all(|o| !o.is_token), "CR 111.7");
    assert!(names_in(&engine, ZoneKind::Hand, Some(P0)).is_empty());
}

#[test]
fn return_all_fires_a_leaves_the_battlefield_trigger_for_each_object() {
    let mut a = placed(1, "A", CardTypeSet::CREATURE, P0, ZoneKind::Battlefield);
    a.script = Some(LEAVES_TRIGGER);
    let mut b = placed(2, "B", CardTypeSet::CREATURE, P0, ZoneKind::Battlefield);
    b.script = Some(LEAVES_TRIGGER);
    let mut engine = engine_with(vec![a, b], Some(return_all(ZoneKind::Battlefield, None)));
    let before = engine.state().player(P0).unwrap().life;
    cast_and_resolve(&mut engine);
    let StepStatus::AwaitingTriggerOrderChoice { group, .. } = drain(&mut engine) else { panic!("two simultaneous triggers ask for an order (CR 603.3b)") };
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
fn return_all_from_graveyards_takes_only_cards_the_filter_matches() {
    let mut engine = engine_with(
        vec![
            placed(1, "Mine", CardTypeSet::CREATURE, P0, ZoneKind::Graveyard),
            placed(2, "Theirs", CardTypeSet::CREATURE, P1, ZoneKind::Graveyard),
            placed(3, "Bolt", CardTypeSet::INSTANT, P0, ZoneKind::Graveyard),
            placed(4, "OnBoard", CardTypeSet::CREATURE, P0, ZoneKind::Battlefield),
        ],
        Some(return_all(ZoneKind::Graveyard, Some(FilterController::You))),
    );
    cast_and_resolve(&mut engine);

    assert_eq!(names_in(&engine, ZoneKind::Hand, Some(P0)), vec!["Mine"]);
    assert_eq!(names_in(&engine, ZoneKind::Graveyard, Some(P0)), vec!["Bolt", "Test Spell"]);
    assert_eq!(names_in(&engine, ZoneKind::Graveyard, Some(P1)), vec!["Theirs"], "an opponent's graveyard is untouched");
    assert_eq!(names_in(&engine, ZoneKind::Battlefield, None), vec!["OnBoard"]);
}

#[test]
fn return_all_from_exile_returns_exiled_cards() {
    let mut engine = engine_with(
        vec![placed(1, "Exiled", CardTypeSet::CREATURE, P1, ZoneKind::Exile), placed(2, "Dead", CardTypeSet::CREATURE, P1, ZoneKind::Graveyard)],
        Some(return_all(ZoneKind::Exile, None)),
    );
    cast_and_resolve(&mut engine);

    assert_eq!(names_in(&engine, ZoneKind::Hand, Some(P1)), vec!["Exiled"]);
    assert_eq!(names_in(&engine, ZoneKind::Graveyard, Some(P1)), vec!["Dead"]);
}

#[test]
fn return_all_with_no_match_resolves_cleanly() {
    let mut engine = engine_with(vec![placed(1, "Forest", CardTypeSet::LAND, P0, ZoneKind::Battlefield)], Some(return_all(ZoneKind::Battlefield, None)));
    cast_and_resolve(&mut engine);

    assert!(engine.state().stack.is_empty());
    assert_eq!(names_in(&engine, ZoneKind::Battlefield, None), vec!["Forest"]);
}

#[test]
fn lua_return_all_defaults_to_the_battlefield() {
    let runtime = stackfish_engine::script::ScriptRuntime::new();
    let (effect, _) = runtime.build_operation(RETURN_ALL_CREATURES).unwrap().unwrap();
    match effect {
        Some(AbilityEffect::ReturnAllToHand { filter, zone }) => {
            assert_eq!(zone, ZoneKind::Battlefield);
            assert_eq!(filter.card_types, Some(CardTypeSet::CREATURE));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn lua_return_all_rejects_a_zone_it_cannot_source_from() {
    let runtime = stackfish_engine::script::ScriptRuntime::new();
    assert!(runtime.build_operation(BAD_ZONE).is_err());
}


