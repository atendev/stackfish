//! Phase 5 dry-run slice: PassPriority + the minimal pipeline loop back to
//! PriorityWindow (see docs/implementation.md, "Phase 5: Dry Run #2 -
//! pass-priority-loop"). Builds `GameState` directly via struct literals
//! (same pattern as `stackfish_scenario`, since the engine's own
//! constructors are still `todo!()` past PassPriority) so each test has
//! full access to `Engine`'s internals (`StepStatus`, `pipeline_context_active`)
//! that the higher-level scenario-TOML harness abstracts away.

use std::collections::HashMap;

use stackfish_engine::engine::{ActionError, Engine, StepStatus};
use stackfish_engine::*;

fn two_player_state() -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        players: vec![
            PlayerState {
                player_id: p0,
                life: 20,
                mana_pool: ManaPool { white: 0, blue: 0, black: 0, red: 0, green: 0, colorless: 0 },
                counters: CounterSet::new(),
                emblems: Vec::new(),
                commander_damage_taken: Vec::new(),
                eliminated: false,
            },
            PlayerState {
                player_id: p1,
                life: 20,
                mana_pool: ManaPool { white: 0, blue: 0, black: 0, red: 0, green: 0, colorless: 0 },
                counters: CounterSet::new(),
                emblems: Vec::new(),
                commander_damage_taken: Vec::new(),
                eliminated: false,
            },
        ],
        turn_order: vec![p0, p1],
        active_player: p0,
        turn_number: 1,
        phase: Phase::PreGame,
        step: Step::Untap,
        priority_player: p0,
        consecutive_passes: 0,
        postcombat_main: false,
        lands_played_this_turn: HashMap::new(),
        objects: HashMap::new(),
        zones: Vec::new(),
        stack: Vec::new(),
        continuous_effects: Vec::new(),
        replacement_effects: Vec::new(),
        abilities: Vec::new(),
        ledger: Vec::new(),
        rng: RngState { seed: 1, draws_consumed: 0 },
        next_event_id: 0,
        next_sequence_no: 0,
    }
}

/// Runs `step()` until it stops being `Progressed`, returning the last
/// status. Mirrors `stackfish_scenario::run_action`'s loop but keeps the
/// status visible to the caller instead of collapsing it to `StepOutcome`.
fn drain(engine: &mut Engine) -> StepStatus {
    loop {
        let result = engine.step();
        match result.status {
            StepStatus::Progressed => continue,
            other => return other,
        }
    }
}

#[test]
fn pass_priority_command_works() {
    let mut engine = Engine::new(two_player_state());
    engine
        .submit_action(PlayerAction { player: PlayerId(0), kind: PlayerActionKind::PassPriority })
        .expect("player 0 holds priority, should be accepted");

    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == PlayerId(1)));
    assert_eq!(engine.state().ledger.len(), 1);
    assert!(matches!(engine.state().ledger[0].kind, GameEventKind::PriorityPassed { player } if player == PlayerId(0)));
    assert_eq!(engine.state().priority_player, PlayerId(1));
    assert_eq!(engine.state().consecutive_passes, 1);
}

#[test]
fn wrong_player_pass_priority_is_rejected() {
    let mut engine = Engine::new(two_player_state());
    let err = engine
        .submit_action(PlayerAction { player: PlayerId(1), kind: PlayerActionKind::PassPriority })
        .expect_err("player 1 doesn't hold priority yet");
    assert!(matches!(err, ActionError::NotYourTurn));
}

#[test]
fn all_pass_with_empty_stack_advances_turn_step() {
    let mut engine = Engine::new(two_player_state());

    engine
        .submit_action(PlayerAction { player: PlayerId(0), kind: PlayerActionKind::PassPriority })
        .unwrap();
    drain(&mut engine);
    assert_eq!(engine.state().consecutive_passes, 1, "one pass shouldn't trigger all-pass yet");
    assert_eq!(engine.state().phase, Phase::PreGame, "no turn advance yet");

    engine
        .submit_action(PlayerAction { player: PlayerId(1), kind: PlayerActionKind::PassPriority })
        .unwrap();
    let status = drain(&mut engine);

    // Both players passed in a row with an empty stack - CR 117.4 - so the
    // engine should have driven itself through AdvanceTurnStep and landed
    // back on PriorityWindow without needing a third step() call from here.
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == PlayerId(0)));
    assert_eq!(engine.state().consecutive_passes, 0, "pass count resets once all-pass resolves");
    assert_eq!(engine.state().phase, Phase::Beginning);
    assert_eq!(engine.state().step, Step::Untap);
    assert_eq!(engine.state().priority_player, PlayerId(0), "priority reset to active player");
    assert!(!engine.pipeline_context_active(), "pipeline_context must be cleared once back at PriorityWindow");

    // 2 PriorityPassed (one per pass) + PhaseStarted + StepStarted + TurnStarted.
    assert_eq!(engine.state().ledger.len(), 5);
    assert!(matches!(
        engine.state().ledger[2].kind,
        GameEventKind::PhaseStarted { phase: Phase::Beginning }
    ));
    assert!(matches!(engine.state().ledger[3].kind, GameEventKind::StepStarted { step: Step::Untap }));
    assert!(matches!(
        engine.state().ledger[4].kind,
        GameEventKind::TurnStarted { player, turn_number: 1 } if player == PlayerId(0)
    ));
}

