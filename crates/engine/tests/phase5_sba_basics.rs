//! Phase 5 vertical slice: baseline CR 704 state-based actions, limited to
//! 704.5a (player at 0 or less life loses), 704.5f (creature toughness 0 or
//! less dies), and 704.5g (creature with lethal marked damage dies). Every
//! other CR 704.5 SBA stays the pre-existing no-op stub - see `sba.rs`'s own
//! TODO for the full list and why.
//!
//! No combat damage step (CR 510) yet - `damage_marked` is pre-seeded
//! directly on fixtures here, the same "fixture stands in for a producer
//! that doesn't exist yet" pattern `mana_pool`/`controlled_since_turn` used
//! before their real producers landed.

use std::collections::{HashMap, VecDeque};

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::sba::{check_state_based_actions, SbaOutcome};
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;

/// This slice's fixtures never register a `ReplacementEffect` on a dying
/// creature - a real CR 616.1 pause here would mean the fixture itself is
/// wrong, not something to handle gracefully.
fn progressed(outcome: SbaOutcome) -> Vec<GameEvent> {
    match outcome {
        SbaOutcome::Progressed(events) => events,
        SbaOutcome::NeedsOrderingChoice { .. } => {
            panic!("fixture didn't expect a replacement ordering-choice pause")
        }
    }
}

fn creature_definition(power: i32, toughness: i32) -> CardDefinition {
    CardDefinition {
        definition_id: 1,
        name: "Test Creature".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::CREATURE,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: Some(PTValue::Fixed(power)),
        toughness: Some(PTValue::Fixed(toughness)),
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_creature".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
    }
}

fn creature(id: ObjectId, owner: PlayerId, toughness: i32, damage_marked: u32) -> GameObject {
    GameObject {
        object_id: id,
        lineage: ObjectLineage { previous: None },
        definition: creature_definition(2, toughness),
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
        visibility: VisibilityPolicy { public: true, visible_to: Vec::new() },
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
        mulligans_taken: 0,
        hand_kept: false,
    }
}

/// One player, an empty battlefield + that player's graveyard already
/// registered (SBA-driven creature death needs a graveyard zone to land in).
fn base_state() -> GameState {
    let p0 = PlayerId(0);
    GameState {
        players: vec![player_state(p0, 20)],
        turn_order: vec![p0],
        active_player: p0,
        turn_number: 1,
        phase: Phase::Combat,
        step: Step::CombatDamage,
        priority_player: p0,
        consecutive_passes: 0,
        lands_played_this_turn: HashMap::new(),
        combat: None,
        last_active_turn_number: HashMap::from([(p0, 1)]),
        objects: HashMap::new(),
        zones: vec![
            Zone { kind: ZoneKind::Battlefield, owner: None, contents: VecDeque::new() },
            Zone { kind: ZoneKind::Graveyard, owner: Some(p0), contents: VecDeque::new() },
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
    }
}

fn put_on_battlefield(state: &mut GameState, object: GameObject) {
    let id = object.object_id;
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(id);
    state.objects.insert(id, object);
}

#[test]
fn lethal_marked_damage_destroys_creature() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let bear = ObjectId(0);
    put_on_battlefield(&mut state, creature(bear, p0, 2, 2));

    let events = progressed(check_state_based_actions(&mut state, &ScriptRuntime::new()));

    assert!(state.object(bear).is_none(), "old id should no longer resolve");
    assert!(!state.zone(ZoneKind::Battlefield, None).unwrap().contents.contains(&bear));
    let gy = state.zone(ZoneKind::Graveyard, Some(p0)).unwrap();
    assert_eq!(gy.contents.len(), 1);
    let new_id = gy.contents[0];
    assert_eq!(state.object(new_id).unwrap().zone, ZoneKind::Graveyard);

    assert_eq!(events.len(), 1);
    match &events[0].kind {
        GameEventKind::ZoneChange { object, from, to, last_known_info } => {
            assert_eq!(*object, new_id);
            assert_eq!(*from, ZoneKind::Battlefield);
            assert_eq!(*to, ZoneKind::Graveyard);
            assert_eq!(last_known_info.as_ref().unwrap().object_id, bear);
        }
        other => panic!("expected ZoneChange, got {other:?}"),
    }
}

#[test]
fn zero_toughness_creature_dies_without_any_damage_marked() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let weenie = ObjectId(0);
    put_on_battlefield(&mut state, creature(weenie, p0, 0, 0));

    let events = progressed(check_state_based_actions(&mut state, &ScriptRuntime::new()));

    assert!(state.object(weenie).is_none());
    assert_eq!(events.len(), 1);
}

