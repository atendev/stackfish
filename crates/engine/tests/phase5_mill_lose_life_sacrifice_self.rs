//! Phase 5 vertical slice: mill, lose life and sacrifice self.
//!
//! `Game.Ops.Mill(n)` (CR 701.17) moves the top of a library to its owner's
//! graveyard one replacement-checked card at a time. `Game.Ops.LoseLife(n)`
//! (CR 119.3) subtracts life and records a `LifeChanged` event.
//! `Game.Ops.SacrificeSelf` (CR 701.21a) sacrifices the resolving ability's own
//! source. Mill and LoseLife act on the controller when the ability declares no
//! target spec and on the first legal player target when it does.

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, minimal_card_definition, two_player_state as base_two_player_state, zones};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const SPELL: ObjectId = ObjectId(100);

const SACRIFICE_SELF_ACTIVATED: &str = r#"
local s = {}
function s.register_abilities(c)
    local a = Game.CreateActivatedAbility(c)
    a:SetOperation(Game.Ops.SacrificeSelf)
    a:Register()
end
return s
"#;

const SACRIFICE_SELF_WITH_DIES_TRIGGER: &str = r#"
local s = {}
function s.register_abilities(c)
    local a = Game.CreateActivatedAbility(c)
    a:SetOperation(Game.Ops.SacrificeSelf)
    a:Register()
    local t = Game.CreateTriggeredAbility(c)
    t:SetCondition(Game.Conditions.Dies)
    t:SetOperation(Game.Ops.GainLife(1))
    t:Register()
end
return s
"#;

const MILL_ACTIVATED: &str = r#"
local s = {}
function s.register_abilities(c)
    local a = Game.CreateActivatedAbility(c)
    a:SetOperation(Game.Ops.Mill(2))
    a:Register()
end
return s
"#;

const SACRIFICE_SELF_COST_THEN_MILL: &str = r#"
local s = {}
function s.register_abilities(c)
    local a = Game.CreateActivatedAbility(c)
    a:AddCost(Game.Costs.SacrificeSelf)
    a:SetOperation(Game.Ops.Mill(2))
    a:Register()
end
return s
"#;

const DIES_MILL_TRIGGER: &str = r#"
local s = {}
function s.register_abilities(c)
    local a = Game.CreateActivatedAbility(c)
    a:SetOperation(Game.Ops.SacrificeSelf)
    a:Register()
    local t = Game.CreateTriggeredAbility(c)
    t:SetCondition(Game.Conditions.Dies)
    t:SetOperation(Game.Ops.Mill(2))
    t:Register()
end
return s
"#;

const LOSE_LIFE_ACTIVATED_TARGETED: &str = r#"
local s = {}
function s.register_abilities(c)
    local a = Game.CreateActivatedAbility(c)
    a:SetTarget({ type = "player", controller = "opponent" })
    a:SetOperation(Game.Ops.LoseLife(3))
    a:Register()
end
return s
"#;

const LUA_MILL_THREE: &str = r#"
local s = {}
function s.operation() return Game.Ops.Mill(3) end
return s
"#;

const LUA_LOSE_LIFE_NEGATIVE: &str = r#"
local s = {}
function s.operation() return Game.Ops.LoseLife(-1) end
return s
"#;

const LUA_EACH_PLAYER_MILL: &str = r#"
local s = {}
function s.operation() return Game.Ops.ForEachPlayer(Game.Ops.Mill(1)) end
return s
"#;

const LUA_EACH_PLAYER_DAMAGE: &str = r#"
local s = {}
function s.operation() return Game.Ops.ForEachPlayer(Game.Ops.Damage(1)) end
return s
"#;

const LUA_SACRIFICE_SELF: &str = r#"
local s = {}
function s.operation() return Game.Ops.SacrificeSelf end
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

fn player_target(up_to: bool) -> TargetSpec {
    TargetSpec::Primitive(TargetRequirement {
        filter: TargetFilter::Player { controller: None },
        count: 1,
        up_to,
        at_least: false,
        exclude_source: false,
    })
}

