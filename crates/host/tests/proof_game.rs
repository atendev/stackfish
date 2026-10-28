//! A random bot plays full games over bytes with the proof card set, reading nothing but
//! the Decisions it is sent.

use std::collections::{HashMap, HashSet};

use serde_json::{Value, json};
use stackfish_host::{Bot, Outcome, Session, proof_cards, proof_setup_request};

const ANSWER_LIMIT: usize = 50_000;

fn create(request: &[u8]) -> Session {
    let (session, response) = Session::create(request, &proof_cards()).unwrap_or_else(|e| {
        panic!("the proof setup is valid: {}", String::from_utf8_lossy(&e));
    });
    let response: Value = serde_json::from_slice(&response).expect("a response is JSON");
    assert_eq!(response["body"]["status"], "created");
    session
}

fn play(players: usize, seed: u64) -> Outcome {
    let mut session = create(&proof_setup_request(players, seed));
    Bot::new(seed).play(&mut session, ANSWER_LIMIT).unwrap_or_else(|e| panic!("seed {seed}: {e}"))
}

fn play_recorded(players: usize, seed: u64) -> (Outcome, Bot) {
    let mut session = create(&proof_setup_request(players, seed));
    let mut bot = Bot::new(seed).recording(&[]);
    let outcome = bot.play(&mut session, ANSWER_LIMIT).unwrap_or_else(|e| panic!("seed {seed}: {e}"));
    (outcome, bot)
}

fn assert_finished(outcome: &Outcome, players: usize, seed: u64) {
    assert_eq!(outcome.rejected, 0, "seed {seed}: the Session rejected a bot Answer");
    assert!(outcome.answers > 20, "seed {seed}: the game ended after {} answers", outcome.answers);
    if let Some(winner) = outcome.winner {
        assert!((winner as usize) < players, "seed {seed}: winner {winner} is not a player");
    }
}

#[test]
fn a_bot_plays_thirty_two_player_games_to_the_end() {
    let mut winners = HashSet::new();
    for seed in 0..30 {
        let (outcome, bot) = play_recorded(2, seed);
        assert_finished(&outcome, 2, seed);
        winners.insert(outcome.winner);
        for seat in 0..2 {
            let errors: Vec<_> = bot.events(seat).iter().filter(|e| e["kind"] == "script_error").collect();
            assert!(errors.is_empty(), "seed {seed}: a proof card script failed to load: {errors:?}");
        }
    }
    assert!(winners.contains(&Some(0)) && winners.contains(&Some(1)), "both players should win some games: {winners:?}");
}

const SPELLS: [&str; 21] = [
    "Grizzly Bear",
    "Stone Giant",
    "Wind Drake",
    "Cinder Duelist",
    "Scholar Imp",
    "Grave Rat",
    "Ember Pinger",
    "Mirror Sage",
    "Shock",
    "Divination",
    "Counterspell",
    "Bounce",
    "Fork in the Road",
    "Rampant Growth",
    "Raise Militia",
    "Peek",
    "Drain Touch",
    "Doom Blade",
    "Giant Growth Aura",
    "Spark Stone",
    "Iron Blade",
];

#[test]
fn the_games_cast_every_card_of_the_set_and_play_every_land() {
    let mut cast = HashSet::new();
    let mut lands = HashSet::new();
    for seed in 0..30 {
        let (_, bot) = play_recorded(2, seed);
        // A `spell_cast` event names only the stack object; the move onto the stack names the card.
        let mut on_stack = HashMap::new();
        for seat in 0..2 {
            for event in bot.events(seat) {
                if event["kind"] != "zone_change" {
                    continue;
                }
                let name = event["identity"]["name"].as_str();
                if let (Some(object), Some(name), "stack") = (event["object"].as_str(), name, event["to"].as_str().unwrap_or("")) {
                    on_stack.insert(object.to_string(), name.to_string());
                }
                if let (Some(name), "hand", "battlefield") = (name, event["from"].as_str().unwrap_or(""), event["to"].as_str().unwrap_or("")) {
                    lands.insert(name.to_string());
                }
            }
        }
        for seat in 0..2 {
            for event in bot.events(seat).iter().filter(|e| e["kind"] == "spell_cast") {
                if let Some(name) = event["object"].as_str().and_then(|o| on_stack.get(o)) {
                    cast.insert(name.clone());
                }
            }
        }
    }
    let missing: Vec<_> = SPELLS.iter().filter(|name| !cast.contains(**name)).collect();
    assert!(missing.is_empty(), "no game cast {missing:?}");
    for land in ["Mountain", "Forest", "Island", "Swamp"] {
        assert!(lands.contains(land), "no game played {land}");
    }
}

