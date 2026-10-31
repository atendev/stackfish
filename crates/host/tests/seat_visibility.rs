//! Per-Seat visibility over bytes. Seats 0 and 1 are players, Seat 2 is a spectator and
//! Seat 3 is an omniscient spectator.

use std::collections::HashSet;

use serde_json::{Value, json};
use stackfish_engine::{CardTypeSet, ManaCost};
use stackfish_host::{FixtureCardSource, Session};
use stackfish_testutil::minimal_card_definition;

const OMNISCIENT: u32 = 3;

fn cards() -> FixtureCardSource {
    let card = |id: u64, name: &str, types: CardTypeSet| {
        minimal_card_definition(
            id,
            name,
            name,
            ManaCost {
                symbols: Vec::new(),
            },
            types,
        )
    };
    FixtureCardSource::new()
        .with("bear", card(1, "Bear", CardTypeSet::CREATURE))
        .with("land", card(2, "Land", CardTypeSet::LAND))
}

// Alternating cards, so any seven-card hand holds both kinds.
fn deck() -> Value {
    let entries: Vec<Value> = (0..12)
        .flat_map(|_| {
            [
                json!({ "card": "bear", "count": 1 }),
                json!({ "card": "land", "count": 1 }),
            ]
        })
        .collect();
    json!({ "deck": entries })
}

fn setup(overrides: Value) -> Value {
    json!({
        "players": [deck(), deck()],
        "spectators": [{}, { "omniscient": true }],
        "format": "standard",
        "overrides": overrides,
        "seed": "7",
    })
}

fn quick() -> Value {
    json!({ "shuffle_libraries": false, "skip_mulligan": true })
}

fn parse(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).expect("a response is JSON")
}

fn send(session: &mut Session, request: Value) -> Value {
    parse(&session.handle(&serde_json::to_vec(&request).unwrap()))
}

fn body(response: &Value) -> &Value {
    &response["body"]
}

fn create(setup: Value) -> Session {
    let request = serde_json::to_vec(&json!({ "v": 1, "kind": "setup", "body": setup })).unwrap();
    let (session, response) = Session::create(&request, &cards()).expect("setup is valid");
    assert_eq!(parse(&response)["body"]["seats"], json!([0, 1, 2, 3]));
    session
}

struct Table {
    session: Session,
    seen: [Vec<Value>; 4],
}

impl Table {
    fn new(setup: Value) -> Table {
        Table {
            session: create(setup),
            seen: Default::default(),
        }
    }

    fn sync(&mut self) -> Value {
        let mut decision = Value::Null;
        for seat in 0..4u32 {
            let response = send(
                &mut self.session,
                json!({ "v": 1, "kind": "poll", "seat": seat, "body": {} }),
            );
            self.seen[seat as usize].extend(body(&response)["events"].as_array().unwrap().clone());
            if seat == OMNISCIENT {
                decision = body(&response)["decision"].clone();
            }
        }
        decision
    }

    fn advance(&mut self) -> Value {
        let response = send(
            &mut self.session,
            json!({ "v": 1, "kind": "advance", "body": {} }),
        );
        assert_eq!(
            body(&response)["events"],
            json!([]),
            "a request without a seat receives no events"
        );
        self.sync()
    }

    fn answer(&mut self, decision: &Value, reply: Value) -> Value {
        let seat = decision["player"].clone();
        let response = send(
            &mut self.session,
            json!({ "v": 1, "kind": "answer", "seat": seat, "body": { "decision": decision["id"], "reply": reply } }),
        );
        assert!(body(&response).get("error").is_none(), "{response}");
        let seat = seat.as_u64().unwrap() as usize;
        self.seen[seat].extend(body(&response)["events"].as_array().unwrap().clone());
        self.sync()
    }

    fn pass(&mut self, decision: &Value) -> Value {
        self.answer(
            decision,
            json!({ "kind": "action", "action": { "kind": "pass_priority" } }),
        )
    }