fn spell(effect: AbilityEffect, target: Option<TargetSpec>) -> CardDefinition {
    CardDefinition {
        effect: Some(effect),
        target_requirement: target,
        ..minimal_card_definition(99, "Test Spell", "unused", ManaCost { symbols: Vec::new() }, CardTypeSet::INSTANT)
    }
}

struct Placed {
    id: u64,
    name: &'static str,
    types: CardTypeSet,
    script: Option<&'static str>,
    owner: PlayerId,
    zone: ZoneKind,
    token: bool,
}

fn placed(id: u64, name: &'static str, types: CardTypeSet, owner: PlayerId, zone: ZoneKind) -> Placed {
    Placed { id, name, types, script: None, owner, zone, token: false }
}

fn library_card(id: u64, name: &'static str, owner: PlayerId) -> Placed {
    placed(id, name, CardTypeSet::LAND, owner, ZoneKind::Library)
}

fn bear(id: u64, script: &'static str) -> Placed {
    Placed { script: Some(script), ..placed(id, "Bear", CardTypeSet::CREATURE, P0, ZoneKind::Battlefield) }
}

fn engine_with(cards: Vec<Placed>, spell_def: Option<CardDefinition>) -> Engine {
    let mut state = GameState {
        step: Step::PreCombatMain,
        ..base_two_player_state(zones(P0, P1, &[ZoneKind::Hand, ZoneKind::Library, ZoneKind::Battlefield, ZoneKind::Graveyard, ZoneKind::Exile]))
    };
    let mut runtime = ScriptRuntime::new();
    for c in cards {
        let mut object = card_object(ObjectId(c.id), card(c.id, c.name, c.types, c.script), c.owner, c.zone);
        object.is_token = c.token;
        state.objects.insert(ObjectId(c.id), object);
        let owner = if matches!(c.zone, ZoneKind::Battlefield | ZoneKind::Exile) { None } else { Some(c.owner) };
        state.zone_mut(c.zone, owner).unwrap().push_bottom(ObjectId(c.id));
        // Fixture battlefield cards never "entered", so register their script by hand.
        if c.zone == ZoneKind::Battlefield && c.script.is_some() {
            let definition = state.objects[&ObjectId(c.id)].definition.clone();
            runtime.run_card_script(&mut state, &definition, ObjectId(c.id)).expect("script registers");
        }
    }
    if let Some(def) = spell_def {
        state.objects.insert(SPELL, card_object(SPELL, def, P0, ZoneKind::Hand));
        state.zone_mut(ZoneKind::Hand, Some(P0)).unwrap().push_bottom(SPELL);
    }
    Engine::new(state)
}

fn pass_both(engine: &mut Engine) {
    for player in [P0, P1] {
        engine.submit_action(PlayerAction { player, kind: PlayerActionKind::PassPriority }).unwrap();
        drain(engine);
    }
}

/// Passes priority until the stack is empty (a trigger needs a second round).
fn resolve_all(engine: &mut Engine) {
    for _ in 0..6 {
        if engine.state().stack.is_empty() {
            break;
        }
        pass_both(engine);
    }
}

fn cast(engine: &mut Engine, targets: Vec<Target>) {
    engine
        .submit_action(PlayerAction {
            player: P0,
            kind: PlayerActionKind::CastSpell { card: SPELL, modes: Vec::new(), targets, x_value: None, alternative_cost: None, pay_buyback: false },
        })
        .expect("p0 holds priority");
    let status = drain(engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }), "{status:?}");
    engine.submit_payment(PaymentPlan { payments: Vec::new() }).unwrap();
    drain(engine);
}

fn cast_and_resolve(engine: &mut Engine, targets: Vec<Target>) {
    cast(engine, targets);
    pass_both(engine);
}

fn activate(engine: &mut Engine, source: u64, targets: Vec<Target>, payments: Vec<CostPayment>) {
    engine
        .submit_action(PlayerAction {
            player: P0,
            kind: PlayerActionKind::ActivateAbility { source: ObjectId(source), ability_index: 0, targets, chosen_colors: Vec::new(), x_value: None },
        })
        .expect("activation is legal");
    let status = drain(engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }), "{status:?}");
    engine.submit_payment(PaymentPlan { payments }).unwrap();
    drain(engine);
}

