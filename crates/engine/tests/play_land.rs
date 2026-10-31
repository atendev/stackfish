//! CR 305.1: `PlayLand` moves a land without using the stack, and is the first caller of `Zone::remove`, `push_bottom`, `GameState::object_mut` and `zone_mut`.

use stackfish_engine::commands::Rejected;
use stackfish_engine::engine::ActionError;
use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::*;
use stackfish_testutil::{
    card_object, drain, hand_and_battlefield_zones, minimal_card_definition,
    two_player_state as base_two_player_state,
};

fn land_definition() -> CardDefinition {
    CardDefinition {
        supertypes: SupertypeSet::BASIC,
        ..minimal_card_definition(
            1,
            "Forest",
            "forest",
            ManaCost {
                symbols: Vec::new(),
            },
            CardTypeSet::LAND,
        )
    }
}

fn instant_definition() -> CardDefinition {
    minimal_card_definition(
        2,
        "Shock",
        "shock",
        ManaCost {
            symbols: Vec::new(),
        },
        CardTypeSet::INSTANT,
    )
}

fn two_player_state() -> GameState {
    base_two_player_state(hand_and_battlefield_zones(PlayerId(0), PlayerId(1)))
}

// CR 400.7: PlayLand mints a new ObjectId, so tests read the moved object from the ZoneChange event.
fn moved_object_id(engine: &Engine) -> ObjectId {
    match engine
        .state()
        .ledger
        .last()
        .expect("expected a ledger entry")
        .kind
    {
        GameEventKind::ZoneChange { object, .. } => object,
        _ => panic!("expected a ZoneChange event"),
    }
}

#[test]
fn play_land_moves_card_hand_to_battlefield() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    state.objects.insert(
        card,
        card_object(card, land_definition(), p0, ZoneKind::Hand),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);

    let mut engine = Engine::new(state);
    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::PlayLand { card },
        })
        .expect("p0 holds priority, land is in hand");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));

    let new_id = moved_object_id(&engine);
    assert!(
        engine.state().object(card).is_none(),
        "old id retired per CR 400.7"
    );
    assert_eq!(
        engine.state().object(new_id).unwrap().zone,
        ZoneKind::Battlefield
    );
    assert!(
        !engine
            .state()
            .zone(ZoneKind::Hand, Some(p0))
            .unwrap()
            .contents
            .contains(&card)
    );
    assert!(
        engine
            .state()
            .zone(ZoneKind::Battlefield, None)
            .unwrap()
            .contents
            .contains(&new_id)
    );
}

#[test]
fn play_land_breaks_pass_chain() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    // CR 305.3: a land can be played only on the actor's own turn, so p1 must be the active player. p0 still passes first.
    state.active_player = p1;
    let land = ObjectId(0);
    state.objects.insert(
        land,
        card_object(land, land_definition(), p1, ZoneKind::Hand),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p1))
        .unwrap()
        .push_bottom(land);

    let mut engine = Engine::new(state);
    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::PassPriority,
        })
        .unwrap();
    drain(&mut engine);
    assert_eq!(engine.state().consecutive_passes, 1);

    engine
        .submit_action(PlayerAction {
            player: p1,
            kind: PlayerActionKind::PlayLand { card: land },
        })
        .unwrap();
    drain(&mut engine);
    assert_eq!(
        engine.state().consecutive_passes,
        0,
        "PlayLand must reset the pass chain"
    );
}

#[test]
fn play_land_rejects_non_land_card() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    state.objects.insert(
        card,
        card_object(card, instant_definition(), p0, ZoneKind::Hand),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);

    let mut engine = Engine::new(state);
    let err = engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::PlayLand { card },
        })
        .unwrap_err();

    assert!(matches!(
        err,
        ActionError::Rejected(Rejected::IllegalAction(_))
    ));
    assert_eq!(
        engine.state().object(card).unwrap().zone,
        ZoneKind::Hand,
        "state untouched"
    );
    assert!(
        engine
            .state()
            .zone(ZoneKind::Hand, Some(p0))
            .unwrap()
            .contents
            .contains(&card)
    );
    assert_eq!(
        engine.state().ledger.len(),
        0,
        "no ledger growth on a rejected command"
    );
}

