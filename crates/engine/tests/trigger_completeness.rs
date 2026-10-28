//! CR 701.22d, 701.25d: `ConditionPrimitive::Scries` and `Surveils` match `CardsScried` and `CardsSurveiled`. CR 700.4, 603.10: `DiesMatching` and `LeavesBattlefieldMatching(FilterSpec)` ("whenever a creature you control dies") are checked in `collect_triggers`' live-source loop and not the departed/LKI loop.
//! CR 603.4: `InterveningIfSpec::Script` is invoked through `invoke_intervening_if` with a state snapshot, and checked at trigger time and again at resolution.

use std::collections::HashMap;

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::triggers::collect_triggers;
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, player_state, two_player_state, zones};

fn creature_definition(name: &str, card_types: CardTypeSet) -> CardDefinition {
    CardDefinition {
        definition_id: 1,
        name: name.to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: Some(PTValue::Fixed(2)),
        toughness: Some(PTValue::Fixed(2)),
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

fn base_state(p0_life: i32) -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        players: vec![PlayerState { life: p0_life, ..player_state(p0) }, player_state(p1)],
        step: Step::PreCombatMain,
        last_active_turn_number: HashMap::from([(p0, 1), (p1, 1)]),
        ..two_player_state(zones(p0, p1, &[ZoneKind::Hand, ZoneKind::Battlefield, ZoneKind::Graveyard, ZoneKind::Exile]))
    }
}

fn put_on_battlefield(state: &mut GameState, object: GameObject) {
    let id = object.object_id;
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(id);
    state.objects.insert(id, object);
}

fn zone_change_event(object: ObjectId, from: ZoneKind, to: ZoneKind, last_known_info: Option<GameObject>) -> GameEvent {
    GameEvent {
        event_id: EventId(0),
        sequence_no: SequenceNo(0),
        kind: GameEventKind::ZoneChange { object, from, to, last_known_info },
        caused_by_command: None,
        parent_event: None,
    }
}

fn cards_scried_event(player: PlayerId) -> GameEvent {
    GameEvent {
        event_id: EventId(0),
        sequence_no: SequenceNo(0),
        kind: GameEventKind::CardsScried { player, kept_on_top: Vec::new(), put_bottom: Vec::new() },
        caused_by_command: None,
        parent_event: None,
    }
}

fn cards_surveiled_event(player: PlayerId) -> GameEvent {
    GameEvent {
        event_id: EventId(0),
        sequence_no: SequenceNo(0),
        kind: GameEventKind::CardsSurveiled { player, kept_on_top: Vec::new(), put_graveyard: Vec::new() },
        caused_by_command: None,
        parent_event: None,
    }
}

fn watcher_ability(source: ObjectId, condition: ConditionPrimitive) -> RegisteredAbility {
    RegisteredAbility {
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
        id: AbilityId(0),
        source,
        kind: AbilityKind::Triggered,
        condition: Some(ConditionSpec::Primitive(condition)),
        cost: None,
        target: None,
        operation: ScriptHandle(0),
        effect: Some(AbilityEffect::GainLife(1)),
        intervening_if: None,
    }
}

#[test]
fn scries_fires_for_the_controllers_own_scry_not_a_different_players() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let watcher = ObjectId(0);
    let mut state = base_state(20);
    put_on_battlefield(&mut state, card_object(watcher, creature_definition("Watcher", CardTypeSet::CREATURE), p0, ZoneKind::Battlefield));
    state.abilities.push(watcher_ability(watcher, ConditionPrimitive::Scries));
    let runtime = ScriptRuntime::new();

    let own_scry = cards_scried_event(p0);
    let (pending, _) = collect_triggers(&state, &own_scry, &runtime, &[]);
    assert_eq!(pending.len(), 1, "Scries fires for the ability controller's own scry");

    let other_scry = cards_scried_event(p1);
    let (pending, _) = collect_triggers(&state, &other_scry, &runtime, &[]);
    assert!(pending.is_empty(), "Scries must not fire for a different player's scry");
}

#[test]
fn each_scries_fires_regardless_of_player() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let watcher = ObjectId(0);
    let mut state = base_state(20);
    put_on_battlefield(&mut state, card_object(watcher, creature_definition("Watcher", CardTypeSet::CREATURE), p0, ZoneKind::Battlefield));
    state.abilities.push(watcher_ability(watcher, ConditionPrimitive::EachScries));
    let runtime = ScriptRuntime::new();

    let (pending, _) = collect_triggers(&state, &cards_scried_event(p1), &runtime, &[]);
    assert_eq!(pending.len(), 1, "EachScries fires regardless of who scried");
}