fn activate_and_resolve(engine: &mut Engine, source: u64, targets: Vec<Target>, payments: Vec<CostPayment>) {
    activate(engine, source, targets, payments);
    resolve_all(engine);
}

/// Card names in a zone in zone order (top of a library first), excluding the test spell.
fn names_ordered(engine: &Engine, zone: ZoneKind, owner: Option<PlayerId>) -> Vec<String> {
    engine
        .state()
        .zone(zone, owner)
        .unwrap()
        .contents
        .iter()
        .filter_map(|id| engine.state().object(*id))
        .map(|o| o.definition.name.clone())
        .filter(|n| n != "Test Spell")
        .collect()
}

fn life(engine: &Engine, player: PlayerId) -> i32 {
    engine.state().player(player).unwrap().life
}

fn life_changes(engine: &Engine) -> Vec<(PlayerId, i32)> {
    engine
        .state()
        .ledger
        .iter()
        .filter_map(|e| match e.kind {
            GameEventKind::LifeChanged { player, delta } => Some((player, delta)),
            _ => None,
        })
        .collect()
}

fn five_cards(owner: PlayerId, first_id: u64) -> Vec<Placed> {
    ["A", "B", "C", "D", "E"].iter().enumerate().map(|(i, n)| library_card(first_id + i as u64, n, owner)).collect()
}

fn library_to_graveyard_filter() -> EffectFilter {
    EffectFilter { controller: None, card_types: None, subtypes: None, excluded_subtypes: None, supertypes: None, excluded_supertypes: None, self_only: false }
}

fn library_redirect(source: u64, to: ZoneKind) -> ReplacementEffect {
    ReplacementEffect {
        source: ObjectId(source),
        spec: ReplacementSpec::Primitive {
            filter: EventFilter::WouldMoveZone { from: Some(ZoneKind::Library), to: Some(ZoneKind::Graveyard), affected: library_to_graveyard_filter() },
            payload: ReplacementPayload::ReplaceZoneDestination(to),
        },
        consumption: ConsumptionPolicy::Repeatable,
    }
}

// ---- Mill -------------------------------------------------------------------

#[test]
fn mill_without_a_target_spec_mills_the_controller_top_down() {
    let mut engine = engine_with(five_cards(P0, 1), Some(spell(AbilityEffect::Mill(2), None)));
    cast_and_resolve(&mut engine, Vec::new());

    assert_eq!(names_ordered(&engine, ZoneKind::Graveyard, Some(P0)), vec!["A", "B"]);
    assert_eq!(names_ordered(&engine, ZoneKind::Library, Some(P0)), vec!["C", "D", "E"]);
}

#[test]
fn mill_with_a_player_target_mills_that_player_not_the_controller() {
    let mut cards = five_cards(P0, 1);
    cards.extend(["X", "Y", "Z"].iter().enumerate().map(|(i, n)| library_card(11 + i as u64, n, P1)));
    let mut engine = engine_with(cards, Some(spell(AbilityEffect::Mill(2), Some(player_target(false)))));
    cast_and_resolve(&mut engine, vec![Target::Player(P1)]);

    assert_eq!(names_ordered(&engine, ZoneKind::Graveyard, Some(P1)), vec!["X", "Y"]);
    assert_eq!(names_ordered(&engine, ZoneKind::Library, Some(P1)), vec!["Z"]);
    assert_eq!(names_ordered(&engine, ZoneKind::Library, Some(P0)).len(), 5, "the controller's library is untouched");
}

#[test]
fn mill_more_than_the_library_holds_mills_what_exists() {
    let mut engine = engine_with(vec![library_card(1, "A", P0), library_card(2, "B", P0)], Some(spell(AbilityEffect::Mill(5), None)));
    cast_and_resolve(&mut engine, Vec::new());

    assert_eq!(names_ordered(&engine, ZoneKind::Graveyard, Some(P0)), vec!["A", "B"], "CR 701.17b");
    assert!(names_ordered(&engine, ZoneKind::Library, Some(P0)).is_empty());
    assert!(engine.state().stack.is_empty());
}