#[test]
fn play_land_rejects_card_not_in_hand() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    state.objects.insert(
        card,
        card_object(card, land_definition(), p0, ZoneKind::Battlefield),
    );
    state
        .zone_mut(ZoneKind::Battlefield, None)
        .unwrap()
        .push_bottom(card);

    let mut engine = Engine::new(state);
    let err = engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::PlayLand { card },
        })
        .unwrap_err();

    assert!(matches!(
        err,
        ActionError::Rejected(Rejected::IllegalAction(_))
    ));
    assert_eq!(
        engine.state().object(card).unwrap().zone,
        ZoneKind::Battlefield,
        "state untouched"
    );
    assert!(
        engine
            .state()
            .zone(ZoneKind::Battlefield, None)
            .unwrap()
            .contents
            .contains(&card)
    );
    assert_eq!(engine.state().ledger.len(), 0);
}

#[test]
fn play_land_rejects_outside_main_phase() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    state.phase = Phase::Combat;
    state.objects.insert(
        card,
        card_object(card, land_definition(), p0, ZoneKind::Hand),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);

    let mut engine = Engine::new(state);
    let err = engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::PlayLand { card },
        })
        .unwrap_err();

    assert!(matches!(
        err,
        ActionError::Rejected(Rejected::IllegalAction(_))
    ));
    assert_eq!(
        engine.state().object(card).unwrap().zone,
        ZoneKind::Hand,
        "state untouched"
    );
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
        escaped: false,
        costs_paid: Vec::new(),
        operation: ScriptHandle(0),
        effect: None,
        intervening_if: None,
        pending_mode_choice: None,
    });
    state.objects.insert(
        card,
        card_object(card, land_definition(), p0, ZoneKind::Hand),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);

    let mut engine = Engine::new(state);
    let err = engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::PlayLand { card },
        })
        .unwrap_err();

    assert!(matches!(
        err,
        ActionError::Rejected(Rejected::IllegalAction(_))
    ));
    assert_eq!(
        engine.state().object(card).unwrap().zone,
        ZoneKind::Hand,
        "state untouched"
    );
}

#[test]
fn play_land_rejects_not_actors_turn() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    // It is p1's turn but p0 holds priority and tries to play a land (CR 305.3).
    state.active_player = p1;
    let card = ObjectId(0);
    state.objects.insert(
        card,
        card_object(card, land_definition(), p0, ZoneKind::Hand),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);

    let mut engine = Engine::new(state);
    let err = engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::PlayLand { card },
        })
        .unwrap_err();

    assert!(matches!(
        err,
        ActionError::Rejected(Rejected::IllegalAction(_))
    ));
    assert_eq!(
        engine.state().object(card).unwrap().zone,
        ZoneKind::Hand,
        "state untouched"
    );
}

#[test]
fn play_land_rejects_second_land_same_turn() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let first = ObjectId(0);
    let second = ObjectId(1);
    state.objects.insert(
        first,
        card_object(first, land_definition(), p0, ZoneKind::Hand),
    );
    state.objects.insert(
        second,
        card_object(second, land_definition(), p0, ZoneKind::Hand),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(first);
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(second);

    let mut engine = Engine::new(state);
    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::PlayLand { card: first },
        })
        .unwrap();
    drain(&mut engine);
    let first_new_id = moved_object_id(&engine);
    assert!(
        engine.state().object(first).is_none(),
        "old id retired per CR 400.7"
    );
    assert_eq!(
        engine.state().object(first_new_id).unwrap().zone,
        ZoneKind::Battlefield
    );

    let err = engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::PlayLand { card: second },
        })
        .unwrap_err();

    assert!(matches!(
        err,
        ActionError::Rejected(Rejected::IllegalAction(_))
    ));
    assert_eq!(
        engine.state().object(second).unwrap().zone,
        ZoneKind::Hand,
        "second land untouched"
    );
}

