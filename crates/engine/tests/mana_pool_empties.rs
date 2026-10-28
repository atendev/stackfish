//! CR 500.5, 106.4: unspent mana empties as each step or phase ends. `advance_turn_step` is the single choke point every step transition runs through.

use stackfish_engine::engine::Engine;
use stackfish_engine::*;
use stackfish_testutil::{drain, two_player_state, zones};

fn state_in_precombat_main() -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        phase: Phase::Main,
        step: Step::PreCombatMain,
        turn_number: 1,
        ..two_player_state(zones(p0, p1, &[ZoneKind::Hand, ZoneKind::Library, ZoneKind::Battlefield]))
    }
}

fn pass_both(engine: &mut Engine, p0: PlayerId, p1: PlayerId) {
    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine);
    engine.submit_action(PlayerAction { player: p1, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine);
}

#[test]
fn mana_empties_the_moment_the_step_it_was_added_in_ends() {
    let mut state = state_in_precombat_main();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    state.player_mut(p0).unwrap().mana_pool.add_unit(ManaUnit::Colored(Color::White));
    assert_eq!(state.player(p0).unwrap().mana_pool.white, 1);

    let mut engine = Engine::new(state);
    // One full pass-round: PreCombatMain -> BeginningOfCombat. CR 500.5 fires
    // on leaving PreCombatMain, not only at the end of the whole turn.
    pass_both(&mut engine, p0, p1);
    assert_eq!(engine.state().phase, Phase::Combat);
    assert_eq!(engine.state().step, Step::BeginningOfCombat);
    assert_eq!(engine.state().player(p0).unwrap().mana_pool.white, 0, "mana pool empties as the step it floated in ends");
}

#[test]
fn mana_does_not_survive_into_the_next_turns_upkeep() {
    let mut state = state_in_precombat_main();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    state.player_mut(p0).unwrap().mana_pool.add_unit(ManaUnit::Colored(Color::White));

    let mut engine = Engine::new(state);
    // PreCombatMain -> Combat(no attackers, auto-skips to EndOfCombat)
    // -> PostCombatMain -> End -> Cleanup(no discard) -> next turn Untap
    // (no priority) -> Upkeep.
    for _ in 0..6 {
        pass_both(&mut engine, p0, p1);
    }
    assert_eq!(engine.state().turn_number, 2);
    assert_eq!(engine.state().phase, Phase::Beginning);
    assert_eq!(engine.state().step, Step::Upkeep);
    assert_eq!(engine.state().player(p0).unwrap().mana_pool.white, 0, "no mana should have carried over from turn 1's main phase");
}
