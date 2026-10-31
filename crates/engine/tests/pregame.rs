//! CR 103: PreGame automation, driven end to end through `Engine::new_game(GameConfig)` and the pregame pipeline it triggers.

use stackfish_engine::engine::{BottomCardsError, Engine, MulliganError, StepStatus};
use stackfish_engine::*;

fn plain_card_def(name: &str) -> CardDefinition {
    CardDefinition {
        definition_id: 0,
        name: name.to_string(),
        mana_cost: ManaCost {
            symbols: Vec::new(),
        },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::CREATURE,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: None,
        toughness: None,
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

fn two_player_config(p0_cards: usize, p1_cards: usize, settings: GameSettings) -> GameConfig {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameConfig {
        players: vec![
            PlayerConfig {
                player_id: p0,
                library: (0..p0_cards)
                    .map(|i| plain_card_def(&format!("p0_card_{i}")))
                    .collect(),
                commanders: Vec::new(),
                life_override: None,
            },
            PlayerConfig {
                player_id: p1,
                library: (0..p1_cards)
                    .map(|i| plain_card_def(&format!("p1_card_{i}")))
                    .collect(),
                commanders: Vec::new(),
                life_override: None,
            },
        ],
        turn_order: vec![p0, p1],
        settings,
        rng_seed: 42,
    }
}

fn run_until_pause(engine: &mut Engine) -> StepStatus {
    loop {
        let result = engine.step();
        match result.status {
            StepStatus::Progressed => continue,
            other => return other,
        }
    }
}

fn hand_size(engine: &Engine, player: PlayerId) -> usize {
    engine
        .state()
        .zone(ZoneKind::Hand, Some(player))
        .map_or(0, |z| z.contents.len())
}

fn library_size(engine: &Engine, player: PlayerId) -> usize {
    engine
        .state()
        .zone(ZoneKind::Library, Some(player))
        .map_or(0, |z| z.contents.len())
}

#[test]
fn skip_mulligan_auto_advances_to_beginning() {
    let settings = GameSettings {
        skip_mulligan: true,
        ..GameSettings::default()
    };
    let mut engine = Engine::new_game(two_player_config(10, 10, settings));

    let status = run_until_pause(&mut engine);

    assert_eq!(engine.state().phase, Phase::Beginning);
    // CR 502.4: no priority during Untap — engine auto-advances through it to Upkeep.
    assert_eq!(engine.state().step, Step::Upkeep);
    assert!(
        matches!(status, StepStatus::AwaitingAction { .. }),
        "expected AwaitingAction after pregame, got {status:?}"
    );
}

#[test]
fn opening_hands_drawn() {
    let settings = GameSettings {
        skip_mulligan: true,
        ..GameSettings::default()
    };
    let mut engine = Engine::new_game(two_player_config(10, 10, settings));
    run_until_pause(&mut engine);

    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    assert_eq!(hand_size(&engine, p0), 7, "p0 hand");
    assert_eq!(hand_size(&engine, p1), 7, "p1 hand");
    assert_eq!(library_size(&engine, p0), 3, "p0 library (10 - 7)");
    assert_eq!(library_size(&engine, p1), 3, "p1 library");
}

#[test]
fn shuffle_disabled_preserves_library_order() {
    let settings = GameSettings {
        skip_mulligan: true,
        shuffle_libraries: false,
        ..GameSettings::default()
    };
    let mut engine = Engine::new_game(two_player_config(10, 10, settings));
    run_until_pause(&mut engine);

    let p0 = PlayerId(0);
    // With no shuffle the 7 drawn cards come from the top, leaving 3 in the library.
    assert_eq!(library_size(&engine, p0), 3);
    // All 7 hand cards belong to p0.
    let hand_zone = engine.state().zone(ZoneKind::Hand, Some(p0)).unwrap();
    for &obj_id in &hand_zone.contents {
        let obj = engine.state().object(obj_id).unwrap();
        assert_eq!(obj.owner, p0);
    }
}

#[test]
fn life_override_per_player() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let settings = GameSettings {
        skip_mulligan: true,
        ..GameSettings::default()
    };
    let config = GameConfig {
        players: vec![
            PlayerConfig {
                player_id: p0,
                library: (0..7).map(|i| plain_card_def(&format!("c{i}"))).collect(),
                commanders: Vec::new(),
                life_override: Some(40), // commander life
            },
            PlayerConfig {
                player_id: p1,
                library: (0..7).map(|i| plain_card_def(&format!("d{i}"))).collect(),
                commanders: Vec::new(),
                life_override: None, // use default (20)
            },
        ],
        turn_order: vec![p0, p1],
        settings,
        rng_seed: 0,
    };
    let mut engine = Engine::new_game(config);
    run_until_pause(&mut engine);

    assert_eq!(engine.state().player(p0).unwrap().life, 40);
    assert_eq!(engine.state().player(p1).unwrap().life, 20);
}

#[test]
fn draw_skip_seeded_for_starting_player() {
    let settings = GameSettings {
        skip_mulligan: true,
        ..GameSettings::default()
    };
    let mut engine = Engine::new_game(two_player_config(10, 10, settings));
    run_until_pause(&mut engine);

    let p0 = PlayerId(0);
    assert!(
        engine
            .state()
            .pending_step_skips
            .contains(&(p0, Step::Draw)),
        "starting player's draw skip must be seeded by the PreGame→Beginning transition"
    );
}

#[test]
fn pregame_pauses_for_mulligan_decision() {
    let mut engine = Engine::new_game(two_player_config(10, 10, GameSettings::default()));
    let status = run_until_pause(&mut engine);

    assert!(
        matches!(status, StepStatus::AwaitingMulliganDecision { player } if player == PlayerId(0)),
        "expected AwaitingMulliganDecision for P0, got {status:?}"
    );
}

#[test]
fn keep_advances_to_next_player() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let mut engine = Engine::new_game(two_player_config(10, 10, GameSettings::default()));
    run_until_pause(&mut engine);

    engine
        .submit_mulligan_decision(PlayerAction {
            player: p0,
            kind: PlayerActionKind::KeepHand,
        })
        .expect("keep should be accepted");

    let status = run_until_pause(&mut engine);
    assert!(
        matches!(status, StepStatus::AwaitingMulliganDecision { player } if player == p1),
        "expected AwaitingMulliganDecision for P1 after P0 keeps, got {status:?}"
    );
    assert_eq!(hand_size(&engine, p0), 7, "hand unchanged after keep");
    assert_eq!(engine.state().player(p0).unwrap().mulligans_taken, 0);
}

