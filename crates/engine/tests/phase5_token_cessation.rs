//! CR 111.7, 704.5d: `sba::collect_ceasing_tokens` and `commit_token_cessation` purge any `GameObject.is_token` found outside the battlefield, at the start of each SBA pass and with no new `ObjectId`.
//! A token that dies this pass is not outside the battlefield until the pass ends (CR 704.3), so cessation naturally lands on the next pass and its triggers fire first.

use std::collections::HashMap;

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::sba::{check_state_based_actions, SbaOutcome};
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, zones, two_player_state as base_two_player_state};

fn creature_definition(name: &str, toughness: i32) -> CardDefinition {
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

fn creature(id: ObjectId, owner: PlayerId, toughness: i32, damage_marked: u32, is_token: bool) -> GameObject {
    let definition = creature_definition(if is_token { "Test Token" } else { "Test Creature" }, toughness);
    GameObject { damage_marked, is_token, ..card_object(id, definition, owner, ZoneKind::Battlefield) }
}

fn dies_ability(source: ObjectId) -> RegisteredAbility {
    RegisteredAbility {
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
        id: AbilityId(0),
        source,
        kind: AbilityKind::Triggered,
        condition: Some(ConditionSpec::Primitive(ConditionPrimitive::Dies)),
        cost: None,
        target: None,
        operation: ScriptHandle(0),
        effect: Some(AbilityEffect::GainLife(1)),
        intervening_if: None,
    }
}

fn base_state() -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    // Exile is per-player here, so the two Exile entries are appended by hand.
    let mut zones = zones(p0, p1, &[ZoneKind::Battlefield, ZoneKind::Graveyard]);
    zones.push(Zone::new(ZoneKind::Exile, Some(p0)));
    zones.push(Zone::new(ZoneKind::Exile, Some(p1)));
    GameState {
        step: Step::PreCombatMain,
        last_active_turn_number: HashMap::from([(p0, 1), (p1, 1)]),
        ..base_two_player_state(zones)
    }
}

fn put_on_battlefield(state: &mut GameState, object: GameObject) {
    let id = object.object_id;
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(id);
    state.objects.insert(id, object);
}

fn put_in_exile(state: &mut GameState, object: GameObject) {
    let id = object.object_id;
    let owner = object.owner;
    let mut object = object;
    object.zone = ZoneKind::Exile;
    state.zone_mut(ZoneKind::Exile, Some(owner)).unwrap().push_bottom(id);
    state.objects.insert(id, object);
}

fn progressed(outcome: SbaOutcome) -> Vec<GameEvent> {
    match outcome {
        SbaOutcome::Progressed(events, _) => events,
        SbaOutcome::NeedsOrderingChoice { .. } => panic!("fixture didn't expect a replacement ordering-choice pause"),
        SbaOutcome::NeedsLegendRuleChoice { .. } => panic!("fixture didn't expect a legend-rule choice pause"),
        SbaOutcome::NeedsCommanderReturnChoice { .. } => panic!("fixture didn't expect a commander-return choice pause"),
    }
}

#[test]
fn a_token_that_just_died_does_not_cease_until_the_next_sba_pass() {
    let p0 = PlayerId(0);
    let bear = ObjectId(0);
    let mut state = base_state();
    put_on_battlefield(&mut state, creature(bear, p0, 2, 2, true));

    // Pass 1: CR 704.5g kills the token (a ZoneChange with a new ObjectId into the graveyard), and it has not ceased yet.
    let events = progressed(check_state_based_actions(&mut state, &ScriptRuntime::new()));
    assert_eq!(events.len(), 1);
    let GameEventKind::ZoneChange { object: graveyard_id, to: ZoneKind::Graveyard, .. } = events[0].kind else {
        panic!("expected a plain death, got {:?}", events[0].kind);
    };
    assert!(state.object(graveyard_id).is_some(), "the token's post-death object must still exist after this same pass");
    assert!(
        state.zone(ZoneKind::Graveyard, Some(p0)).unwrap().contents.contains(&graveyard_id),
        "sitting in the graveyard, not yet purged"
    );

    // Pass 2: it is now a token outside the battlefield as of this check, so CR 704.5d applies.
    let events = progressed(check_state_based_actions(&mut state, &ScriptRuntime::new()));
    assert_eq!(events.len(), 1);
    assert!(matches!(events[0].kind, GameEventKind::TokenCeased { object } if object == graveyard_id));
    assert!(state.object(graveyard_id).is_none(), "ceased - purged from state.objects");
    assert!(
        !state.zone(ZoneKind::Graveyard, Some(p0)).unwrap().contents.contains(&graveyard_id),
        "also removed from the graveyard's own contents"
    );
}

