//! Phase 5 vertical slice: CR 111.7/704.5d token cessation. Pulled from the
//! "Known unresolved gaps" list rather than a numbered backlog item - the
//! numbered backlog (1-17) closed several slices ago, leaving only item 16
//! (planeswalkers/battles/DFCs/split/sagas, an explicit non-goal) unstarted.
//!
//! `sba::collect_ceasing_tokens`/`commit_token_cessation` are the new pieces:
//! every SBA pass now starts by scanning every non-battlefield zone for
//! `GameObject.is_token` and purging any it finds (no new `ObjectId` minted -
//! this is disappearance, not a CR 400.7 zone move). Timing is the whole
//! point of this slice: CR 111.7's own parenthetical says "applicable
//! triggered abilities will trigger before the token ceases to exist" - a
//! token that dies *this* pass isn't "in a zone other than the battlefield"
//! until *after* this pass ends (CR 704.3's "simultaneous, based on the
//! state as of this check"), so the scan running at the very top of
//! `check_state_based_actions` (before this same pass's own
//! `collect_dying_creatures`/`resolve_deaths` commit anything) naturally
//! defers cessation to the *next* pass, for free - not a special case bolted
//! on afterward.

use std::collections::{HashMap, VecDeque};

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::sba::{check_state_based_actions, SbaOutcome};
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;

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
        effect: None,
        modes: Vec::new(),
    }
}

fn creature(id: ObjectId, owner: PlayerId, toughness: i32, damage_marked: u32, is_token: bool) -> GameObject {
    GameObject {
        object_id: id,
        copiable_override: None,
        lineage: ObjectLineage { previous: None },
        definition: creature_definition(if is_token { "Test Token" } else { "Test Creature" }, toughness),
        owner,
        controller: owner,
        zone: ZoneKind::Battlefield,
        tapped: false,
        flipped: false,
        face_down: false,
        phased_out: false,
        damage_marked,
        deathtouched: false,
        counters: CounterSet::new(),
        attachments: Vec::new(),
        attached_to: None,
        timestamp: SequenceNo(0),
        controlled_since_turn: 0,
        is_token,
        is_commander: false,
        visibility: VisibilityPolicy { public: true, visible_to: Vec::new() },
    }
}

fn dies_ability(source: ObjectId) -> RegisteredAbility {
    RegisteredAbility {
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

fn player_state(id: PlayerId, life: i32) -> PlayerState {
    PlayerState {
        player_id: id,
        life,
        mana_pool: ManaPool::default(),
        counters: CounterSet::new(),
        emblems: Vec::new(),
        commander_damage_taken: Vec::new(),
        eliminated: false,
        drew_from_empty_library: false,
        mulligans_taken: 0,
        hand_kept: false,
        commander_casts: std::collections::HashMap::new(),
    }
}

fn base_state() -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        players: vec![player_state(p0, 20), player_state(p1, 20)],
        turn_order: vec![p0, p1],
        active_player: p0,
        turn_number: 1,
        phase: Phase::Main,
        step: Step::PreCombatMain,
        priority_player: p0,
        consecutive_passes: 0,
        lands_played_this_turn: HashMap::new(),
        combat: None,
        last_active_turn_number: HashMap::from([(p0, 1), (p1, 1)]),
        objects: HashMap::new(),
        zones: vec![
            Zone { kind: ZoneKind::Battlefield, owner: None, contents: VecDeque::new() },
            Zone { kind: ZoneKind::Graveyard, owner: Some(p0), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Graveyard, owner: Some(p1), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Exile, owner: Some(p0), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Exile, owner: Some(p1), contents: VecDeque::new() },
        ],
        stack: Vec::new(),
        continuous_effects: Vec::new(),
        replacement_effects: Vec::new(),
        cost_modifiers: Vec::new(),
        abilities: Vec::new(),
        ledger: Vec::new(),
        rng: RngState { seed: 1, draws_consumed: 0 },
        next_event_id: 0,
        next_sequence_no: 0,
        next_object_id: 1000,
        pending_step_skips: Vec::new(),
        once_per_turn_triggered: std::collections::HashSet::new(),
        commander_return_offered: std::collections::HashSet::new(),
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

// ---------------------------------------------------------------------
// Unit-level: sba::check_state_based_actions
// ---------------------------------------------------------------------

#[test]
fn a_token_that_just_died_does_not_cease_until_the_next_sba_pass() {
    let p0 = PlayerId(0);
    let bear = ObjectId(0);
    let mut state = base_state();
    put_on_battlefield(&mut state, creature(bear, p0, 2, 2, true));

    // Pass 1: CR 704.5g kills the token (lethal damage) - a real ZoneChange,
    // new ObjectId minted, now sitting in the graveyard. Not ceased yet.
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

    // Pass 2: now it's "a token in a zone other than the battlefield" as of
    // the top of *this* check - CR 704.5d applies.
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

    // Run the fixed point all the way out - a real (non-token) card must
    // never be swept up by the new CR 704.5d check, regardless of how many
    // more passes run.
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
    // Not graveyard-specific - CR 704.5d is "any zone other than the
    // battlefield." This token was never on the battlefield this pass at
    // all (placed directly in Exile by the fixture), so it's eligible from
    // the very first check, unlike the "just died" case above.
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

// ---------------------------------------------------------------------
// End-to-end: real Engine pipeline - a token's own Dies trigger still fires
// (and resolves) despite the token ceasing a pass later.
// ---------------------------------------------------------------------

fn drain(engine: &mut Engine) -> StepStatus {
    loop {
        match engine.step().status {
            StepStatus::Progressed => continue,
            other => return other,
        }
    }
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

    // By now every SBA pass this pipeline run needed has already settled,
    // including the token's own cessation one pass after its own death.
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