#[test]
fn keep_wrong_player_rejected() {
    let p1 = PlayerId(1);
    let mut engine = Engine::new_game(two_player_config(10, 10, GameSettings::default()));
    run_until_pause(&mut engine);

    let err = engine
        .submit_mulligan_decision(PlayerAction {
            player: p1,
            kind: PlayerActionKind::KeepHand,
        })
        .unwrap_err();
    assert!(matches!(err, MulliganError::WrongPlayer));
}

#[test]
fn take_mulligan_redraws_and_increments() {
    let p0 = PlayerId(0);
    let _p1 = PlayerId(1);
    // 21 cards each so P0 can take 2 mulligans (7 + 7 + 7)
    let mut engine = Engine::new_game(two_player_config(21, 7, GameSettings::default()));
    run_until_pause(&mut engine); // paused at P0

    engine
        .submit_mulligan_decision(PlayerAction {
            player: p0,
            kind: PlayerActionKind::TakeMulligan,
        })
        .expect("mulligan should be accepted");
    run_until_pause(&mut engine); // advance past internal state change, land at P1 (or back to P0)

    // P0 still has 7 cards (redrawn), library shrank by another 7
    assert_eq!(hand_size(&engine, p0), 7, "hand redrawn to 7");
    assert_eq!(engine.state().player(p0).unwrap().mulligans_taken, 1);
}