// CR 305.2
// A LandPlayModifier granting one additional land play lets a second land through and still rejects a third.
#[test]
fn play_land_allows_an_additional_land_with_a_land_play_modifier() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let source = ObjectId(50);
    state.land_play_modifiers.push(LandPlayModifier {
        source,
        controller: p0,
        affected: LandPlayAffected::Filter(FilterController::You),
        additional: 1,
    });
    let first = ObjectId(0);
    let second = ObjectId(1);
    let third = ObjectId(2);
    for id in [first, second, third] {
        state
            .objects
            .insert(id, card_object(id, land_definition(), p0, ZoneKind::Hand));
        state
            .zone_mut(ZoneKind::Hand, Some(p0))
            .unwrap()
            .push_bottom(id);
    }

    let mut engine = Engine::new(state);
    for card in [first, second] {
        engine
            .submit_action(PlayerAction {
                player: p0,
                kind: PlayerActionKind::PlayLand { card },
            })
            .unwrap();
        let status = drain(&mut engine);
        assert!(
            matches!(status, StepStatus::AwaitingAction { .. }),
            "expected {card:?} to be playable, got {status:?}"
        );
    }

    let err = engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::PlayLand { card: third },
        })
        .unwrap_err();
    assert!(
        matches!(err, ActionError::Rejected(Rejected::IllegalAction(_))),
        "a 3rd land should still be rejected"
    );
}

// CR 305.2
// Two +1 modifiers grant 2 extra land plays (3 total) and not the max of the two, unlike effective_draw_limit.
#[test]
fn play_land_two_land_play_modifiers_stack_additively() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    state.land_play_modifiers.push(LandPlayModifier {
        source: ObjectId(50),
        controller: p0,
        affected: LandPlayAffected::Filter(FilterController::You),
        additional: 1,
    });
    state.land_play_modifiers.push(LandPlayModifier {
        source: ObjectId(51),
        controller: p0,
        affected: LandPlayAffected::Filter(FilterController::You),
        additional: 1,
    });
    assert_eq!(effective_land_play_limit(&state.land_play_modifiers, p0), 3);

    let lands: Vec<ObjectId> = (0..3).map(ObjectId).collect();
    for &id in &lands {
        state
            .objects
            .insert(id, card_object(id, land_definition(), p0, ZoneKind::Hand));
        state
            .zone_mut(ZoneKind::Hand, Some(p0))
            .unwrap()
            .push_bottom(id);
    }

    let mut engine = Engine::new(state);
    for &card in &lands {
        engine
            .submit_action(PlayerAction {
                player: p0,
                kind: PlayerActionKind::PlayLand { card },
            })
            .unwrap();
        let status = drain(&mut engine);
        assert!(
            matches!(status, StepStatus::AwaitingAction { .. }),
            "expected {card:?} to be playable, got {status:?}"
        );
    }
}

/// A modifier scoped to the opponent doesn't raise this player's own limit.
#[test]
fn play_land_opponent_scoped_modifier_does_not_apply_to_you() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    state.land_play_modifiers.push(LandPlayModifier {
        source: ObjectId(50),
        controller: p1,
        affected: LandPlayAffected::Filter(FilterController::You),
        additional: 1,
    });
    assert_eq!(
        effective_land_play_limit(&state.land_play_modifiers, p0),
        1,
        "p0 isn't the modifier's controller"
    );
    assert_eq!(
        effective_land_play_limit(&state.land_play_modifiers, p1),
        2,
        "p1 is"
    );
}

#[test]
fn play_land_event_gets_causal_ids() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    state.objects.insert(
        card,
        card_object(card, land_definition(), p0, ZoneKind::Hand),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);

    let mut engine = Engine::new(state);
    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::PlayLand { card },
        })
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