#[test]
fn milling_an_empty_library_is_not_a_draw_and_does_not_lose_the_game() {
    let mut engine = engine_with(Vec::new(), Some(spell(AbilityEffect::Mill(3), None)));
    cast_and_resolve(&mut engine, Vec::new());

    let player = engine.state().player(P0).unwrap();
    assert!(!player.drew_from_empty_library, "CR 704.5b needs a draw attempt, a mill is not one");
    assert!(!player.eliminated);
    assert!(engine.state().stack.is_empty());
}

#[test]
fn milled_cards_are_new_objects_and_log_library_to_graveyard_moves() {
    let mut engine = engine_with(five_cards(P0, 1), Some(spell(AbilityEffect::Mill(2), None)));
    cast_and_resolve(&mut engine, Vec::new());

    let graveyard = engine.state().zone(ZoneKind::Graveyard, Some(P0)).unwrap().contents.clone();
    let milled: Vec<&GameObject> = graveyard.iter().filter_map(|id| engine.state().object(*id)).filter(|o| o.definition.name != "Test Spell").collect();
    assert_eq!(milled.len(), 2);
    for object in milled {
        assert!(object.object_id != ObjectId(1) && object.object_id != ObjectId(2), "CR 400.7: a fresh id");
        assert!(object.lineage.previous.is_some());
    }
    let moves = engine
        .state()
        .ledger
        .iter()
        .filter(|e| matches!(&e.kind, GameEventKind::ZoneChange { from: ZoneKind::Library, to: ZoneKind::Graveyard, .. }))
        .count();
    assert_eq!(moves, 2);
}

#[test]
fn a_leyline_style_replacement_sends_milled_cards_to_exile() {
    let mut cards = five_cards(P0, 1);
    cards.push(placed(900, "Leyline", CardTypeSet::ENCHANTMENT, P0, ZoneKind::Battlefield));
    let mut engine = engine_with(cards, Some(spell(AbilityEffect::Mill(2), None)));
    engine.state_mut().replacement_effects.push(library_redirect(900, ZoneKind::Exile));
    cast_and_resolve(&mut engine, Vec::new());

    assert!(names_ordered(&engine, ZoneKind::Graveyard, Some(P0)).is_empty(), "CR 614: the graveyard move was replaced");
    assert_eq!(names_ordered(&engine, ZoneKind::Exile, None), vec!["A", "B"], "committed through commit_zone_move, so the shared Exile zone works");
}

#[test]
fn two_competing_replacements_pause_each_card_and_the_mill_resumes() {
    let mut cards = five_cards(P0, 1);
    cards.push(placed(901, "Exiler", CardTypeSet::ENCHANTMENT, P0, ZoneKind::Battlefield));
    cards.push(placed(902, "Bouncer", CardTypeSet::ENCHANTMENT, P0, ZoneKind::Battlefield));
    let mut engine = engine_with(cards, Some(spell(AbilityEffect::Mill(3), None)));
    engine.state_mut().replacement_effects.push(library_redirect(901, ZoneKind::Exile));
    engine.state_mut().replacement_effects.push(library_redirect(902, ZoneKind::Hand));
    cast(&mut engine, Vec::new());
    let status = {
        engine.submit_action(PlayerAction { player: P0, kind: PlayerActionKind::PassPriority }).unwrap();
        drain(&mut engine);
        engine.submit_action(PlayerAction { player: P1, kind: PlayerActionKind::PassPriority }).unwrap();
        drain(&mut engine)
    };
    assert!(matches!(status, StepStatus::AwaitingReplacementChoice { .. }), "CR 616.1: {status:?}");

    let answers = [901, 902, 901];
    let mut status = status;
    for answer in answers {
        assert!(matches!(status, StepStatus::AwaitingReplacementChoice { .. }), "{status:?}");
        engine.submit_replacement_choice(stackfish_engine::replacement::ReplacementCandidate::Registered(ObjectId(answer))).unwrap();
        status = drain(&mut engine);
    }

    assert_eq!(names_ordered(&engine, ZoneKind::Exile, None), vec!["A", "C"]);
    assert!(names_ordered(&engine, ZoneKind::Hand, Some(P0)).contains(&"B".to_string()));
    assert_eq!(names_ordered(&engine, ZoneKind::Library, Some(P0)), vec!["D", "E"], "exactly three cards left the library");
    assert!(engine.state().stack.is_empty());
}