#[test]
fn the_same_seeds_give_a_byte_identical_game() {
    // A `HashMap` walk differs between runs only some of the time, so try several seeds.
    for seed in 0..8 {
        let first = play(2, seed);
        let second = play(2, seed);
        assert_eq!(first.transcript_hash, second.transcript_hash, "seed {seed} played two different games");
        assert_eq!(first.answers, second.answers);
    }
    assert_ne!(play(2, 11).transcript_hash, play(2, 12).transcript_hash, "another seed plays another game");
}

#[test]
fn three_players_play_through_the_same_protocol() {
    for seed in [3, 4, 5] {
        let outcome = play(3, seed);
        assert_finished(&outcome, 3, seed);
    }
}

#[test]
#[ignore = "500 games, run on demand: cargo test -p stackfish-host --test proof_game -- --ignored"]
fn five_hundred_seeds_all_finish_without_a_rejection() {
    for seed in 1000..1500 {
        let outcome = play(2, seed);
        assert_finished(&outcome, 2, seed);
    }
}

// The ledger's own numbers are not object ids.
fn ids_in(events: &[Value]) -> HashSet<String> {
    fn walk(value: &Value, key: &str, found: &mut HashSet<String>) {
        // Power, toughness, loyalty and defense are digit strings too.
        const NOT_IDS: [&str; 10] = [
            "event_id",
            "sequence_no",
            "caused_by_command",
            "parent_event",
            "original_event",
            "value",
            "power",
            "toughness",
            "loyalty",
            "defense",
        ];
        match value {
            Value::String(text) if !NOT_IDS.contains(&key) && !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()) => {
                found.insert(text.clone());
            }
            Value::Array(items) => items.iter().for_each(|v| walk(v, key, found)),
            Value::Object(map) => map.iter().for_each(|(k, v)| walk(v, k, found)),
            _ => {}
        }
    }
    let mut found = HashSet::new();
    events.iter().for_each(|e| walk(e, "", &mut found));
    found
}

#[test]
fn no_seat_receives_a_hidden_id_over_a_whole_proof_game() {
    let seed = 21;
    // The omniscient spectator is Seat 2, so its events show which ids are hidden.
    let mut setup: Value = serde_json::from_slice(&proof_setup_request(2, seed)).unwrap();
    setup["body"]["spectators"] = json!([{ "omniscient": true }]);
    let mut session = create(&serde_json::to_vec(&setup).unwrap());
    let mut bot = Bot::new(seed).recording(&[2]);
    let outcome = bot.play(&mut session, ANSWER_LIMIT).expect("the game finishes");
    assert_eq!(outcome.rejected, 0);

    let mut library = HashSet::new();
    let mut hands = [HashSet::new(), HashSet::new()];
    for event in bot.events(2).iter().filter(|e| e["kind"] == "zone_change") {
        // `previous` is the id in the zone left and `object` the id in the zone entered.
        for (id_field, zone_field) in [("previous", "from"), ("object", "to")] {
            let Some(id) = event.get(id_field).and_then(Value::as_str) else { continue };
            if event[zone_field] == "library" {
                library.insert(id.to_string());
            }
            if event[zone_field] == "hand" {
                hands[event["owner"].as_u64().unwrap() as usize].insert(id.to_string());
            }
        }
    }
    assert!(!library.is_empty() && !hands[0].is_empty() && !hands[1].is_empty());

    for seat in [0u32, 1] {
        let seen = ids_in(bot.events(seat));
        assert!(seen.is_disjoint(&library), "seat {seat} received a library id");
        let other = 1 - seat as usize;
        assert!(seen.is_disjoint(&hands[other]), "seat {seat} received an id from player {other}'s hand");
    }
}
