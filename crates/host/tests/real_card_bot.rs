//! The random bot plays two 40-card decks of real cards from `../stackfish-db/stackfish.db`.
//! The tests skip when the database is absent, so CI stays green without it.

use std::path::PathBuf;

use serde_json::{Value, json};
use stackfish_host::{Bot, CardDbCardSource, Outcome, Session};

const ANSWER_LIMIT: usize = 50_000;

struct Deck {
    cards: &'static [(&'static str, u32)],
}

const WHITE_BLUE: Deck = Deck {
    cards: &[
        ("bc71ebf6-2056-41f7-be35-b2e5c34afa99", 8), // Plains
        ("b2c6aa39-2d2a-459c-a555-fb48ba993373", 8), // Island
        ("b3b9a87d-cb95-435c-90b6-037406cab32e", 4), // Elite Vanguard
        ("39780477-01ba-40c6-8fe6-2178efb10b92", 4), // Eager Cadet
        ("218d9277-c179-4de3-9c7f-79b5a6d4fa38", 4), // Merfolk of the Pearl Trident
        ("10706fd1-7847-4316-be8d-59b56143ce45", 4), // Coral Eel
        ("837182db-1bf3-4a2c-bd01-1af9d9873561", 4), // Unsummon
        ("cc187110-1148-4090-bbb8-e205694a39f5", 4), // Counterspell
        ("7bff8d4a-1c7d-48b8-b3e3-737dd6f01823", 4), // Quick Study
    ],
};

const BLACK_RED: Deck = Deck {
    cards: &[
        ("56719f6a-1a6c-4c0a-8d21-18f7d7350b68", 8), // Swamp
        ("a3fb7228-e76b-4e96-a40e-20b5fed75685", 8), // Mountain
        ("bca13a12-6723-4a5e-8f1b-21646a8b3e7e", 4), // Muck Rats
        ("fea95888-e16a-4209-9cd4-623f7f4d2f67", 4), // Walking Corpse
        ("a37159df-f6d7-4db6-85de-0ea77f425993", 4), // Mons's Goblin Raiders
        ("343ecf2f-0bbc-4840-8fb3-f5f61b26e76f", 4), // Swab Goblin
        ("4457ed35-7c10-48c8-9776-456485fdf070", 4), // Lightning Bolt
        ("a9d288b8-cdc1-4e55-a0c9-d6edfc95e65d", 4), // Shock
        ("13718d93-fe46-4a18-a511-88649ef1c9de", 4), // Sovereign's Bite
    ],
};

fn db_path() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .expect("manifest is in crates/host")
        .parent()
        .expect("crates is inside the repo root")
        .parent()
        .expect("repo root is inside Documents/Github")
        .join("stackfish-db")
        .join("stackfish.db")
}

fn create(request: &[u8]) -> Session {
    let source = CardDbCardSource::open(db_path()).expect("the real card database opens");
    let (session, response) = Session::create(request, &source).unwrap_or_else(|e| {
        panic!("the real-card setup is valid: {}", String::from_utf8_lossy(&e));
    });
    let response: Value = serde_json::from_slice(&response).expect("a response is JSON");
    assert_eq!(response["body"]["status"], "created");
    session
}

fn setup_request(players: usize, seed: u64) -> Vec<u8> {
    fn deck_json(deck: &Deck) -> Value {
        let entries: Vec<Value> =
            deck.cards.iter().map(|(card, count)| json!({ "card": card, "count": count })).collect();
        json!({ "deck": entries })
    }

    let decks: Vec<Value> =
        (0..players).map(|i| deck_json(if i % 2 == 0 { &WHITE_BLUE } else { &BLACK_RED })).collect();
    let body = json!({
        "players": decks,
        "format": "standard",
        "seed": seed.to_string(),
        "overrides": { "shuffle_libraries": false, "skip_mulligan": true },
    });
    serde_json::to_vec(
        &json!({ "v": stackfish_host::PROTOCOL_VERSION, "kind": "setup", "body": body })
    )
    .expect("a setup request holds only strings, numbers and lists")
}

fn play(players: usize, seed: u64) -> Outcome {
    let mut session = create(&setup_request(players, seed));
    Bot::new(seed).play(&mut session, ANSWER_LIMIT).unwrap_or_else(|e| panic!("seed {seed}: {e}"))
}

fn assert_finished(outcome: &Outcome, players: usize, seed: u64) {
    assert_eq!(outcome.rejected, 0, "seed {seed}: the Session rejected a bot Answer");
    assert!(outcome.answers > 20, "seed {seed}: the game ended after {} answers", outcome.answers);
    if let Some(winner) = outcome.winner {
        assert!((winner as usize) < players, "seed {seed}: winner {winner} is not a player");
    }
}

#[test]
fn real_card_bot_plays_two_player_games() {
    let path = db_path();
    if !path.exists() {
        eprintln!("skipping real_card_bot_plays_two_player_games: database not at {path:?}");
        return;
    }

    let mut winners = Vec::new();
    for seed in 0..5 {
        let outcome = play(2, seed);
        assert_finished(&outcome, 2, seed);
        winners.push(outcome.winner);
    }
    println!("winners over 5 seeds: {winners:?}");
}

#[test]
fn real_card_bot_games_are_byte_identical_for_the_same_seed() {
    let path = db_path();
    if !path.exists() {
        eprintln!("skipping real_card_bot_games_are_byte_identical_for_the_same_seed: database not at {path:?}");
        return;
    }

    for seed in [0, 1, 2] {
        let first = play(2, seed);
        let second = play(2, seed);
        assert_eq!(
            first.transcript_hash, second.transcript_hash,
            "seed {seed} produced two different transcripts"
        );
    }
}

#[test]
#[ignore = "30 real-card bot games, run on demand"]
fn real_card_bot_plays_thirty_games() {
    let path = db_path();
    if !path.exists() {
        eprintln!("skipping real_card_bot_plays_thirty_games: database not at {path:?}");
        return;
    }

    let mut winners = Vec::new();
    for seed in 0..30 {
        let outcome = play(2, seed);
        assert_finished(&outcome, 2, seed);
        winners.push(outcome.winner);
    }
    println!("winners over 30 seeds: {winners:?}");
}

#[test]
fn real_card_bot_loads_every_script_without_errors() {
    let path = db_path();
    if !path.exists() {
        eprintln!("skipping real_card_bot_loads_every_script_without_errors: database not at {path:?}");
        return;
    }

    let mut session = create(&setup_request(2, 0));
    let mut bot = Bot::new(0).recording(&[0, 1]);
    let outcome = bot.play(&mut session, ANSWER_LIMIT).expect("the game finishes");
    assert_finished(&outcome, 2, 0);

    for seat in [0, 1] {
        let errors: Vec<_> =
            bot.events(seat).iter().filter(|e| e["kind"] == "script_error").collect();
        assert!(errors.is_empty(), "seat {seat} saw script errors: {errors:?}");
    }
}