#[test]
fn all_pass_with_nonempty_stack_resolves_top_instead_of_advancing_step() {
    let mut state = two_player_state();
    let stack_id = ObjectId(0);
    state.stack.push(StackObject {
        stack_id,
        kind: StackEntryKind::Spell,
        source: stack_id,
        source_lki: None,
        controller: PlayerId(0),
        chosen_modes: Vec::new(),
        targets: Vec::new(),
        x_value: None,
        costs_paid: Vec::new(),
        operation: ScriptHandle(0),
    });
    let mut engine = Engine::new(state);

    engine
        .submit_action(PlayerAction { player: PlayerId(0), kind: PlayerActionKind::PassPriority })
        .unwrap();
    drain(&mut engine);
    engine
        .submit_action(PlayerAction { player: PlayerId(1), kind: PlayerActionKind::PassPriority })
        .unwrap();
    let status = drain(&mut engine);

    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == PlayerId(0)));
    // Stack object popped and resolved - turn/phase must NOT have advanced.
    assert!(engine.state().stack.is_empty());
    assert_eq!(engine.state().phase, Phase::PreGame);
    assert_eq!(engine.state().step, Step::Untap);
    assert_eq!(engine.state().priority_player, PlayerId(0));
    assert!(!engine.pipeline_context_active());

    // 2 PriorityPassed + 1 StackObjectResolved.
    assert_eq!(engine.state().ledger.len(), 3);
    assert!(matches!(
        engine.state().ledger[2].kind,
        GameEventKind::StackObjectResolved { stack_id: id } if id == stack_id
    ));
}

#[test]
fn committed_events_get_causal_ids() {
    let mut engine = Engine::new(two_player_state());
    engine
        .submit_action(PlayerAction { player: PlayerId(0), kind: PlayerActionKind::PassPriority })
        .unwrap();
    drain(&mut engine);

    let event = &engine.state().ledger[0];
    assert!(event.caused_by_command.is_some(), "event committed from a PendingAction run must carry its CommandId");
    assert!(event.parent_event.is_none());
}

#[test]
fn pipeline_context_clears_between_separate_runs() {
    let mut engine = Engine::new(two_player_state());
    assert!(!engine.pipeline_context_active(), "nothing in flight before the first action");

    engine
        .submit_action(PlayerAction { player: PlayerId(0), kind: PlayerActionKind::PassPriority })
        .unwrap();
    // First step() call moves PendingAction -> ReplacementCheck, which is
    // where pipeline_context gets set - check it's actually populated
    // mid-run, not just (trivially) absent the whole time.
    let first = engine.step();
    assert!(matches!(first.status, StepStatus::Progressed));
    assert!(engine.pipeline_context_active(), "context must be set once a pipeline run begins");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { .. }));
    assert!(!engine.pipeline_context_active(), "first run's context must be cleared once it ends");

    engine
        .submit_action(PlayerAction { player: PlayerId(1), kind: PlayerActionKind::PassPriority })
        .unwrap();
    let first2 = engine.step();
    assert!(matches!(first2.status, StepStatus::Progressed));
    assert!(engine.pipeline_context_active(), "second run's context must be set too, fresh");
    drain(&mut engine);
    assert!(!engine.pipeline_context_active(), "second run's context must also be cleared");
}

#[test]
fn non_pass_action_breaks_pass_chain() {
    let mut engine = Engine::new(two_player_state());

    engine
        .submit_action(PlayerAction { player: PlayerId(0), kind: PlayerActionKind::PassPriority })
        .unwrap();
    drain(&mut engine);
    assert_eq!(engine.state().consecutive_passes, 1, "one pass recorded, not yet all-pass");
    assert_eq!(engine.state().priority_player, PlayerId(1));

    // Player 1 (who now holds priority) takes a non-pass action. CR 117.3c:
    // any action other than passing breaks the chain immediately, even
    // though the count hadn't reached turn_order.len() yet.
    engine
        .submit_action(PlayerAction {
            player: PlayerId(1),
            kind: PlayerActionKind::MakeChoice { description: "test".to_string(), selection: Vec::new() },
        })
        .unwrap();
    drain(&mut engine);

    assert_eq!(engine.state().consecutive_passes, 0, "non-pass action must reset the chain");
    assert_eq!(
        engine.state().priority_player,
        PlayerId(1),
        "PriorityHandoff::Unchanged - priority stays with whoever just acted"
    );
    assert_eq!(engine.state().phase, Phase::PreGame, "chain reset, not all-pass - no turn advance");
}

#[test]
fn step_status_alternates_progressed_then_awaiting_action() {
    let mut engine = Engine::new(two_player_state());
    engine
        .submit_action(PlayerAction { player: PlayerId(0), kind: PlayerActionKind::PassPriority })
        .unwrap();

    // PendingAction -> ReplacementCheck -> CommitEvents -> SbaLoop ->
    // CollectTriggers -> PutTriggersApnap, each its own Progressed step()
    // call, then AwaitingAction once back at PriorityWindow.
    let mut progressed_count = 0;
    loop {
        let result = engine.step();
        match result.status {
            StepStatus::Progressed => progressed_count += 1,
            StepStatus::AwaitingAction { player } => {
                assert_eq!(player, PlayerId(1));
                break;
            }
            other => panic!("unexpected status before AwaitingAction: {other:?}"),
        }
    }
    assert!(progressed_count >= 5, "expected at least 5 pipeline stages, got {progressed_count}");
}