#[test]
fn a_declared_player_target_that_chose_nobody_does_nothing_and_never_falls_back_to_the_controller() {
    let mut engine = engine_with(five_cards(P0, 1), Some(spell(AbilityEffect::Mill(2), Some(player_target(true)))));
    cast_and_resolve(&mut engine, Vec::new());

    assert_eq!(names_ordered(&engine, ZoneKind::Library, Some(P0)).len(), 5, "an \"up to one target player\" spec with no player must not mill the controller");
    assert!(names_ordered(&engine, ZoneKind::Graveyard, Some(P0)).is_empty());
    assert!(engine.state().stack.is_empty());
}

/// Pushes an already-announced ability whose player target no longer exists.
fn push_ability_with_gone_player(engine: &mut Engine, effect: AbilityEffect) {
    let stack_id = ObjectId(50);
    engine.state_mut().stack.push(StackObject::new(
        stack_id,
        StackEntryKind::ActivatedAbility,
        ObjectId(1),
        P0,
        P0,
        Vec::new(),
        vec![Target::Player(PlayerId(9))],
        Some(player_target(false)),
        None,
        false,
        false,
        false,
        ScriptHandle(0),
        Some(effect),
        None,
        None,
        None,
    ));
}

#[test]
fn a_player_target_that_became_illegal_fizzles_without_touching_the_controller() {
    let mut cards = five_cards(P0, 10);
    cards.push(placed(1, "Source", CardTypeSet::ARTIFACT, P0, ZoneKind::Battlefield));
    let mut engine = engine_with(cards, None);
    push_ability_with_gone_player(&mut engine, AbilityEffect::Mill(2));
    pass_both(&mut engine);

    assert_eq!(names_ordered(&engine, ZoneKind::Library, Some(P0)).len(), 5);
    assert!(engine.state().ledger.iter().any(|e| matches!(e.kind, GameEventKind::Fizzled { .. })));

    let mut engine = engine_with(vec![placed(1, "Source", CardTypeSet::ARTIFACT, P0, ZoneKind::Battlefield)], None);
    let before = life(&engine, P0);
    push_ability_with_gone_player(&mut engine, AbilityEffect::LoseLife(3));
    pass_both(&mut engine);
    assert_eq!(life(&engine, P0), before, "no controller fallback for lose life either");
}

#[test]
fn mill_zero_moves_nothing() {
    let mut engine = engine_with(five_cards(P0, 1), Some(spell(AbilityEffect::Mill(0), None)));
    cast_and_resolve(&mut engine, Vec::new());

    assert_eq!(names_ordered(&engine, ZoneKind::Library, Some(P0)).len(), 5);
    assert!(engine.state().stack.is_empty());
}

#[test]
fn an_activated_mill_ability_resolves_end_to_end() {
    let mut cards = five_cards(P0, 10);
    cards.push(bear(1, MILL_ACTIVATED));
    let mut engine = engine_with(cards, None);
    activate_and_resolve(&mut engine, 1, Vec::new(), Vec::new());

    assert_eq!(names_ordered(&engine, ZoneKind::Graveyard, Some(P0)), vec!["A", "B"]);
}

#[test]
fn a_triggered_mill_ability_resolves_end_to_end() {
    let mut cards = five_cards(P0, 10);
    cards.push(bear(1, DIES_MILL_TRIGGER));
    let mut engine = engine_with(cards, None);
    activate_and_resolve(&mut engine, 1, Vec::new(), Vec::new());

    let graveyard = names_ordered(&engine, ZoneKind::Graveyard, Some(P0));
    assert_eq!(graveyard, vec!["Bear", "A", "B"], "the sacrificed source dies, then its Dies trigger mills two");
}

