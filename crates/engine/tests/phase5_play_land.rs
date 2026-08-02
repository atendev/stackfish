//! Phase 5 vertical slice: `PlayLand` (CR 305.1 - lands aren't spells, no
//! stack involved) plus the zone helpers (`Zone::remove`/`push_bottom`,
//! `GameState::object_mut`/`zone_mut`) it's the first real caller of.

use stackfish_engine::commands::CommandError;
use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::*;
use stackfish_testutil::{
    card_object, drain, hand_and_battlefield_zones, minimal_card_definition, two_player_state as base_two_player_state,
};

fn land_definition() -> CardDefinition {
    CardDefinition {
        supertypes: SupertypeSet::BASIC,
        ..minimal_card_definition(1, "Forest", "forest", ManaCost { symbols: Vec::new() }, CardTypeSet::LAND)
    }
}

fn instant_definition() -> CardDefinition {
    minimal_card_definition(2, "Shock", "shock", ManaCost { symbols: Vec::new() }, CardTypeSet::INSTANT)
}

/// Two players, both zone sets present (hand/battlefield) so `PlayLand` has
/// somewhere real to move a card to/from. `base_two_player_state` already
/// defaults to Main phase (sorcery-speed timing, CR 305.1/305.3), so callers
/// don't have to set it per test; the rejection tests below still test their
/// own intended failure (non-Land, not-in-hand), not an incidental timing
/// failure.
fn two_player_state() -> GameState {
    base_two_player_state(hand_and_battlefield_zones(PlayerId(0), PlayerId(1)))
}

/// CR 400.7: `PlayLand` mints a new `ObjectId` for the permanent rather than
/// reusing the hand card's id (see commands.rs). Tests that need to find the
/// moved object afterward can't look it up by the original id - they read it
/// back from the `ZoneChange` event `PlayLand` produces instead.
fn moved_object_id(engine: &Engine) -> ObjectId {
    match engine.state().ledger.last().expect("expected a ledger entry").kind {
        GameEventKind::ZoneChange { object, .. } => object,
        _ => panic!("expected a ZoneChange event"),
    }
}

#[test]
fn play_land_moves_card_hand_to_battlefield() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    state.objects.insert(card, card_object(card, land_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    engine
        .submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PlayLand { card } })
        .expect("p0 holds priority, land is in hand");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));

    let new_id = moved_object_id(&engine);
    assert!(engine.state().object(card).is_none(), "old id retired per CR 400.7");
    assert_eq!(engine.state().object(new_id).unwrap().zone, ZoneKind::Battlefield);
    assert!(!engine.state().zone(ZoneKind::Hand, Some(p0)).unwrap().contents.contains(&card));
    assert!(engine.state().zone(ZoneKind::Battlefield, None).unwrap().contents.contains(&new_id));
}

#[test]
fn play_land_breaks_pass_chain() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    // CR 305.3: a land can only be played on the actor's own turn - p1 is
    // the one playing the land below, so p1 must be the active player. p0
    // still passes first (passing priority has no turn restriction).
    state.active_player = p1;
    let land = ObjectId(0);
    state.objects.insert(land, card_object(land, land_definition(), p1, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p1)).unwrap().push_bottom(land);

    let mut engine = Engine::new(state);
    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(&mut engine);
    assert_eq!(engine.state().consecutive_passes, 1);

    engine
        .submit_action(PlayerAction { player: p1, kind: PlayerActionKind::PlayLand { card: land } })
        .unwrap();
    drain(&mut engine);
    assert_eq!(engine.state().consecutive_passes, 0, "PlayLand must reset the pass chain");
}

#[test]
fn play_land_rejects_non_land_card() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    state.objects.insert(card, card_object(card, instant_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    engine
        .submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PlayLand { card } })
        .unwrap();
    let status = drain(&mut engine);

    assert!(matches!(status, StepStatus::Error(CommandError::IllegalAction(_))));
    assert_eq!(engine.state().object(card).unwrap().zone, ZoneKind::Hand, "state untouched");
    assert!(engine.state().zone(ZoneKind::Hand, Some(p0)).unwrap().contents.contains(&card));
    assert_eq!(engine.state().ledger.len(), 0, "no ledger growth on a rejected command");
}

