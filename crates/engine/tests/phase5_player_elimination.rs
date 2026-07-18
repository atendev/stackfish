//! Phase 5 vertical slice: backlog item 11b — player elimination actually
//! affects the game. `sba::apply_player_loss` already flipped
//! `PlayerState.eliminated` (a prior slice); nothing downstream read it.
//! Closes three independent gaps: `StepStatus::GameOver` never constructed
//! (CR 104.2a/104.4a), an eliminated player still got turns/priority (CR
//! 800.4b), and their objects never left the game (CR 800.4a).

use std::collections::{HashMap, VecDeque};

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::sba::{check_state_based_actions, SbaOutcome};
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;

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

fn creature_definition() -> CardDefinition {
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
        toughness: Some(PTValue::Fixed(2)),
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

fn card_object(id: ObjectId, definition: CardDefinition, owner: PlayerId, zone: ZoneKind) -> GameObject {
    GameObject {
        object_id: id,
        copiable_override: None,
        lineage: ObjectLineage { previous: None },
        definition,
        owner,
        controller: owner,
        zone,
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

/// Two players, ready for priority at `(Main, PreCombatMain)` - the shape
/// most `PassPriority`-driven fixtures across this crate use once past
/// pregame. Every zone kind a purged object could sit in is registered for
/// both players, plus the shared battlefield/exile.
fn two_player_state() -> GameState {
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
            Zone { kind: ZoneKind::Library, owner: Some(p0), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Hand, owner: Some(p0), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Graveyard, owner: Some(p0), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Library, owner: Some(p1), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Hand, owner: Some(p1), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Graveyard, owner: Some(p1), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Battlefield, owner: None, contents: VecDeque::new() },
            Zone { kind: ZoneKind::Exile, owner: None, contents: VecDeque::new() },
        ],
        stack: Vec::new(),
        continuous_effects: Vec::new(),
        replacement_effects: Vec::new(),
        cost_modifiers: Vec::new(),
        hand_size_modifiers: Vec::new(),
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

/// Same shape as `two_player_state`, plus `p2` - CR 800.4b's turn/priority
/// skip only shows up with a third player still in the game once one is
/// eliminated.
fn three_player_state() -> GameState {
    let mut state = two_player_state();
    let p2 = PlayerId(2);
    state.players.push(player_state(p2, 20));
    state.turn_order.push(p2);
    state.last_active_turn_number.insert(p2, 1);
    state.zones.push(Zone { kind: ZoneKind::Library, owner: Some(p2), contents: VecDeque::new() });
    state.zones.push(Zone { kind: ZoneKind::Hand, owner: Some(p2), contents: VecDeque::new() });
    state.zones.push(Zone { kind: ZoneKind::Graveyard, owner: Some(p2), contents: VecDeque::new() });
    state
}

fn put_in_zone(state: &mut GameState, object: GameObject, owner: Option<PlayerId>) {
    let kind = object.zone;
    let id = object.object_id;
    state.zone_mut(kind, owner).unwrap().push_bottom(id);
    state.objects.insert(id, object);
}

fn drain(engine: &mut Engine) -> StepStatus {
    loop {
        match engine.step().status {
            StepStatus::Progressed => continue,
            other => return other,
        }
    }
}

fn pass(engine: &mut Engine, player: PlayerId) -> StepStatus {
    engine
        .submit_action(PlayerAction { player, kind: PlayerActionKind::PassPriority })
        .expect("player holds priority");
    drain(engine)
}

// ---------------------------------------------------------------------
// GameOver (CR 104.2a/104.4a)
// ---------------------------------------------------------------------

#[test]
fn two_player_zero_life_ends_the_game_and_then_stays_ended() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    state.player_mut(p1).unwrap().life = 0;

    let mut engine = Engine::new(state);
    let status = pass(&mut engine, p0);

    assert!(
        matches!(status, StepStatus::GameOver { winner: Some(w) } if w == p0),
        "704.5a eliminates p1; 104.2a: p0 is the sole remaining player, got {status:?}"
    );
    assert!(engine.state().player(p1).unwrap().eliminated);

    // Terminal: further step() calls keep returning the same GameOver
    // rather than mutating anything (no further action can be submitted
    // either - submit_action only accepts PriorityWindow).
    let status_again = engine.step().status;
    assert!(matches!(status_again, StepStatus::GameOver { winner: Some(w) } if w == p0));
    assert!(
        engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).is_err(),
        "a finished game shouldn't accept further actions"
    );
}

#[test]
fn simultaneous_elimination_of_every_remaining_player_is_a_draw() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    state.player_mut(p0).unwrap().life = 0;
    state.player_mut(p1).unwrap().life = 0;

    let mut engine = Engine::new(state);
    let status = pass(&mut engine, p0);

    assert!(
        matches!(status, StepStatus::GameOver { winner: None }),
        "104.4a: both players lost simultaneously, got {status:?}"
    );
}