#[test]
fn a_mill_ability_still_resolves_after_its_source_was_sacrificed_as_a_cost() {
    let mut cards = five_cards(P0, 10);
    cards.push(bear(1, SACRIFICE_SELF_COST_THEN_MILL));
    let mut engine = engine_with(cards, None);
    activate_and_resolve(&mut engine, 1, Vec::new(), vec![CostPayment::Sacrifice(ObjectId(1))]);

    let graveyard = names_ordered(&engine, ZoneKind::Graveyard, Some(P0));
    assert!(graveyard.contains(&"A".to_string()) && graveyard.contains(&"B".to_string()), "{graveyard:?}");
}

#[test]
fn for_each_player_mill_mills_every_library() {
    let mut cards = five_cards(P0, 1);
    cards.extend(["X", "Y", "Z"].iter().enumerate().map(|(i, n)| library_card(11 + i as u64, n, P1)));
    let effect = AbilityEffect::ForEachPlayer(Some(Box::new(AbilityEffect::Mill(2))));
    let mut engine = engine_with(cards, Some(spell(effect, None)));
    cast_and_resolve(&mut engine, Vec::new());

    assert_eq!(names_ordered(&engine, ZoneKind::Graveyard, Some(P0)), vec!["A", "B"]);
    assert_eq!(names_ordered(&engine, ZoneKind::Graveyard, Some(P1)), vec!["X", "Y"]);
}

// ---- LoseLife ---------------------------------------------------------------

#[test]
fn lose_life_without_a_target_spec_hits_the_controller_and_logs_the_change() {
    let mut engine = engine_with(Vec::new(), Some(spell(AbilityEffect::LoseLife(3), None)));
    let (mine, theirs) = (life(&engine, P0), life(&engine, P1));
    cast_and_resolve(&mut engine, Vec::new());

    assert_eq!(life(&engine, P0), mine - 3);
    assert_eq!(life(&engine, P1), theirs);
    assert_eq!(life_changes(&engine), vec![(P0, -3)]);
}

#[test]
fn lose_life_with_a_player_target_hits_only_that_player() {
    let mut engine = engine_with(Vec::new(), Some(spell(AbilityEffect::LoseLife(2), Some(player_target(false)))));
    let (mine, theirs) = (life(&engine, P0), life(&engine, P1));
    cast_and_resolve(&mut engine, vec![Target::Player(P1)]);

    assert_eq!(life(&engine, P0), mine);
    assert_eq!(life(&engine, P1), theirs - 2);
    assert_eq!(life_changes(&engine), vec![(P1, -2)]);
}

#[test]
fn losing_the_last_life_loses_the_game_and_life_is_not_clamped() {
    let mut engine = engine_with(Vec::new(), Some(spell(AbilityEffect::LoseLife(5), None)));
    engine.state_mut().player_mut(P0).unwrap().life = 2;
    cast_and_resolve(&mut engine, Vec::new());

    assert_eq!(life(&engine, P0), -3, "CR 119.3: no clamp at zero");
    assert!(engine.state().player(P0).unwrap().eliminated, "CR 704.5a");
}

#[test]
fn losing_zero_life_changes_nothing_and_logs_nothing() {
    let mut engine = engine_with(Vec::new(), Some(spell(AbilityEffect::LoseLife(0), None)));
    let before = life(&engine, P0);
    cast_and_resolve(&mut engine, Vec::new());

    assert_eq!(life(&engine, P0), before);
    assert!(life_changes(&engine).is_empty());
}

#[test]
fn a_declared_player_target_that_chose_nobody_loses_no_life() {
    let mut engine = engine_with(Vec::new(), Some(spell(AbilityEffect::LoseLife(3), Some(player_target(true)))));
    let before = life(&engine, P0);
    cast_and_resolve(&mut engine, Vec::new());

    assert_eq!(life(&engine, P0), before, "no controller fallback");
    assert!(life_changes(&engine).is_empty());
}

#[test]
fn an_activated_targeted_lose_life_ability_resolves_end_to_end() {
    let mut engine = engine_with(vec![bear(1, LOSE_LIFE_ACTIVATED_TARGETED)], None);
    let theirs = life(&engine, P1);
    activate_and_resolve(&mut engine, 1, vec![Target::Player(P1)], Vec::new());

    assert_eq!(life(&engine, P1), theirs - 3);
}