#[test]
fn a_non_token_creature_with_the_same_fate_never_ceases() {
    let p0 = PlayerId(0);
    let bear = ObjectId(0);
    let mut state = base_state();
    put_on_battlefield(&mut state, creature(bear, p0, 2, 2, false));

    let events = progressed(check_state_based_actions(&mut state, &ScriptRuntime::new()));
    assert_eq!(events.len(), 1);
    let GameEventKind::ZoneChange { object: graveyard_id, to: ZoneKind::Graveyard, .. } = events[0].kind else {
        panic!("expected a plain death, got {:?}", events[0].kind);
    };

    // Running to a fixed point must never sweep up a real (non-token) card.
    loop {
        let events = progressed(check_state_based_actions(&mut state, &ScriptRuntime::new()));
        if events.is_empty() {
            break;
        }
        panic!("a non-token card should never produce further SBA events here, got {:?}", events);
    }
    assert!(state.object(graveyard_id).is_some(), "still sitting in the graveyard, never ceases");
}

#[test]
fn a_token_already_sitting_in_exile_ceases_on_the_very_first_check() {
    // CR 704.5d covers any zone other than the battlefield. This token was placed in Exile directly, so it is eligible from the first check.
    let p0 = PlayerId(0);
    let relic = ObjectId(0);
    let mut state = base_state();
    put_in_exile(&mut state, creature(relic, p0, 2, 0, true));

    let events = progressed(check_state_based_actions(&mut state, &ScriptRuntime::new()));
    assert_eq!(events.len(), 1);
    assert!(matches!(events[0].kind, GameEventKind::TokenCeased { object } if object == relic));
    assert!(state.object(relic).is_none());
    assert!(state.zone(ZoneKind::Exile, Some(p0)).unwrap().contents.is_empty());
}

fn pass(engine: &mut Engine, player: PlayerId) {
    engine.submit_action(PlayerAction { player, kind: PlayerActionKind::PassPriority }).unwrap();
}

#[test]
fn a_dying_tokens_own_dies_trigger_still_fires_and_resolves_despite_later_ceasing() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let bear = ObjectId(0);
    let mut state = base_state();
    put_on_battlefield(&mut state, creature(bear, p0, 2, 2, true));
    state.abilities.push(dies_ability(bear));

    let mut engine = Engine::new(state);
    pass(&mut engine, p0);
    let status = drain(&mut engine);
    assert!(
        matches!(status, StepStatus::AwaitingAction { player } if player == p1),
        "the Dies trigger must have been collected and pushed to the stack: {status:?}"
    );
    assert_eq!(engine.state().stack.len(), 1, "the Dies trigger was collected and pushed, same as a real card would be");

    pass(&mut engine, p1);
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert!(engine.state().stack.is_empty(), "the triggered ability resolved");
    assert_eq!(engine.state().player(p0).unwrap().life, 21, "GainLife(1) applied - the trigger fired correctly");

    // Every SBA pass this run needed has settled, including the token's cessation one pass after its death.
    assert!(
        engine.state().zone(ZoneKind::Graveyard, Some(p0)).unwrap().contents.is_empty(),
        "the token itself is gone - it ceased to exist, not just died"
    );
}

#[test]
fn a_dying_non_token_creature_stays_in_the_graveyard_through_the_same_pipeline() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let bear = ObjectId(0);
    let mut state = base_state();
    put_on_battlefield(&mut state, creature(bear, p0, 2, 2, false));

    let mut engine = Engine::new(state);
    pass(&mut engine, p0);
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p1));

    assert_eq!(
        engine.state().zone(ZoneKind::Graveyard, Some(p0)).unwrap().contents.len(),
        1,
        "a real (non-token) card stays in the graveyard - CR 704.5d never applies to it"
    );
}