#[test]
fn sub_lethal_damage_creature_survives() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let bear = ObjectId(0);
    put_on_battlefield(&mut state, creature(bear, p0, 3, 2));

    let events = progressed(check_state_based_actions(&mut state, &ScriptRuntime::new()));

    assert!(state.object(bear).is_some());
    assert!(events.is_empty());
}

#[test]
fn two_lethally_damaged_creatures_die_in_the_same_pass() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let a = ObjectId(0);
    let b = ObjectId(1);
    put_on_battlefield(&mut state, creature(a, p0, 2, 2));
    put_on_battlefield(&mut state, creature(b, p0, 1, 5));

    let events = progressed(check_state_based_actions(&mut state, &ScriptRuntime::new()));

    assert!(state.object(a).is_none());
    assert!(state.object(b).is_none());
    assert_eq!(events.len(), 2);
    assert_eq!(state.zone(ZoneKind::Graveyard, Some(p0)).unwrap().contents.len(), 2);
}

#[test]
fn creature_with_characteristic_defining_toughness_is_skipped() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let star = ObjectId(0);
    let mut object = creature(star, p0, 0, 0);
    object.definition.toughness =
        Some(PTValue::CharacteristicDefining { display: "*".to_string(), script: None });
    put_on_battlefield(&mut state, object);

    let events = progressed(check_state_based_actions(&mut state, &ScriptRuntime::new()));

    // Can't resolve a CDA without the script bridge - must not guess and
    // must not panic.
    assert!(state.object(star).is_some());
    assert!(events.is_empty());
}

#[test]
fn player_at_zero_life_is_eliminated() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    state.player_mut(p0).unwrap().life = 0;

    let events = progressed(check_state_based_actions(&mut state, &ScriptRuntime::new()));

    assert!(state.player(p0).unwrap().eliminated);
    assert_eq!(events.len(), 1);
    assert!(matches!(events[0].kind, GameEventKind::PlayerEliminated { player } if player == p0));
}

#[test]
fn player_above_zero_life_is_untouched() {
    let mut state = base_state();
    let p0 = PlayerId(0);

    let events = progressed(check_state_based_actions(&mut state, &ScriptRuntime::new()));

    assert!(!state.player(p0).unwrap().eliminated);
    assert!(events.is_empty());
}

#[test]
fn already_eliminated_player_does_not_re_emit_on_next_pass() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    state.player_mut(p0).unwrap().life = -5;

    let first = progressed(check_state_based_actions(&mut state, &ScriptRuntime::new()));
    assert_eq!(first.len(), 1);

    let second = progressed(check_state_based_actions(&mut state, &ScriptRuntime::new()));
    assert!(second.is_empty(), "fixed-point loop must terminate, not re-eliminate every pass");
}

#[test]
fn nothing_applies_returns_empty() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let bear = ObjectId(0);
    put_on_battlefield(&mut state, creature(bear, p0, 2, 0));

    let events = progressed(check_state_based_actions(&mut state, &ScriptRuntime::new()));

    assert!(events.is_empty());
    assert!(state.object(bear).is_some());
}

/// End-to-end: confirms `SbaLoop` is actually wired to this function inside
/// `Engine::step`, not just correct in isolation. A lethally-damaged
/// creature is pre-seeded on the battlefield; a single `PassPriority` (which
/// routes through the full PendingAction -> ... -> SbaLoop -> CollectTriggers
/// pipeline for any non-all-pass action) should be enough to kill it.
#[test]
fn lethal_damage_dies_through_the_real_engine_pipeline() {
    let mut state = base_state();
    let p0 = PlayerId(0);
    let bear = ObjectId(0);
    put_on_battlefield(&mut state, creature(bear, p0, 2, 2));
    state.turn_order = vec![p0, PlayerId(1)];
    state.players.push(player_state(PlayerId(1), 20));

    let mut engine = Engine::new(state);
    engine
        .submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority })
        .expect("p0 holds priority");
    let status = loop {
        match engine.step().status {
            StepStatus::Progressed => continue,
            other => break other,
        }
    };
    assert!(matches!(status, StepStatus::AwaitingAction { .. }));

    assert!(engine.state().object(bear).is_none());
    assert_eq!(engine.state().zone(ZoneKind::Graveyard, Some(p0)).unwrap().contents.len(), 1);
    assert!(
        engine
            .state()
            .ledger
            .iter()
            .any(|e| matches!(e.kind, GameEventKind::ZoneChange { to: ZoneKind::Graveyard, .. }))
    );
}