#[test]
fn signed_gain_life_still_loses_life_and_pushes_no_event() {
    let mut engine = engine_with(Vec::new(), Some(spell(AbilityEffect::GainLife(-2), None)));
    let before = life(&engine, P0);
    cast_and_resolve(&mut engine, Vec::new());

    assert_eq!(life(&engine, P0), before - 2);
    assert!(life_changes(&engine).is_empty(), "GainLife does not push LifeChanged (left as a TODO)");
}

#[test]
fn for_each_player_lose_life_hits_everyone_once() {
    let effect = AbilityEffect::ForEachPlayer(Some(Box::new(AbilityEffect::LoseLife(1))));
    let mut engine = engine_with(Vec::new(), Some(spell(effect, None)));
    let (mine, theirs) = (life(&engine, P0), life(&engine, P1));
    cast_and_resolve(&mut engine, Vec::new());

    assert_eq!((life(&engine, P0), life(&engine, P1)), (mine - 1, theirs - 1));
    assert_eq!(life_changes(&engine).len(), 2);
}

// ---- SacrificeSelf ----------------------------------------------------------

#[test]
fn sacrifice_self_moves_the_source_to_its_owners_graveyard() {
    let mut engine = engine_with(vec![bear(1, SACRIFICE_SELF_ACTIVATED)], None);
    activate_and_resolve(&mut engine, 1, Vec::new(), Vec::new());

    assert!(names_ordered(&engine, ZoneKind::Battlefield, None).is_empty());
    assert_eq!(names_ordered(&engine, ZoneKind::Graveyard, Some(P0)), vec!["Bear"]);
    let dead = engine.state().objects.values().find(|o| o.zone == ZoneKind::Graveyard).unwrap();
    assert_ne!(dead.object_id, ObjectId(1), "CR 400.7");
}

#[test]
fn indestructible_does_not_stop_a_sacrifice() {
    let mut engine = engine_with(vec![bear(1, SACRIFICE_SELF_ACTIVATED)], None);
    engine.state_mut().objects.get_mut(&ObjectId(1)).unwrap().definition.keywords = KeywordSet::INDESTRUCTIBLE;
    activate_and_resolve(&mut engine, 1, Vec::new(), Vec::new());

    assert_eq!(names_ordered(&engine, ZoneKind::Graveyard, Some(P0)), vec!["Bear"]);
}

#[test]
fn a_sacrificed_source_fires_its_dies_trigger() {
    let mut engine = engine_with(vec![bear(1, SACRIFICE_SELF_WITH_DIES_TRIGGER)], None);
    let before = life(&engine, P0);
    activate_and_resolve(&mut engine, 1, Vec::new(), Vec::new());

    assert_eq!(life(&engine, P0), before + 1);
}

#[test]
fn a_source_that_already_left_is_not_sacrificed_again() {
    let mut engine = engine_with(vec![bear(1, SACRIFICE_SELF_ACTIVATED)], None);
    activate(&mut engine, 1, Vec::new(), Vec::new());
    // The source leaves while the ability is on the stack: it is a new object (CR 400.7).
    let bear_object = engine.state_mut().objects.remove(&ObjectId(1)).unwrap();
    engine.state_mut().zone_mut(ZoneKind::Battlefield, None).unwrap().remove(ObjectId(1));
    let mut moved = bear_object;
    moved.object_id = ObjectId(77);
    moved.zone = ZoneKind::Hand;
    engine.state_mut().zone_mut(ZoneKind::Hand, Some(P0)).unwrap().push_bottom(ObjectId(77));
    engine.state_mut().objects.insert(ObjectId(77), moved);
    resolve_all(&mut engine);

    assert!(names_ordered(&engine, ZoneKind::Graveyard, Some(P0)).is_empty());
    assert_eq!(names_ordered(&engine, ZoneKind::Hand, Some(P0)), vec!["Bear"]);
}