#[test]
fn surveils_fires_for_the_controllers_own_surveil_not_a_different_players() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let watcher = ObjectId(0);
    let mut state = base_state(20);
    put_on_battlefield(&mut state, card_object(watcher, creature_definition("Watcher", CardTypeSet::CREATURE), p0, ZoneKind::Battlefield));
    state.abilities.push(watcher_ability(watcher, ConditionPrimitive::Surveils));
    let runtime = ScriptRuntime::new();

    let (pending, _) = collect_triggers(&state, &cards_surveiled_event(p0), &runtime, &[]);
    assert_eq!(pending.len(), 1);

    let (pending, _) = collect_triggers(&state, &cards_surveiled_event(p1), &runtime, &[]);
    assert!(pending.is_empty(), "Surveils must not fire for a different player's surveil");
}

#[test]
fn each_surveils_fires_regardless_of_player() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let watcher = ObjectId(0);
    let mut state = base_state(20);
    put_on_battlefield(&mut state, card_object(watcher, creature_definition("Watcher", CardTypeSet::CREATURE), p0, ZoneKind::Battlefield));
    state.abilities.push(watcher_ability(watcher, ConditionPrimitive::EachSurveils));
    let runtime = ScriptRuntime::new();

    let (pending, _) = collect_triggers(&state, &cards_surveiled_event(p1), &runtime, &[]);
    assert_eq!(pending.len(), 1, "EachSurveils fires regardless of who surveiled");
}

fn creature_only_filter(controller: Option<FilterController>) -> FilterSpec {
    FilterSpec { card_types: Some(CardTypeSet::CREATURE), supertypes: None, subtypes: None, controller, random: false, extra: None }
}

#[test]
fn dies_matching_fires_when_a_creature_you_control_dies() {
    let p0 = PlayerId(0);
    let watcher = ObjectId(0);
    let dying_creature = ObjectId(1);
    let mut state = base_state(20);
    put_on_battlefield(&mut state, card_object(watcher, creature_definition("Watcher", CardTypeSet::ARTIFACT), p0, ZoneKind::Battlefield));
    state.abilities.push(watcher_ability(
        watcher,
        ConditionPrimitive::DiesMatching(creature_only_filter(Some(FilterController::You))),
    ));
    let runtime = ScriptRuntime::new();

    let dying = card_object(dying_creature, creature_definition("Bear", CardTypeSet::CREATURE), p0, ZoneKind::Battlefield);
    let event = zone_change_event(ObjectId(2), ZoneKind::Battlefield, ZoneKind::Graveyard, Some(dying));
    let (pending, _) = collect_triggers(&state, &event, &runtime, &[]);
    assert_eq!(pending.len(), 1, "a creature the watcher's controller controls died");
    assert_eq!(pending[0].source, watcher);
}

#[test]
fn dies_matching_does_not_fire_for_a_noncreature_permanent_dying() {
    let p0 = PlayerId(0);
    let watcher = ObjectId(0);
    let mut state = base_state(20);
    put_on_battlefield(&mut state, card_object(watcher, creature_definition("Watcher", CardTypeSet::ARTIFACT), p0, ZoneKind::Battlefield));
    state.abilities.push(watcher_ability(
        watcher,
        ConditionPrimitive::DiesMatching(creature_only_filter(None)),
    ));
    let runtime = ScriptRuntime::new();

    let dying_land = card_object(ObjectId(1), creature_definition("Test Land", CardTypeSet::LAND), p0, ZoneKind::Battlefield);
    let event = zone_change_event(ObjectId(2), ZoneKind::Battlefield, ZoneKind::Graveyard, Some(dying_land));
    let (pending, _) = collect_triggers(&state, &event, &runtime, &[]);
    assert!(pending.is_empty(), "the filter is CREATURE-only - a dying land shouldn't match");
}

#[test]
fn dies_matching_does_not_fire_for_an_opponents_creature_when_filter_is_you_only() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let watcher = ObjectId(0);
    let mut state = base_state(20);
    put_on_battlefield(&mut state, card_object(watcher, creature_definition("Watcher", CardTypeSet::ARTIFACT), p0, ZoneKind::Battlefield));
    state.abilities.push(watcher_ability(
        watcher,
        ConditionPrimitive::DiesMatching(creature_only_filter(Some(FilterController::You))),
    ));
    let runtime = ScriptRuntime::new();

    // p1's creature dies - the watcher's controller (p0) doesn't control it.
    let dying = card_object(ObjectId(1), creature_definition("Bear", CardTypeSet::CREATURE), p1, ZoneKind::Battlefield);
    let event = zone_change_event(ObjectId(2), ZoneKind::Battlefield, ZoneKind::Graveyard, Some(dying));
    let (pending, _) = collect_triggers(&state, &event, &runtime, &[]);
    assert!(pending.is_empty(), "FilterController::You means the watcher's own controller, not the dying creature's");
}

