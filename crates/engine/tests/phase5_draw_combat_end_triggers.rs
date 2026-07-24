//! Phase 5 vertical slice: draw/beginning-of-combat/beginning-of-end-step
//! trigger conditions (CR 121/507/512-513, backlog item 7's next sub-slice
//! after upkeep). `AdvanceTurnStep` already routes every non-`Untap`/
//! non-`Cleanup` step's events through the real `CollectTriggers` pipeline
//! (`phase5_upkeep_triggers.rs`) - this slice is the "cheap follow-up" that
//! doc comment named: new `ConditionPrimitive` variants only, no further
//! plumbing changes. Same struct-literal `GameState` construction pattern as
//! `phase5_upkeep_triggers.rs`, which this file mirrors closely.

use std::collections::HashMap;

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, zones, two_player_state as base_two_player_state};

fn creature_definition(name: &str) -> CardDefinition {
    CardDefinition {
        definition_id: 1,
        name: name.to_string(),
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
        script_ref: ScriptReference::ByKey("bear".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn land_definition(name: &str) -> CardDefinition {
    CardDefinition {
        definition_id: 2,
        name: name.to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::BASIC,
        card_types: CardTypeSet::LAND,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("forest".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn ability(source: ObjectId, condition: ConditionPrimitive) -> RegisteredAbility {
    RegisteredAbility { id: AbilityId(0),
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

/// Two players, starting mid-Beginning-phase (`Phase::Beginning`/
/// `Step::Upkeep`, turn 2 so the draw isn't skipped) so a single
/// `pass_both` reaches `Step::Draw` - mirrors
/// `phase5_beginning_phase::draw_on_turn_2_or_later`'s starting point.
fn state_at_upkeep_turn_2() -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        turn_number: 2,
        phase: Phase::Beginning,
        step: Step::Upkeep,
        last_active_turn_number: HashMap::from([(p0, 2), (p1, 1)]),
        ..base_two_player_state(zones(p0, p1, &[ZoneKind::Hand, ZoneKind::Library, ZoneKind::Battlefield]))
    }
}

fn pass_both(engine: &mut Engine, p0: PlayerId, p1: PlayerId) -> StepStatus {
    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine);
    engine.submit_action(PlayerAction { player: p1, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine)
}

// ---------------------------------------------------------------------
// DrawsCard / EachDrawsCard
// ---------------------------------------------------------------------

#[test]
fn draws_card_trigger_fires_and_resolves_for_its_controllers_own_draw() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let permanent = ObjectId(0);
    let card = ObjectId(1);
    let mut state = state_at_upkeep_turn_2();
    state.objects.insert(permanent, card_object(permanent, creature_definition("Bear"), p0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(permanent);
    state.objects.insert(card, card_object(card, land_definition("Forest"), p0, ZoneKind::Library));
    state.zone_mut(ZoneKind::Library, Some(p0)).unwrap().push_bottom(card);
    state.abilities.push(ability(permanent, ConditionPrimitive::DrawsCard));

    let mut engine = Engine::new(state);
    let status = pass_both(&mut engine, p0, p1);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert_eq!(engine.state().step, Step::Draw);
    assert_eq!(engine.state().stack.len(), 1, "the DrawsCard trigger was collected and pushed for p0's own draw");

    let status = pass_both(&mut engine, p0, p1);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert!(engine.state().stack.is_empty());
    assert_eq!(engine.state().player(p0).unwrap().life, 21, "GainLife(1) applied on p0's own draw");
}

#[test]
fn draws_card_trigger_does_not_fire_for_a_different_players_draw() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let permanent = ObjectId(0);
    let card = ObjectId(1);
    let mut state = state_at_upkeep_turn_2();
    // Controlled by p1, but p0 is the active/drawing player this turn -
    // DrawsCard must not fire for p1 here.
    state.objects.insert(permanent, card_object(permanent, creature_definition("Bear"), p1, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(permanent);
    state.objects.insert(card, card_object(card, land_definition("Forest"), p0, ZoneKind::Library));
    state.zone_mut(ZoneKind::Library, Some(p0)).unwrap().push_bottom(card);
    state.abilities.push(ability(permanent, ConditionPrimitive::DrawsCard));

    let mut engine = Engine::new(state);
    let status = pass_both(&mut engine, p0, p1);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert_eq!(engine.state().step, Step::Draw);
    assert!(engine.state().stack.is_empty(), "p1's ability shouldn't fire on p0's draw");
}

#[test]
fn each_draws_card_trigger_fires_regardless_of_who_drew() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let permanent = ObjectId(0);
    let card = ObjectId(1);
    let mut state = state_at_upkeep_turn_2();
    // Controlled by p1 while p0 is the one drawing - EachDrawsCard must fire
    // anyway, unlike DrawsCard above.
    state.objects.insert(permanent, card_object(permanent, creature_definition("Bear"), p1, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(permanent);
    state.objects.insert(card, card_object(card, land_definition("Forest"), p0, ZoneKind::Library));
    state.zone_mut(ZoneKind::Library, Some(p0)).unwrap().push_bottom(card);
    state.abilities.push(ability(permanent, ConditionPrimitive::EachDrawsCard));

    let mut engine = Engine::new(state);
    let status = pass_both(&mut engine, p0, p1);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert_eq!(engine.state().stack.len(), 1, "EachDrawsCard fires even though its controller isn't the drawing player");

    let status = pass_both(&mut engine, p0, p1);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert!(engine.state().stack.is_empty());
    assert_eq!(engine.state().player(p1).unwrap().life, 21, "GainLife(1) applied to the ability's own controller, p1");
}

// ---------------------------------------------------------------------
// BeginningOfYourCombat / BeginningOfEachCombat
// ---------------------------------------------------------------------

#[test]
fn beginning_of_your_combat_trigger_fires_and_resolves_for_its_controller() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let permanent = ObjectId(0);
    let mut state = state_at_upkeep_turn_2();
    state.phase = Phase::Main;
    state.step = Step::PreCombatMain;
    state.objects.insert(permanent, card_object(permanent, creature_definition("Bear"), p0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(permanent);
    state.abilities.push(ability(permanent, ConditionPrimitive::BeginningOfYourCombat));

    let mut engine = Engine::new(state);
    let status = pass_both(&mut engine, p0, p1);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert_eq!(engine.state().phase, Phase::Combat);
    assert_eq!(engine.state().step, Step::BeginningOfCombat);
    assert_eq!(engine.state().stack.len(), 1, "the beginning-of-combat trigger was collected and pushed");

    let status = pass_both(&mut engine, p0, p1);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert!(engine.state().stack.is_empty());
    assert_eq!(engine.state().player(p0).unwrap().life, 21, "GainLife(1) applied at p0's own beginning of combat");
}

#[test]
fn beginning_of_your_combat_trigger_does_not_fire_for_a_different_controller() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let permanent = ObjectId(0);
    let mut state = state_at_upkeep_turn_2();
    state.phase = Phase::Main;
    state.step = Step::PreCombatMain;
    state.objects.insert(permanent, card_object(permanent, creature_definition("Bear"), p1, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(permanent);
    state.abilities.push(ability(permanent, ConditionPrimitive::BeginningOfYourCombat));

    let mut engine = Engine::new(state);
    let status = pass_both(&mut engine, p0, p1);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert_eq!(engine.state().step, Step::BeginningOfCombat);
    assert!(engine.state().stack.is_empty(), "p1's ability shouldn't fire on p0's own combat");
}

#[test]
fn beginning_of_each_combat_trigger_fires_regardless_of_whose_turn_it_is() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let permanent = ObjectId(0);
    let mut state = state_at_upkeep_turn_2();
    state.phase = Phase::Main;
    state.step = Step::PreCombatMain;
    state.objects.insert(permanent, card_object(permanent, creature_definition("Bear"), p1, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(permanent);
    state.abilities.push(ability(permanent, ConditionPrimitive::BeginningOfEachCombat));

    let mut engine = Engine::new(state);
    let status = pass_both(&mut engine, p0, p1);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert_eq!(engine.state().stack.len(), 1, "EachCombat fires even though its controller isn't the active player");

    let status = pass_both(&mut engine, p0, p1);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert!(engine.state().stack.is_empty());
    assert_eq!(engine.state().player(p1).unwrap().life, 21, "GainLife(1) applied to the ability's own controller, p1");
}

// ---------------------------------------------------------------------
// BeginningOfYourEndStep / BeginningOfEachEndStep
// ---------------------------------------------------------------------

#[test]
fn beginning_of_your_end_step_trigger_fires_and_resolves_for_its_controller() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let permanent = ObjectId(0);
    let mut state = state_at_upkeep_turn_2();
    state.phase = Phase::Main;
    state.step = Step::PostCombatMain;
    state.objects.insert(permanent, card_object(permanent, creature_definition("Bear"), p0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(permanent);
    state.abilities.push(ability(permanent, ConditionPrimitive::BeginningOfYourEndStep));

    let mut engine = Engine::new(state);
    let status = pass_both(&mut engine, p0, p1);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert_eq!(engine.state().phase, Phase::Ending);
    assert_eq!(engine.state().step, Step::End);
    assert_eq!(engine.state().stack.len(), 1, "the beginning-of-end-step trigger was collected and pushed");

    let status = pass_both(&mut engine, p0, p1);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert!(engine.state().stack.is_empty());
    assert_eq!(engine.state().player(p0).unwrap().life, 21, "GainLife(1) applied at p0's own end step");
}

#[test]
fn beginning_of_your_end_step_trigger_does_not_fire_for_a_different_controller() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let permanent = ObjectId(0);
    let mut state = state_at_upkeep_turn_2();
    state.phase = Phase::Main;
    state.step = Step::PostCombatMain;
    state.objects.insert(permanent, card_object(permanent, creature_definition("Bear"), p1, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(permanent);
    state.abilities.push(ability(permanent, ConditionPrimitive::BeginningOfYourEndStep));

    let mut engine = Engine::new(state);
    let status = pass_both(&mut engine, p0, p1);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert_eq!(engine.state().step, Step::End);
    assert!(engine.state().stack.is_empty(), "p1's ability shouldn't fire on p0's own end step");
}

#[test]
fn beginning_of_each_end_step_trigger_fires_regardless_of_whose_turn_it_is() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let permanent = ObjectId(0);
    let mut state = state_at_upkeep_turn_2();
    state.phase = Phase::Main;
    state.step = Step::PostCombatMain;
    state.objects.insert(permanent, card_object(permanent, creature_definition("Bear"), p1, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(permanent);
    state.abilities.push(ability(permanent, ConditionPrimitive::BeginningOfEachEndStep));

    let mut engine = Engine::new(state);
    let status = pass_both(&mut engine, p0, p1);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert_eq!(engine.state().stack.len(), 1, "EachEndStep fires even though its controller isn't the active player");

    let status = pass_both(&mut engine, p0, p1);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert!(engine.state().stack.is_empty());
    assert_eq!(engine.state().player(p1).unwrap().life, 21, "GainLife(1) applied to the ability's own controller, p1");
}