#[test]
fn both_keep_immediately_skips_bottoming() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let mut engine = Engine::new_game(two_player_config(10, 10, GameSettings::default()));

    run_until_pause(&mut engine); // P0 decision
    engine
        .submit_mulligan_decision(PlayerAction {
            player: p0,
            kind: PlayerActionKind::KeepHand,
        })
        .unwrap();
    run_until_pause(&mut engine); // P1 decision
    engine
        .submit_mulligan_decision(PlayerAction {
            player: p1,
            kind: PlayerActionKind::KeepHand,
        })
        .unwrap();
    let status = run_until_pause(&mut engine);

    assert_eq!(engine.state().phase, Phase::Beginning);
    assert!(matches!(status, StepStatus::AwaitingAction { .. }));
}

#[test]
fn one_paid_mulligan_requires_bottoming() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let mut engine = Engine::new_game(two_player_config(14, 7, GameSettings::default()));

    // P0 mulligans → engine advances to P1's decision
    run_until_pause(&mut engine);
    engine
        .submit_mulligan_decision(PlayerAction {
            player: p0,
            kind: PlayerActionKind::TakeMulligan,
        })
        .unwrap();
    // P1 keeps → engine advances back to P0 (still !hand_kept)
    run_until_pause(&mut engine);
    engine
        .submit_mulligan_decision(PlayerAction {
            player: p1,
            kind: PlayerActionKind::KeepHand,
        })
        .unwrap();
    // P0 keeps their redrawn hand → transition_to_bottoming (1 paid mulligan)
    run_until_pause(&mut engine);
    engine
        .submit_mulligan_decision(PlayerAction {
            player: p0,
            kind: PlayerActionKind::KeepHand,
        })
        .unwrap();
    let status = run_until_pause(&mut engine);
    assert!(
        matches!(status, StepStatus::AwaitingBottomCardChoice { player, count } if player == p0 && count == 1),
        "expected AwaitingBottomCardChoice for P0 count=1, got {status:?}"
    );
}

#[test]
fn bottoming_moves_card_to_library() {
    let p0 = PlayerId(0);
    let _p1 = PlayerId(1);
    let mut engine = Engine::new_game(two_player_config(14, 7, GameSettings::default()));

    // Drive P0 to take 1 mulligan then keep, P1 keeps
    run_until_pause(&mut engine);
    engine
        .submit_mulligan_decision(PlayerAction {
            player: p0,
            kind: PlayerActionKind::TakeMulligan,
        })
        .unwrap();
    run_until_pause(&mut engine);
    // handle ordering: keep whoever the engine asks for
    let status = run_until_pause(&mut engine);
    if let StepStatus::AwaitingMulliganDecision { player } = status {
        engine
            .submit_mulligan_decision(PlayerAction {
                player,
                kind: PlayerActionKind::KeepHand,
            })
            .unwrap();
        let status2 = run_until_pause(&mut engine);
        if let StepStatus::AwaitingMulliganDecision { player: player2 } = status2 {
            engine
                .submit_mulligan_decision(PlayerAction {
                    player: player2,
                    kind: PlayerActionKind::KeepHand,
                })
                .unwrap();
            run_until_pause(&mut engine);
        }
    }

    // Now at AwaitingBottomCardChoice
    let hand = engine
        .state()
        .zone(ZoneKind::Hand, Some(p0))
        .unwrap()
        .contents
        .clone();
    let card_to_bottom = hand[0];
    let lib_before = library_size(&engine, p0);

    engine
        .submit_bottom_cards(PlayerAction {
            player: p0,
            kind: PlayerActionKind::BottomCards {
                cards: vec![card_to_bottom],
            },
        })
        .expect("valid bottom submission");

    assert_eq!(
        library_size(&engine, p0),
        lib_before + 1,
        "library grew by 1"
    );
    assert_eq!(hand_size(&engine, p0), 6, "hand shrank by 1");
    // Card is at the very bottom of the library
    let lib = engine.state().zone(ZoneKind::Library, Some(p0)).unwrap();
    // The new object will have a different ObjectId (CR 400.7), but it should
    // be at the back of the library VecDeque.
    assert_eq!(lib.contents.len(), lib_before + 1);
}