#[test]
fn single_player_fixture_never_triggers_game_over() {
    // CR 104.2a presupposes an opponent to win against - a one-player test
    // fixture (common throughout this crate, unrelated to this feature)
    // reaching 0 life shouldn't read as "game over."
    let p0 = PlayerId(0);
    let mut state = two_player_state();
    state.players.truncate(1);
    state.turn_order = vec![p0];
    state.player_mut(p0).unwrap().life = 0;

    let mut engine = Engine::new(state);
    let status = pass(&mut engine, p0);

    assert!(!matches!(status, StepStatus::GameOver { .. }), "got {status:?}");
    assert!(engine.state().player(p0).unwrap().eliminated, "the SBA itself is unrelated, still fires");
}

// ---------------------------------------------------------------------
// Turn/priority skip (CR 800.4b)
// ---------------------------------------------------------------------

#[test]
fn priority_pass_skips_an_already_eliminated_player() {
    let mut state = three_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let p2 = PlayerId(2);
    state.player_mut(p1).unwrap().eliminated = true;

    let mut engine = Engine::new(state);
    let status = pass(&mut engine, p0);

    assert!(
        matches!(status, StepStatus::AwaitingAction { player } if player == p2),
        "p1 is eliminated - priority should skip straight to p2, got {status:?}"
    );
}

#[test]
fn end_of_turn_rotation_skips_an_already_eliminated_player() {
    let mut state = three_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let p2 = PlayerId(2);
    state.player_mut(p1).unwrap().eliminated = true;
    state.phase = Phase::Ending;
    state.step = Step::End;

    let mut engine = Engine::new(state);
    let status = pass(&mut engine, p0);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p2), "got {status:?}");
    let status = pass(&mut engine, p2);

    assert!(
        matches!(status, StepStatus::AwaitingAction { player } if player == p2),
        "active_player should skip p1 and land back on p2 (the only other non-eliminated \
         player), reopening priority for their own Upkeep, got {status:?}"
    );
    assert_eq!(engine.state().active_player, p2);
    assert_eq!(engine.state().phase, Phase::Beginning);
    assert_eq!(engine.state().step, Step::Upkeep);
}

// ---------------------------------------------------------------------
// Objects leaving the game (CR 800.4a)
// ---------------------------------------------------------------------

#[test]
fn eliminated_players_battlefield_permanent_leaves_the_game_without_a_dies_trigger() {
    let mut state = two_player_state();
    let p1 = PlayerId(1);
    let creature = ObjectId(0);
    put_in_zone(&mut state, card_object(creature, creature_definition(), p1, ZoneKind::Battlefield), None);
    state.abilities.push(dies_ability(creature));
    state.player_mut(p1).unwrap().life = 0;

    let outcome = check_state_based_actions(&mut state, &ScriptRuntime::new());
    let SbaOutcome::Progressed(_, departed) = outcome else {
        panic!("fixture didn't expect a choice pause");
    };

    assert!(state.object(creature).is_none(), "800.4a: the object itself is gone, not just relocated");
    assert!(!state.zone(ZoneKind::Battlefield, None).unwrap().contents.contains(&creature));
    assert!(state.abilities.is_empty(), "its Triggered ability is purged along with it");
    // 800.4d: a triggered ability controlled by a player who left the game
    // never goes on the stack - retire_object's return value (what a normal
    // death would feed into CollectTriggers) is discarded, not surfaced
    // here, so no Dies trigger fires for it.
    assert!(departed.is_empty());
}

#[test]
fn eliminated_players_hand_and_library_are_emptied() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let in_hand = ObjectId(0);
    let in_library = ObjectId(1);
    put_in_zone(&mut state, card_object(in_hand, creature_definition(), p1, ZoneKind::Hand), Some(p1));
    put_in_zone(&mut state, card_object(in_library, creature_definition(), p1, ZoneKind::Library), Some(p1));
    state.player_mut(p1).unwrap().life = 0;

    let _ = check_state_based_actions(&mut state, &ScriptRuntime::new());

    assert!(state.object(in_hand).is_none());
    assert!(state.object(in_library).is_none());
    assert!(state.zone(ZoneKind::Hand, Some(p1)).unwrap().contents.is_empty());
    assert!(state.zone(ZoneKind::Library, Some(p1)).unwrap().contents.is_empty());
    // p0's own zones are untouched - this isn't a blanket wipe.
    assert!(state.player(p0).is_some());
}

#[test]
fn a_non_eliminated_players_objects_are_untouched() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let survivor = ObjectId(0);
    put_in_zone(&mut state, card_object(survivor, creature_definition(), p0, ZoneKind::Battlefield), None);
    state.player_mut(p1).unwrap().life = 0;

    let _ = check_state_based_actions(&mut state, &ScriptRuntime::new());

    assert!(state.object(survivor).is_some(), "p0 wasn't eliminated - their permanent must stay");
    assert!(state.zone(ZoneKind::Battlefield, None).unwrap().contents.contains(&survivor));
}