#[test]
fn a_source_under_another_players_control_is_not_sacrificed() {
    let mut engine = engine_with(vec![bear(1, SACRIFICE_SELF_ACTIVATED)], None);
    activate(&mut engine, 1, Vec::new(), Vec::new());
    engine.state_mut().objects.get_mut(&ObjectId(1)).unwrap().controller = P1;
    resolve_all(&mut engine);

    assert_eq!(names_ordered(&engine, ZoneKind::Battlefield, None), vec!["Bear"], "CR 701.21a: P0 no longer controls it");
    assert!(names_ordered(&engine, ZoneKind::Graveyard, Some(P0)).is_empty());
}

#[test]
fn a_death_replacement_applies_to_a_self_sacrifice() {
    let cards = vec![bear(1, SACRIFICE_SELF_ACTIVATED), placed(900, "Rest", CardTypeSet::ENCHANTMENT, P0, ZoneKind::Battlefield)];
    let mut engine = engine_with(cards, None);
    engine.state_mut().replacement_effects.push(ReplacementEffect {
        source: ObjectId(900),
        spec: ReplacementSpec::Primitive {
            filter: EventFilter::WouldMoveZone {
                from: Some(ZoneKind::Battlefield),
                to: Some(ZoneKind::Graveyard),
                affected: EffectFilter { card_types: Some(CardTypeSet::CREATURE), ..library_to_graveyard_filter() },
            },
            payload: ReplacementPayload::ReplaceZoneDestination(ZoneKind::Exile),
        },
        consumption: ConsumptionPolicy::Repeatable,
    });
    activate_and_resolve(&mut engine, 1, Vec::new(), Vec::new());

    assert_eq!(names_ordered(&engine, ZoneKind::Exile, None), vec!["Bear"], "CR 614: exiled instead of dying");
}

#[test]
fn a_regeneration_shield_does_not_stop_a_sacrifice() {
    let mut engine = engine_with(vec![bear(1, SACRIFICE_SELF_ACTIVATED)], None);
    engine.state_mut().replacement_effects.push(ReplacementEffect {
        source: ObjectId(1),
        spec: ReplacementSpec::Primitive {
            filter: EventFilter::WouldDie { affected: EffectFilter { self_only: true, ..library_to_graveyard_filter() } },
            payload: ReplacementPayload::Regenerate,
        },
        consumption: ConsumptionPolicy::Repeatable,
    });
    activate_and_resolve(&mut engine, 1, Vec::new(), Vec::new());

    assert_eq!(names_ordered(&engine, ZoneKind::Graveyard, Some(P0)), vec!["Bear"], "CR 701.21a: sacrifice is not destruction");
}

#[test]
fn a_token_that_sacrifices_itself_ceases_to_exist() {
    let mut token = bear(1, SACRIFICE_SELF_ACTIVATED);
    token.token = true;
    let mut engine = engine_with(vec![token], None);
    activate_and_resolve(&mut engine, 1, Vec::new(), Vec::new());

    assert!(engine.state().objects.values().all(|o| !o.is_token), "CR 111.7");
    assert!(names_ordered(&engine, ZoneKind::Graveyard, Some(P0)).is_empty());
}

// ---- Lua --------------------------------------------------------------------

#[test]
fn lua_ops_build_the_expected_effects() {
    let runtime = ScriptRuntime::new();
    let (effect, _) = runtime.build_operation(LUA_MILL_THREE).unwrap().unwrap();
    assert!(matches!(effect, Some(AbilityEffect::Mill(3))), "{effect:?}");
    let (effect, _) = runtime.build_operation(LUA_SACRIFICE_SELF).unwrap().unwrap();
    assert!(matches!(effect, Some(AbilityEffect::SacrificeSelf)), "{effect:?}");
    let (effect, _) = runtime.build_operation(LUA_EACH_PLAYER_MILL).unwrap().unwrap();
    match effect {
        Some(AbilityEffect::ForEachPlayer(Some(inner))) => assert!(matches!(*inner, AbilityEffect::Mill(1))),
        other => panic!("{other:?}"),
    }
}

#[test]
fn lua_rejects_negative_lose_life_and_unsupported_for_each_player_tags() {
    let runtime = ScriptRuntime::new();
    assert!(runtime.build_operation(LUA_LOSE_LIFE_NEGATIVE).is_err());
    assert!(runtime.build_operation(LUA_EACH_PLAYER_DAMAGE).is_err());
}