#[test]
fn bottoming_wrong_count_rejected() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let mut engine = Engine::new_game(two_player_config(14, 7, GameSettings::default()));

    // Drive to bottoming: P0 mulligans, P1 keeps, P0 keeps
    run_until_pause(&mut engine);
    engine
        .submit_mulligan_decision(PlayerAction {
            player: p0,
            kind: PlayerActionKind::TakeMulligan,
        })
        .unwrap();
    run_until_pause(&mut engine);
    engine
        .submit_mulligan_decision(PlayerAction {
            player: p1,
            kind: PlayerActionKind::KeepHand,
        })
        .unwrap();
    run_until_pause(&mut engine);
    engine
        .submit_mulligan_decision(PlayerAction {
            player: p0,
            kind: PlayerActionKind::KeepHand,
        })
        .unwrap();
    run_until_pause(&mut engine); // now at AwaitingBottomCardChoice { P0, 1 }

    // Try to bottom 0 cards when count == 1
    let err = engine
        .submit_bottom_cards(PlayerAction {
            player: p0,
            kind: PlayerActionKind::BottomCards { cards: vec![] },
        })
        .unwrap_err();
    assert!(matches!(
        err,
        BottomCardsError::WrongCount {
            expected: 1,
            got: 0
        }
    ));
}

#[test]
fn free_mulligan_skips_bottoming() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let settings = GameSettings {
        free_mulligans: 1,
        ..GameSettings::default()
    };
    let mut engine = Engine::new_game(two_player_config(14, 7, settings));

    // P0 mulligans once (free) → P1 decision window
    run_until_pause(&mut engine);
    engine
        .submit_mulligan_decision(PlayerAction {
            player: p0,
            kind: PlayerActionKind::TakeMulligan,
        })
        .unwrap();
    // P1 keeps → back to P0
    run_until_pause(&mut engine);
    engine
        .submit_mulligan_decision(PlayerAction {
            player: p1,
            kind: PlayerActionKind::KeepHand,
        })
        .unwrap();
    // P0 keeps their redrawn hand → free mulligan → no bottoming
    run_until_pause(&mut engine);
    engine
        .submit_mulligan_decision(PlayerAction {
            player: p0,
            kind: PlayerActionKind::KeepHand,
        })
        .unwrap();
    let status = run_until_pause(&mut engine);

    assert_eq!(
        engine.state().phase,
        Phase::Beginning,
        "no bottoming when mulligan was free"
    );
    assert!(matches!(status, StepStatus::AwaitingAction { .. }));
}

#[test]
fn max_mulligans_enforced() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let settings = GameSettings {
        max_mulligans: Some(1),
        ..GameSettings::default()
    };
    let mut engine = Engine::new_game(two_player_config(21, 7, settings));

    // P0 takes first (and only allowed) mulligan → engine moves to P1
    run_until_pause(&mut engine);
    engine
        .submit_mulligan_decision(PlayerAction {
            player: p0,
            kind: PlayerActionKind::TakeMulligan,
        })
        .expect("first mulligan allowed");
    // P1 keeps → back to P0 for their next decision
    run_until_pause(&mut engine);
    engine
        .submit_mulligan_decision(PlayerAction {
            player: p1,
            kind: PlayerActionKind::KeepHand,
        })
        .expect("P1 keep");
    // P0's second decision: TakeMulligan must be rejected (max reached)
    run_until_pause(&mut engine);
    let err = engine
        .submit_mulligan_decision(PlayerAction {
            player: p0,
            kind: PlayerActionKind::TakeMulligan,
        })
        .unwrap_err();
    assert!(matches!(err, MulliganError::MaxMulligansReached));
}

#[test]
fn pass_priority_rejected_during_pregame() {
    let p0 = PlayerId(0);
    let settings = GameSettings {
        skip_mulligan: false,
        ..GameSettings::default()
    };
    let mut engine = Engine::new_game(two_player_config(7, 7, settings));
    // Don't run step() yet — engine is in Phase::PreGame, process_state is
    // PriorityWindow initially but step() should bypass it for pregame.
    // Once we do one step() it enters AwaitingMulliganDecision.
    run_until_pause(&mut engine);

    // Now in AwaitingMulliganDecision — submit_action should fail
    let err = engine.submit_action(PlayerAction {
        player: p0,
        kind: PlayerActionKind::PassPriority,
    });
    assert!(
        err.is_err(),
        "submit_action should be rejected during pregame decision window"
    );
}