#[test]
fn play_land_rejects_card_not_in_hand() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    state.objects.insert(card, card_object(card, land_definition(), p0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    engine
        .submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PlayLand { card } })
        .unwrap();
    let status = drain(&mut engine);

    assert!(matches!(status, StepStatus::Error(CommandError::IllegalAction(_))));
    assert_eq!(engine.state().object(card).unwrap().zone, ZoneKind::Battlefield, "state untouched");
    assert!(engine.state().zone(ZoneKind::Battlefield, None).unwrap().contents.contains(&card));
    assert_eq!(engine.state().ledger.len(), 0);
}

#[test]
fn play_land_rejects_outside_main_phase() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    state.phase = Phase::Combat;
    state.objects.insert(card, card_object(card, land_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    engine
        .submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PlayLand { card } })
        .unwrap();
    let status = drain(&mut engine);

    assert!(matches!(status, StepStatus::Error(CommandError::IllegalAction(_))));
    assert_eq!(engine.state().object(card).unwrap().zone, ZoneKind::Hand, "state untouched");
}

#[test]
fn play_land_rejects_nonempty_stack() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    state.stack.push(StackObject {
        stack_id: ObjectId(99),
        kind: StackEntryKind::Spell,
        source: ObjectId(99),
        source_lki: None,
        owner: p0,
        controller: p0,
        chosen_modes: Vec::new(),
        targets: Vec::new(),
        target_requirement: None,
        x_value: None,
        exile_on_leaving_stack: false,
        bought_back: false,
        costs_paid: Vec::new(),
        operation: ScriptHandle(0),
        effect: None,
        intervening_if: None,
        pending_mode_choice: None,
    });
    state.objects.insert(card, card_object(card, land_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    engine
        .submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PlayLand { card } })
        .unwrap();
    let status = drain(&mut engine);

    assert!(matches!(status, StepStatus::Error(CommandError::IllegalAction(_))));
    assert_eq!(engine.state().object(card).unwrap().zone, ZoneKind::Hand, "state untouched");
}

#[test]
fn play_land_rejects_not_actors_turn() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    // It's p1's turn, but p0 (who currently holds priority) tries to play
    // a land they own - CR 305.3.
    state.active_player = p1;
    let card = ObjectId(0);
    state.objects.insert(card, card_object(card, land_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    engine
        .submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PlayLand { card } })
        .unwrap();
    let status = drain(&mut engine);

    assert!(matches!(status, StepStatus::Error(CommandError::IllegalAction(_))));
    assert_eq!(engine.state().object(card).unwrap().zone, ZoneKind::Hand, "state untouched");
}

#[test]
fn play_land_rejects_second_land_same_turn() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let first = ObjectId(0);
    let second = ObjectId(1);
    state.objects.insert(first, card_object(first, land_definition(), p0, ZoneKind::Hand));
    state.objects.insert(second, card_object(second, land_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(first);
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(second);

    let mut engine = Engine::new(state);
    engine
        .submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PlayLand { card: first } })
        .unwrap();
    drain(&mut engine);
    let first_new_id = moved_object_id(&engine);
    assert!(engine.state().object(first).is_none(), "old id retired per CR 400.7");
    assert_eq!(engine.state().object(first_new_id).unwrap().zone, ZoneKind::Battlefield);

    engine
        .submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PlayLand { card: second } })
        .unwrap();
    let status = drain(&mut engine);

    assert!(matches!(status, StepStatus::Error(CommandError::IllegalAction(_))));
    assert_eq!(engine.state().object(second).unwrap().zone, ZoneKind::Hand, "second land untouched");
}

#[test]
fn play_land_event_gets_causal_ids() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    state.objects.insert(card, card_object(card, land_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    engine
        .submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PlayLand { card } })
        .unwrap();
    drain(&mut engine);

    assert_eq!(engine.state().ledger.len(), 1);
    let event = &engine.state().ledger[0];
    assert!(event.caused_by_command.is_some());
    assert!(event.parent_event.is_none());
    assert!(matches!(
        &event.kind,
        GameEventKind::ZoneChange { object, from: ZoneKind::Hand, to: ZoneKind::Battlefield, last_known_info: Some(lki) }
            // CR 400.7: `object` is the new battlefield-side id, not `card` -
            // the old id is only recoverable via the last_known_info snapshot.
            if *object != card && lki.object_id == card
    ));
}