#[test]
fn leaves_battlefield_matching_fires_on_exile_not_just_death() {
    let p0 = PlayerId(0);
    let watcher = ObjectId(0);
    let mut state = base_state(20);
    put_on_battlefield(&mut state, card_object(watcher, creature_definition("Watcher", CardTypeSet::ARTIFACT), p0, ZoneKind::Battlefield));
    state.abilities.push(watcher_ability(
        watcher,
        ConditionPrimitive::LeavesBattlefieldMatching(creature_only_filter(Some(FilterController::You))),
    ));
    let runtime = ScriptRuntime::new();

    let exiled = card_object(ObjectId(1), creature_definition("Bear", CardTypeSet::CREATURE), p0, ZoneKind::Battlefield);
    let event = zone_change_event(ObjectId(2), ZoneKind::Battlefield, ZoneKind::Exile, Some(exiled));
    let (pending, _) = collect_triggers(&state, &event, &runtime, &[]);
    assert_eq!(pending.len(), 1, "LeavesBattlefieldMatching fires on exile, unlike DiesMatching");
}

#[test]
fn game_conditions_dies_matching_lua_binding_decodes_correctly() {
    let mut runtime = ScriptRuntime::new();
    let condition = runtime
        .load_condition(r#"return Game.Conditions.DiesMatching({ card_types = { "Creature" }, controller = "you" })"#)
        .expect("primitive loads");
    let ConditionSpec::Primitive(ConditionPrimitive::DiesMatching(filter)) = condition else {
        panic!("expected ConditionSpec::Primitive(DiesMatching(_)), got {condition:?}")
    };
    assert_eq!(filter.card_types, Some(CardTypeSet::CREATURE));
    assert_eq!(filter.controller, Some(FilterController::You));
}

// TODO: expand intervening-if Lua snapshot beyond per-player life totals.
const AT_OR_BELOW_LIFE_5_SCRIPT: &str = r#"
return function(source, controller, snapshot)
    return snapshot.life[controller] <= 5
end
"#;

fn interveningif_upkeep_ability_script(source: ObjectId, handle: ScriptHandle) -> RegisteredAbility {
    RegisteredAbility {
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
        id: AbilityId(1),
        source,
        kind: AbilityKind::Triggered,
        condition: Some(ConditionSpec::Primitive(ConditionPrimitive::BeginningOfYourUpkeep)),
        cost: None,
        target: None,
        operation: ScriptHandle(0),
        effect: Some(AbilityEffect::GainLife(1)),
        intervening_if: Some(InterveningIfSpec::Script(handle)),
    }
}

fn heal_spell_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 2,
        name: "Test Heal".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::INSTANT,
        subtypes: Vec::new(),
        oracle_text: "You gain 10 life.".to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_heal".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: Some(AbilityEffect::GainLife(10)),
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

// Adds the Untap-step and PreGame start so pass_both reaches upkeep, and a Hand for the response spell.
fn intervening_if_state(p0_life: i32) -> GameState {
    let mut state = base_state(p0_life);
    state.phase = Phase::PreGame;
    state.step = Step::Untap;
    state
}

fn pass_both(engine: &mut Engine, p0: PlayerId, p1: PlayerId) -> StepStatus {
    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine);
    engine.submit_action(PlayerAction { player: p1, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine)
}

fn cast_spell(engine: &mut Engine, player: PlayerId, card: ObjectId) {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::CastSpell { card, modes: Vec::new(), targets: Vec::new(), x_value: None, alternative_cost: None , pay_buyback: false},
        })
        .expect("player holds priority, spell is in hand");
}

fn pay_free_and_drain(engine: &mut Engine) -> StepStatus {
    let status = drain(engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }));
    engine.submit_payment(PaymentPlan { payments: Vec::new() }).expect("empty plan pays a {0} cost");
    drain(engine)
}

