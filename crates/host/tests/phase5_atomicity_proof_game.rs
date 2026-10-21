//! 024/025: a rejected Answer changes nothing, proved on whole games. The bot
//! sends deliberately illegal Answers before each real one and `Session`
//! checks the engine is exactly as it was after each. Then the game with the
//! illegal Answers must end in exactly the state of the game without them,
//! which is what a replay of the accepted Answers alone produces.

use serde_json::Value;
use stackfish_host::{Bot, Outcome, Session, proof_cards, proof_setup_request};

const ANSWER_LIMIT: usize = 50_000;

fn create(players: usize, seed: u64) -> Session {
    let (session, response) = Session::create(&proof_setup_request(players, seed), &proof_cards())
        .unwrap_or_else(|e| panic!("the proof setup is valid: {}", String::from_utf8_lossy(&e)));
    let response: Value = serde_json::from_slice(&response).expect("a response is JSON");
    assert_eq!(response["body"]["status"], "created");
    session
}

struct Game {
    outcome: Outcome,
    bot: Bot,
    session: Session,
}

fn play(players: usize, seed: u64, injecting: bool) -> Game {
    let mut session = create(players, seed);
    let mut bot = Bot::new(seed).recording(&[]);
    if injecting {
        bot = bot.injecting_illegal_answers();
    }
    let outcome = bot.play(&mut session, ANSWER_LIMIT).unwrap_or_else(|e| panic!("seed {seed}, injecting {injecting}: {e}"));
    Game { outcome, bot, session }
}

#[test]
fn illegal_answers_at_every_decision_change_nothing_and_the_game_ends_the_same() {
    let mut shapes = std::collections::BTreeMap::<String, usize>::new();
    for seed in 0..6 {
        let plain = play(2, seed, false);
        let injected = play(2, seed, true);

        assert!(injected.outcome.illegal_rejected > 0, "seed {seed}: no illegal Answer was sent");
        assert_eq!(plain.outcome.illegal_rejected, 0);
        assert_eq!(injected.outcome.winner, plain.outcome.winner, "seed {seed}");
        assert_eq!(injected.outcome.answers, plain.outcome.answers, "seed {seed}");
        assert_eq!(injected.outcome.rejected, plain.outcome.rejected, "seed {seed}");
        for seat in 0..2 {
            assert_eq!(injected.bot.events(seat), plain.bot.events(seat), "seed {seed}, seat {seat}: events differ");
        }
        let (injected_end, plain_end) = (injected.session.snapshot(), plain.session.snapshot());
        assert!(injected_end.state == plain_end.state, "seed {seed}: the final GameState differs");
        assert_eq!(injected_end.next_command_id, plain_end.next_command_id, "seed {seed}: command ids differ");

        for (shape, count) in injected.bot.illegal_by_shape() {
            *shapes.entry(shape.clone()).or_default() += count;
        }
    }
    // Every Decision shape the bot meets in these games was attacked.
    for shape in ["action", "pick", "yes_no", "payment", "declare_attackers", "declare_blockers"] {
        assert!(shapes.get(shape).copied().unwrap_or(0) > 0, "no illegal Answer was sent at a {shape} Decision: {shapes:?}");
    }
    println!("illegal Answers turned down, by Decision shape: {shapes:?}");
}

#[test]
fn the_same_holds_in_a_three_player_game() {
    let plain = play(3, 7, false);
    let injected = play(3, 7, true);
    assert!(injected.outcome.illegal_rejected > 0);
    assert_eq!(injected.outcome.winner, plain.outcome.winner);
    assert_eq!(injected.outcome.answers, plain.outcome.answers);
    assert!(injected.session.snapshot().state == plain.session.snapshot().state);
}
