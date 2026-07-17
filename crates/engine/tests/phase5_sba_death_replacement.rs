//! Phase 5 vertical slice: backlog item 5b — a dying creature's zone change
//! (CR 704.5f/g) now runs through the real replacement pipeline (CR 614-616)
//! *before* it actually moves, so "if this would die, exile it instead"
//! (CR 614.13) is real, including a full CR 616.1 ordering-choice pause when
//! two such effects match the same death simultaneously (`sba::
//! resolve_deaths`/`ProcessState::AwaitingReplacementChoice`'s `SbaDeath`
//! resume target). See docs/implementation.md's
//! phase5-fix-replacement-bypasses slice notes for the bug this closes.

use std::collections::{HashMap, VecDeque};

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::sba::{check_state_based_actions, SbaOutcome};
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;

fn creature_definition(toughness: i32) -> CardDefinition {
    CardDefinition {
        definition_id: 1,
        name: "Test Creature".to_string(),
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
        script_ref: ScriptReference::ByKey("test_creature".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        effect: None,
        modes: Vec::new(),
    }
}

fn creature(id: ObjectId, owner: PlayerId, toughness: i32, damage_marked: u32) -> GameObject {
    GameObject {
        object_id: id,
        lineage: ObjectLineage { previous: None },
        definition: creature_definition(toughness),
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
        is_token: false,
        is_commander: false,
        visibility: VisibilityPolicy { public: true, visible_to: Vec::new() },
    }
}

fn would_die_exile_effect(source: ObjectId, consumption: ConsumptionPolicy) -> ReplacementEffect {
    ReplacementEffect {
        source,
        spec: ReplacementSpec::Primitive {
            filter: EventFilter::WouldDie {
                affected: EffectFilter { card_types: Some(CardTypeSet::CREATURE), controller: None, self_only: false },
            },
            payload: ReplacementPayload::ReplaceZoneDestination(ZoneKind::Exile),
        },
        consumption,
    }
}

/// Distinct `source` from `would_die_exile_effect`'s own shield - lets a
/// test tell the two apart by which candidate id it chooses, the same way
/// two different real permanents could each grant this style of effect.
fn would_die_command_zone_effect(source: ObjectId) -> ReplacementEffect {
    ReplacementEffect {
        source,
        spec: ReplacementSpec::Primitive {
            filter: EventFilter::WouldDie {
                affected: EffectFilter { card_types: Some(CardTypeSet::CREATURE), controller: None, self_only: false },
            },
            payload: ReplacementPayload::ReplaceZoneDestination(ZoneKind::Command),
        },
        consumption: ConsumptionPolicy::Repeatable,
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
            Zone { kind: ZoneKind::Exile, owner: Some(p0), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Command, owner: Some(p0), contents: VecDeque::new() },
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

/// A non-creature permanent standing in for "whatever grants this
/// replacement effect" - distinct from the dying creature itself, so a test
/// with two such effects can tell them apart by `ReplacementEffect::source`.
fn shield(id: ObjectId, controller: PlayerId) -> GameObject {
    GameObject {
        object_id: id,
        lineage: ObjectLineage { previous: None },
        definition: CardDefinition {
            definition_id: 2,
            name: "Test Shield".to_string(),
            mana_cost: ManaCost { symbols: Vec::new() },
            color_indicator: None,
            supertypes: SupertypeSet::empty(),
            card_types: CardTypeSet::ARTIFACT,
            subtypes: Vec::new(),
            oracle_text: String::new(),
            power: None,
            toughness: None,
            loyalty: None,
            defense: None,
            script_ref: ScriptReference::ByKey("test_shield".to_string()),
            keywords: KeywordSet::empty(),
            target_requirement: None,
            alternative_costs: Vec::new(),
            effect: None,
            modes: Vec::new(),
        },
        owner: controller,
        controller,
        zone: ZoneKind::Battlefield,
        tapped: false,
        flipped: false,
        face_down: false,
        phased_out: false,
        damage_marked: 0,
        deathtouched: false,
        counters: CounterSet::new(),
        attachments: Vec::new(),
        attached_to: None,
        timestamp: SequenceNo(0),
        controlled_since_turn: 0,
        is_token: false,
        is_commander: false,
        visibility: VisibilityPolicy { public: true, visible_to: Vec::new() },
    }
}

// ---------------------------------------------------------------------
// Unit-level: sba::check_state_based_actions / sba::resolve_deaths
// ---------------------------------------------------------------------

#[test]
fn would_die_replacement_exiles_instead_of_dying_to_the_graveyard() {
    let p0 = PlayerId(0);
    let bear = ObjectId(0);
    let mut state = base_state();
    put_on_battlefield(&mut state, creature(bear, p0, 2, 2));
    state.replacement_effects.push(would_die_exile_effect(bear, ConsumptionPolicy::Repeatable));

    let outcome = check_state_based_actions(&mut state, &ScriptRuntime::new());
    let SbaOutcome::Progressed(events, _) = outcome else {
        panic!("a single matching replacement must resolve synchronously, not pause");
    };
    assert_eq!(events.len(), 1);
    let GameEventKind::ZoneChange { object: new_id, from, to, .. } = events[0].kind else {
        panic!("expected a ZoneChange event");
    };
    assert_eq!(from, ZoneKind::Battlefield);
    assert_eq!(to, ZoneKind::Exile, "the ReplaceZoneDestination(Exile) effect must have fired");

    assert!(state.object(bear).is_none(), "the old id is retired");
    let moved = state.object(new_id).expect("the creature now exists under its new id");
    assert_eq!(moved.zone, ZoneKind::Exile);
    assert!(
        state.zone(ZoneKind::Graveyard, Some(p0)).unwrap().contents.is_empty(),
        "it must never have touched the graveyard at all"
    );
    assert!(state.zone(ZoneKind::Exile, Some(p0)).unwrap().contents.contains(&new_id));
}

#[test]
fn two_would_die_replacements_on_the_same_creature_pause_for_an_ordering_choice() {
    let bear = ObjectId(0);
    let mut state = base_state();
    put_on_battlefield(&mut state, creature(bear, PlayerId(0), 2, 2));
    put_on_battlefield(&mut state, shield(ObjectId(1), PlayerId(0)));
    put_on_battlefield(&mut state, shield(ObjectId(2), PlayerId(0)));
    state.replacement_effects.push(would_die_exile_effect(ObjectId(1), ConsumptionPolicy::Repeatable));
    state.replacement_effects.push(would_die_command_zone_effect(ObjectId(2)));

    let outcome = check_state_based_actions(&mut state, &ScriptRuntime::new());
    let SbaOutcome::NeedsOrderingChoice { candidates, pending_deaths, resolved_so_far, affected_player, .. } =
        outcome
    else {
        panic!("two matching replacements on the same event must pause, not guess an order");
    };
    assert_eq!(candidates.len(), 2);
    assert_eq!(affected_player, PlayerId(0), "the dying creature's controller is the affected player");
    assert!(pending_deaths.is_empty(), "no other dying creature was queued behind this one");
    assert!(resolved_so_far.is_empty(), "nothing else resolved before hitting this pause");
    // Nothing applied yet - the creature is still exactly where it was.
    assert!(state.object(bear).is_some());
    assert_eq!(state.object(bear).unwrap().zone, ZoneKind::Battlefield);
}

// ---------------------------------------------------------------------
// End-to-end: real Engine pipeline - PassPriority naturally cascades into
// SbaLoop, pauses on AwaitingReplacementChoice, and submit_replacement_choice
// resumes it correctly.
// ---------------------------------------------------------------------

fn drain(engine: &mut Engine) -> StepStatus {
    loop {
        match engine.step().status {
            StepStatus::Progressed => continue,
            other => return other,
        }
    }
}

#[test]
fn engine_pauses_mid_sba_and_resumes_correctly_after_a_replacement_choice() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let bear = ObjectId(0);
    let mut state = base_state();
    put_on_battlefield(&mut state, creature(bear, p0, 2, 2));
    put_on_battlefield(&mut state, shield(ObjectId(1), p0));
    put_on_battlefield(&mut state, shield(ObjectId(2), p0));
    state.replacement_effects.push(would_die_exile_effect(ObjectId(1), ConsumptionPolicy::Repeatable));
    state.replacement_effects.push(would_die_command_zone_effect(ObjectId(2)));

    let mut engine = Engine::new(state);
    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).unwrap();
    let status = drain(&mut engine);
    let StepStatus::AwaitingReplacementChoice { player, candidates } = status else {
        panic!("expected the SBA pass to pause on the CR 616.1 ordering choice, got {status:?}");
    };
    assert_eq!(player, p0);
    assert_eq!(candidates.len(), 2);

    // Choose the Command-zone effect (ObjectId(2)) specifically, to
    // distinguish it from the Exile one (ObjectId(1)) in the assertions
    // below.
    let command_zone_source =
        candidates.into_iter().find(|&c| c == ObjectId(2)).expect("ObjectId(2) is one of the two candidates");
    engine.submit_replacement_choice(command_zone_source).expect("a real candidate must be accepted");

    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p1), "SBA settles, priority reopens for p1: {status:?}");

    assert!(engine.state().object(bear).is_none(), "the old id is retired");
    let new_id = *engine.state().zone(ZoneKind::Command, Some(p0)).unwrap().contents.front().unwrap();
    assert_eq!(engine.state().object(new_id).unwrap().zone, ZoneKind::Command);
    assert!(
        engine.state().zone(ZoneKind::Exile, Some(p0)).unwrap().contents.is_empty(),
        "the Exile effect must not have also applied - only the chosen one wins"
    );

    // Fixed point genuinely reached - exactly one ZoneChange for this death,
    // not re-processed on a later SbaLoop re-entry.
    let zone_changes =
        engine.state().ledger.iter().filter(|e| matches!(e.kind, GameEventKind::ZoneChange { .. })).count();
    assert_eq!(zone_changes, 1);
}