#[test]
fn script_intervening_if_false_at_trigger_time_never_triggers_at_all() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let permanent = ObjectId(0);
    let mut state = intervening_if_state(10);
    put_on_battlefield(&mut state, card_object(permanent, creature_definition("Bear", CardTypeSet::CREATURE), p0, ZoneKind::Battlefield));

    let mut engine = Engine::new(state);
    let handle = engine.load_script(AT_OR_BELOW_LIFE_5_SCRIPT).expect("script loads");
    engine.state_mut().abilities.push(interveningif_upkeep_ability_script(permanent, handle));

    let status = pass_both(&mut engine, p0, p1);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert!(engine.state().stack.is_empty(), "life 10 > 5 - the Script intervening-if is false at trigger time");
}

#[test]
fn script_intervening_if_true_throughout_resolves_normally() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let permanent = ObjectId(0);
    let mut state = intervening_if_state(5);
    put_on_battlefield(&mut state, card_object(permanent, creature_definition("Bear", CardTypeSet::CREATURE), p0, ZoneKind::Battlefield));

    let mut engine = Engine::new(state);
    let handle = engine.load_script(AT_OR_BELOW_LIFE_5_SCRIPT).expect("script loads");
    engine.state_mut().abilities.push(interveningif_upkeep_ability_script(permanent, handle));

    let status = pass_both(&mut engine, p0, p1);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert_eq!(engine.state().stack.len(), 1, "life 5 <= 5 - true at trigger time");

    let status = pass_both(&mut engine, p0, p1);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert!(engine.state().stack.is_empty(), "resolved");
    assert_eq!(engine.state().player(p0).unwrap().life, 6, "GainLife(1) applied");
}

#[test]
fn script_intervening_if_rechecked_fresh_at_resolution_fizzles() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let permanent = ObjectId(0);
    let mut state = intervening_if_state(3);
    put_on_battlefield(&mut state, card_object(permanent, creature_definition("Bear", CardTypeSet::CREATURE), p0, ZoneKind::Battlefield));
    let heal = ObjectId(1);
    state.objects.insert(heal, card_object(heal, heal_spell_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(heal);

    let mut engine = Engine::new(state);
    let handle = engine.load_script(AT_OR_BELOW_LIFE_5_SCRIPT).expect("script loads");
    engine.state_mut().abilities.push(interveningif_upkeep_ability_script(permanent, handle));

    let status = pass_both(&mut engine, p0, p1);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert_eq!(engine.state().stack.len(), 1, "life 3 <= 5 - true at trigger time");

    cast_spell(&mut engine, p0, heal);
    pay_free_and_drain(&mut engine);
    assert_eq!(engine.state().stack.len(), 2);

    let status = pass_both(&mut engine, p0, p1);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert_eq!(engine.state().player(p0).unwrap().life, 13, "Test Heal resolved first");
    assert_eq!(engine.state().stack.len(), 1);

    let status = pass_both(&mut engine, p0, p1);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert!(engine.state().stack.is_empty());
    assert_eq!(
        engine.state().player(p0).unwrap().life,
        13,
        "GainLife(1) did NOT apply - life 13 > 5 at resolution, the Script intervening-if now returns false"
    );
    assert!(engine.state().ledger.iter().any(|e| matches!(e.kind, GameEventKind::Fizzled { .. })));
}

#[test]
fn set_intervening_if_accepts_a_function_and_self_registers() {
    const SELF_REGISTER_WITH_SCRIPT_INTERVENING_IF: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateTriggeredAbility(c)
    e:SetCondition(Game.Conditions.Upkeep)
    e:SetInterveningIf(function(source, controller, snapshot)
        return snapshot.life[controller] <= 5
    end)
    e:SetOperation(Game.Ops.GainLife(1))
    e:Register()
end
return s
"#;
    let mut state = base_state(3);
    let bear = ObjectId(0);
    let definition = CardDefinition {
        definition_id: 1,
        name: "Test Bear".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::CREATURE,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: Some(PTValue::Fixed(2)),
        toughness: Some(PTValue::Fixed(2)),
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::Inline(SELF_REGISTER_WITH_SCRIPT_INTERVENING_IF.to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    };
    state.objects.insert(bear, card_object(bear, definition.clone(), PlayerId(0), ZoneKind::Battlefield));

    let mut runtime = ScriptRuntime::new();
    runtime.run_card_script(&mut state, &definition, bear).expect("script self-registers cleanly");

    assert_eq!(state.abilities.len(), 1);
    let ability = &state.abilities[0];
    assert!(matches!(ability.intervening_if, Some(InterveningIfSpec::Script(_))));

    // The registered handle is callable and correct, with p0's life (3) <= 5.
    let Some(InterveningIfSpec::Script(handle)) = ability.intervening_if else { unreachable!() };
    assert!(runtime.invoke_intervening_if(handle, &state, bear, PlayerId(0)).expect("script runs"));
}