    fn play(&mut self, decision: &Value, kind: &str) -> Value {
        let entry = decision["entries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["kind"] == kind)
            .cloned();
        match (kind, entry) {
            ("play_land", Some(e)) => self.answer(
                decision,
                json!({ "kind": "action", "action": { "kind": "play_land", "card": e["card"] } }),
            ),
            ("cast", Some(e)) => {
                let cast = json!({ "kind": "cast_spell", "card": e["card"] });
                self.answer(decision, json!({ "kind": "action", "action": cast }))
            }
            _ => self.pass(decision),
        }
    }

    fn bot(&mut self, mut decision: Value, answers: usize) {
        for _ in 0..answers {
            decision = match decision["shape"].as_str() {
                Some("action") => {
                    let kinds: Vec<&str> = decision["entries"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|e| e["kind"].as_str().unwrap())
                        .collect();
                    let kind = ["play_land", "cast"]
                        .into_iter()
                        .find(|k| kinds.contains(k))
                        .unwrap_or("pass");
                    self.play(&decision, kind)
                }
                Some("payment") => self.answer(
                    &decision,
                    json!({ "kind": "payment", "plan": { "payments": [] } }),
                ),
                Some("yes_no") => {
                    self.answer(&decision, json!({ "kind": "yes_no", "value": false }))
                }
                Some("pick") => {
                    let count = decision["min"].as_u64().unwrap() as usize;
                    let picks: Vec<Value> =
                        decision["candidates"].as_array().unwrap()[..count].to_vec();
                    self.answer(&decision, json!({ "kind": "pick", "picks": picks }))
                }
                Some("declare_attackers") => self.answer(
                    &decision,
                    json!({ "kind": "declare_attackers", "attackers": [] }),
                ),
                Some("declare_blockers") => self.answer(
                    &decision,
                    json!({ "kind": "declare_blockers", "blocks": [] }),
                ),
                _ => return,
            };
        }
    }

    fn events(&self, seat: u32) -> &[Value] {
        &self.seen[seat as usize]
    }

    fn hidden_moves(&self, seat: u32) -> Vec<&Value> {
        self.events(seat)
            .iter()
            .filter(|e| e["kind"] == "zone_change")
            .filter(|e| {
                ["hand", "library"]
                    .iter()
                    .any(|z| e["from"] == *z || e["to"] == *z)
            })
            .collect()
    }
}

// The ledger's own numbers are not object ids.
fn ids_in(events: &[Value]) -> HashSet<String> {
    fn walk(value: &Value, key: &str, found: &mut HashSet<String>) {
        const NOT_IDS: [&str; 6] = [
            "event_id",
            "sequence_no",
            "caused_by_command",
            "parent_event",
            "original_event",
            "value",
        ];
        match value {
            Value::String(text)
                if !NOT_IDS.contains(&key)
                    && !text.is_empty()
                    && text.bytes().all(|b| b.is_ascii_digit()) =>
            {
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

fn field<'a>(event: &'a Value, name: &str) -> Option<&'a Value> {
    event.get(name)
}

#[test]
fn a_draw_shows_the_opponent_a_move_with_no_ids_and_no_identity() {
    let mut table = Table::new(setup(quick()));
    table.advance();

    let draws = |seat: u32, owner: u64| -> Vec<Value> {
        table
            .hidden_moves(seat)
            .into_iter()
            .filter(|e| e["from"] == "library" && e["to"] == "hand" && e["owner"] == owner)
            .cloned()
            .collect()
    };
    assert_eq!(draws(0, 0).len(), 7, "the opening hand is seven moves");
    for draw in draws(0, 0) {
        assert!(
            field(&draw, "object").is_some() && field(&draw, "identity").is_some(),
            "the owner sees its card: {draw}"
        );
        assert!(
            field(&draw, "previous").is_none(),
            "no Seat but an omniscient one receives a library id: {draw}"
        );
    }
    for seat in [1, 2] {
        assert_eq!(
            draws(seat, 0).len(),
            7,
            "seat {seat} still counts the draws"
        );
        for draw in draws(seat, 0) {
            for hidden in ["object", "previous", "identity", "last_known_info"] {
                assert!(
                    field(&draw, hidden).is_none(),
                    "seat {seat} must not see {hidden}: {draw}"
                );
            }
        }
    }
    for draw in draws(OMNISCIENT, 0) {
        assert!(
            ["object", "previous", "identity"]
                .iter()
                .all(|f| field(&draw, f).is_some()),
            "{draw}"
        );
    }
}

#[test]
fn a_cast_shows_the_opponent_the_new_id_and_the_identity_but_not_the_hand_id() {
    let mut table = Table::new(setup(quick()));
    let mut decision = table.advance();
    while !decision["entries"]
        .as_array()
        .is_some_and(|e| e.iter().any(|e| e["kind"] == "cast"))
    {
        decision = table.pass(&decision);
        assert!(
            !decision.is_null(),
            "the game ended before a cast was possible"
        );
    }
    let player = decision["player"].as_u64().unwrap() as u32;
    let other = 1 - player;
    let spell = decision["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["kind"] == "cast")
        .unwrap()["card"]
        .clone();
    let next = table.play(&decision, "cast");
    if next["shape"] == "payment" {
        table.answer(
            &next,
            json!({ "kind": "payment", "plan": { "payments": [] } }),
        );
    }

    let cast = |seat: u32| -> Value {
        table
            .events(seat)
            .iter()
            .find(|e| e["kind"] == "zone_change" && e["from"] == "hand" && e["to"] == "stack")
            .unwrap_or_else(|| panic!("seat {seat} saw no cast"))
            .clone()
    };
    let own = cast(player);
    assert_eq!(
        own["previous"], spell,
        "the caster sees the id the card had in its hand"
    );
    let opponent = cast(other);
    assert!(
        field(&opponent, "object").is_some(),
        "the stack is public: {opponent}"
    );
    assert_eq!(opponent["identity"]["name"], own["identity"]["name"]);
    assert!(
        field(&opponent, "previous").is_none(),
        "the hand id stays hidden: {opponent}"
    );
    assert!(
        field(&opponent, "last_known_info").is_none(),
        "the snapshot names the hand id: {opponent}"
    );
    assert_eq!(opponent["owner"], player);
    assert_eq!(cast(OMNISCIENT)["previous"], spell);
}

#[test]
fn no_seat_ever_receives_a_hidden_id() {
    let mut table = Table::new(setup(json!({ "skip_mulligan": true })));
    let decision = table.advance();
    table.bot(decision, 120);
    assert!(
        table.events(OMNISCIENT).len() > 100,
        "the game ran for a while"
    );

    let mut library = HashSet::new();
    let mut hands = [HashSet::new(), HashSet::new()];
    for event in table
        .events(OMNISCIENT)
        .iter()
        .filter(|e| e["kind"] == "zone_change")
    {
        // `previous` is the id in the zone left and `object` the id in the zone entered.
        for (id_field, zone_field) in [("previous", "from"), ("object", "to")] {
            let Some(id) = event.get(id_field).and_then(Value::as_str) else {
                continue;
            };
            if event[zone_field] == "library" {
                library.insert(id.to_string());
            }
            if event[zone_field] == "hand" {
                hands[event["owner"].as_u64().unwrap() as usize].insert(id.to_string());
            }
        }
    }
    assert!(!library.is_empty() && !hands[0].is_empty() && !hands[1].is_empty());

    for seat in [0u32, 1, 2] {
        let seen = ids_in(table.events(seat));
        assert!(
            seen.is_disjoint(&library),
            "seat {seat} received a library id"
        );
        for owner in 0..2 {
            if seat != owner {
                assert!(
                    seen.is_disjoint(&hands[owner as usize]),
                    "seat {seat} received an id from player {owner}'s hand"
                );
            }
        }
    }
    let own = ids_in(table.events(0));
    assert!(
        !own.is_disjoint(&hands[0]),
        "a player does receive the ids in its own hand"
    );
}

#[test]
fn a_spectator_sees_no_hand_and_the_omniscient_seat_sees_rng_draws() {
    let mut table = Table::new(setup(json!({ "skip_mulligan": true })));
    table.advance();
    let rng = |seat: u32| {
        table
            .events(seat)
            .iter()
            .filter(|e| e["kind"] == "rng_drawn")
            .count()
    };
    assert!(
        rng(OMNISCIENT) > 0,
        "shuffling draws from the seeded stream"
    );
    assert_eq!(
        [rng(0), rng(1), rng(2)],
        [0, 0, 0],
        "ticket 004: RngDrawn is replay-only"
    );

    let shuffles = |seat: u32| {
        table
            .events(seat)
            .iter()
            .filter(|e| e["kind"] == "library_shuffled")
            .count()
    };
    assert!(
        shuffles(2) > 0 && shuffles(0) == shuffles(2),
        "a shuffle is public and reveals no order"
    );

    let spectator_ids = ids_in(table.events(2));
    let omniscient_ids = ids_in(table.events(OMNISCIENT));
    assert!(
        spectator_ids.len() < omniscient_ids.len(),
        "the spectator receives fewer ids than the omniscient Seat"
    );
}

#[test]
fn a_decision_is_opaque_to_everyone_but_its_chooser_and_an_omniscient_seat() {
    let mut table = Table::new(setup(quick()));
    table.advance();
    let view = |table: &mut Table, seat: u32| {
        let response = send(
            &mut table.session,
            json!({ "v": 1, "kind": "poll", "seat": seat, "body": {} }),
        );
        body(&response)["decision"].clone()
    };
    let chooser = view(&mut table, 0);
    assert_eq!(chooser["shape"], "action");
    assert!(!chooser["entries"].as_array().unwrap().is_empty());
    assert_eq!(
        view(&mut table, OMNISCIENT),
        chooser,
        "an omniscient Seat sees the full Decision"
    );

    for seat in [1, 2] {
        let seen = view(&mut table, seat);
        assert_eq!(seen["shape"], "opaque", "seat {seat}");
        assert_eq!(seen["reply"], "action");
        assert_eq!(seen["count"], chooser["entries"].as_array().unwrap().len());
        assert_eq!(
            (&seen["id"], &seen["player"]),
            (&chooser["id"], &chooser["player"])
        );
        assert!(
            seen.get("entries").is_none(),
            "the entries name cards in a hand"
        );
    }
}

#[test]
fn a_bottom_choice_is_opaque_to_the_opponent_and_its_hand_ids_stay_hidden() {
    let mut table = Table::new(setup(json!({ "shuffle_libraries": false })));
    let mut decision = table.advance();
    decision = table.answer(&decision, json!({ "kind": "yes_no", "value": true }));
    while decision["shape"] == "yes_no" {
        decision = table.answer(&decision, json!({ "kind": "yes_no", "value": false }));
    }
    assert_eq!(
        decision["shape"], "pick",
        "a paid mulligan asks for a bottom card"
    );

    let view = |table: &mut Table, seat: u32| {
        let response = send(
            &mut table.session,
            json!({ "v": 1, "kind": "poll", "seat": seat, "body": {} }),
        );
        body(&response)["decision"].clone()
    };
    let own = view(&mut table, 0);
    assert_eq!(own["candidates"].as_array().unwrap().len(), 7);
    assert!(
        own.get("identities").is_none(),
        "the chooser learned its hand from the draws"
    );

    let opponent = view(&mut table, 1);
    assert_eq!(
        (&opponent["shape"], &opponent["reply"], &opponent["count"]),
        (&json!("opaque"), &json!("pick"), &json!(7))
    );
    assert!(
        opponent.get("candidates").is_none(),
        "hand candidates are not in a public zone"
    );
}

#[test]
fn spectators_cannot_answer_and_players_cannot_be_omniscient() {
    let mut table = Table::new(setup(quick()));
    let decision = table.advance();
    let reply = json!({ "kind": "action", "action": { "kind": "pass_priority" } });
    for seat in [2, OMNISCIENT] {
        let response = send(
            &mut table.session,
            json!({ "v": 1, "kind": "answer", "seat": seat, "body": { "decision": decision["id"], "reply": reply } }),
        );
        assert_eq!(
            body(&response)["error"]["code"],
            "not_your_decision",
            "seat {seat}"
        );
    }

    let mut players_with_flag = setup(quick());
    players_with_flag["players"][0]["omniscient"] = json!(true);
    let request =
        serde_json::to_vec(&json!({ "v": 1, "kind": "setup", "body": players_with_flag })).unwrap();
    let Err(response) = Session::create(&request, &cards()) else {
        panic!("a player Seat has no omniscient flag")
    };
    assert_eq!(
        parse(&response)["body"]["error"]["code"],
        "malformed_message"
    );
}

#[test]
fn poll_hands_a_seat_what_it_missed_and_needs_a_seat() {
    let mut table = Table::new(setup(quick()));
    table.advance();
    let again = send(
        &mut table.session,
        json!({ "v": 1, "kind": "poll", "seat": 1, "body": {} }),
    );
    assert_eq!(
        body(&again)["events"],
        json!([]),
        "the first sync already drained Seat 1"
    );
    assert_eq!(body(&again)["status"], "decision_open");

    let no_seat = send(
        &mut table.session,
        json!({ "v": 1, "kind": "poll", "body": {} }),
    );
    assert_eq!(body(&no_seat)["error"]["code"], "malformed_message");
    let bad_seat = send(
        &mut table.session,
        json!({ "v": 1, "kind": "poll", "seat": 9, "body": {} }),
    );
    assert_eq!(body(&bad_seat)["error"]["code"], "unknown_seat");
    let extra = send(
        &mut table.session,
        json!({ "v": 1, "kind": "poll", "seat": 0, "body": { "since": 1 } }),
    );
    assert_eq!(body(&extra)["error"]["code"], "malformed_message");
}

#[test]
fn a_seat_that_did_not_ask_gets_the_events_of_an_answer_on_its_next_poll() {
    let mut table = Table::new(setup(quick()));
    let decision = table.advance();
    let before = table.events(1).len();
    // Seat 0 answers directly, so Seat 1 has not polled since.
    let response = send(
        &mut table.session,
        json!({
            "v": 1, "kind": "answer", "seat": 0,
            "body": { "decision": decision["id"], "reply": { "kind": "action", "action": { "kind": "pass_priority" } } },
        }),
    );
    let own = body(&response)["events"].as_array().unwrap().len();
    assert!(
        own > 0,
        "the answering Seat gets the events in its response"
    );
    let polled = send(
        &mut table.session,
        json!({ "v": 1, "kind": "poll", "seat": 1, "body": {} }),
    );
    let missed = body(&polled)["events"].as_array().unwrap();
    assert_eq!(
        missed.len(),
        own,
        "Seat 1 receives the same public events on its next poll"
    );
    assert_eq!(table.events(1).len(), before);
}

#[test]
fn the_game_over_state_is_still_pollable() {
    let mut table = Table::new(setup(
        json!({ "shuffle_libraries": false, "skip_mulligan": true, "starting_life": 0 }),
    ));
    let response = send(
        &mut table.session,
        json!({ "v": 1, "kind": "advance", "seat": 0, "body": {} }),
    );
    assert_eq!(body(&response)["status"], "game_over");
    let polled = send(
        &mut table.session,
        json!({ "v": 1, "kind": "poll", "seat": 1, "body": {} }),
    );
    assert_eq!(body(&polled)["status"], "game_over");
    assert!(
        !body(&polled)["events"].as_array().unwrap().is_empty(),
        "Seat 1 collects its events after the end"
    );
}
