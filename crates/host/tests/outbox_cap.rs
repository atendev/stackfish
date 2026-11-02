//! A Seat whose outbox passes `outbox_cap` lapses: its events stop and its responses carry
//! `resync` until it sends `snapshot`. The cap never changes the game.

use serde_json::{Value, json};
use stackfish_host::{
    Bot, DEFAULT_OUTBOX_CAP, Progress, Replay, Session, proof_cards, proof_setup_request,
};

const SPECTATOR: u32 = 2;
const UNBOUNDED: u64 = u64::MAX;

fn setup(seed: u64, cap: u64) -> Vec<u8> {
    let mut setup: Value = serde_json::from_slice(&proof_setup_request(2, seed)).unwrap();
    setup["body"]["spectators"] = json!([{ "omniscient": true }]);
    setup["body"]["outbox_cap"] = json!(cap);
    serde_json::to_vec(&setup).unwrap()
}

fn create(setup: &[u8]) -> Session {
    Session::create(setup, &proof_cards())
        .unwrap_or_else(|e| panic!("{}", String::from_utf8_lossy(&e)))
        .0
}

fn send(session: &mut Session, request: Value) -> Value {
    let bytes = session.handle(&serde_json::to_vec(&request).unwrap());
    serde_json::from_slice::<Value>(&bytes).unwrap()["body"].clone()
}

fn poll(session: &mut Session, seat: u32) -> Value {
    send(
        session,
        json!({ "v": 1, "kind": "poll", "seat": seat, "body": {} }),
    )
}

fn snapshot(session: &mut Session, seat: u32) -> Value {
    send(
        session,
        json!({ "v": 1, "kind": "snapshot", "seat": seat, "body": {} }),
    )
}

fn events(body: &Value) -> &Vec<Value> {
    body["events"]
        .as_array()
        .expect("every response has events")
}

fn resync(body: &Value) -> bool {
    match body.get("resync") {
        None => false,
        Some(Value::Bool(true)) => true,
        Some(other) => panic!("resync is only ever true: {other}"),
    }
}

fn sequence_no_of(value: &Value) -> u64 {
    value.as_str().unwrap().parse().unwrap()
}

fn sequence_no(event: &Value) -> u64 {
    sequence_no_of(&event["sequence_no"])
}

fn step(bot: &mut Bot, session: &mut Session) {
    let progress = bot.step(session).unwrap();
    assert!(
        !matches!(progress, Progress::GameOver(_)),
        "the game ended too soon"
    );
}

fn assert_lapsed(body: &Value) {
    assert!(resync(body), "{body}");
    assert!(events(body).is_empty(), "{body}");
    assert_eq!(body["status"], "decision_open", "{body}");
    assert!(
        body["decision"].is_object(),
        "a lapsed Seat still sees the Decision"
    );
}

#[test]
fn a_spectator_that_stops_polling_lapses_until_a_snapshot_and_can_lapse_again() {
    let seed = 3;
    let steps = 95;
    // Every Seat is polled after every step here, which gives each step's events per Seat.
    let mut reference = create(&setup(seed, UNBOUNDED));
    let mut bot = Bot::new(seed);
    let mut per_step: Vec<[Value; 3]> = Vec::new();
    for _ in 0..steps {
        step(&mut bot, &mut reference);
        per_step.push([0, 1, 2].map(|seat| poll(&mut reference, seat)));
    }
    // The cap fits any one step of the Seats that poll, so only the spectator's gaps lapse it.
    let players = per_step.iter().flat_map(|p| &p[..2]);
    let polled_spectator = per_step[35..60].iter().map(|p| &p[2]);
    let cap = players
        .chain(polled_spectator)
        .map(|body| events(body).len())
        .max()
        .unwrap();
    let spectator_events = |range: std::ops::Range<usize>| -> usize {
        range.map(|k| events(&per_step[k][2]).len()).sum()
    };
    assert!(spectator_events(0..30) > cap && spectator_events(60..95) > cap);

    let mut session = create(&setup(seed, cap as u64));
    let mut bot = Bot::new(seed);
    let mut through = 0;
    for (k, expected) in per_step.iter().enumerate() {
        step(&mut bot, &mut session);
        for seat in [0, 1] {
            let body = poll(&mut session, seat);
            assert!(!resync(&body), "player seat {seat} lapsed at step {k}");
            assert_eq!(events(&body), events(&expected[seat as usize]), "step {k}");
        }
        match k {
            0..29 | 60..94 => {}
            29 => {
                assert_lapsed(&poll(&mut session, SPECTATOR));
                let refused = [
                    json!({ "v": 1, "kind": "advance", "seat": SPECTATOR, "body": {} }),
                    json!({ "v": 1, "kind": "answer", "seat": SPECTATOR,
                            "body": { "decision": "1", "reply": { "kind": "yes_no", "yes": true } } }),
                ];
                for request in refused {
                    let body = send(&mut session, request);
                    assert!(body["error"].is_object(), "{body}");
                    assert_lapsed(&body);
                }
            }
            30..34 => assert_lapsed(&poll(&mut session, SPECTATOR)),
            34 | 94 => {
                assert_lapsed(&poll(&mut session, SPECTATOR));
                let body = snapshot(&mut session, SPECTATOR);
                assert!(!resync(&body), "{body}");
                assert!(events(&body).is_empty() && body["snapshot"].is_object());
                through = sequence_no_of(&body["snapshot"]["through_sequence_no"]);
            }
            _ => {
                let body = poll(&mut session, SPECTATOR);
                assert!(!resync(&body), "step {k}: {body}");
                assert!(events(&body).len() <= cap);
                assert_eq!(events(&body), events(&expected[2]), "step {k}");
                if k == 35 {
                    assert!(!events(&body).is_empty());
                    assert!(events(&body).iter().all(|e| sequence_no(e) > through));
                }
            }
        }
    }
}

// Plays every recorded Answer through `handle`, so the cap applies throughout.
fn replayed_with_cap(replay: &Value, cap: u64) -> (Session, Vec<Value>) {
    let mut edited = replay.clone();
    let mut setup: Value = serde_json::from_str(replay["setup"].as_str().unwrap()).unwrap();
    setup["body"]["outbox_cap"] = json!(cap);
    edited["setup"] = json!(serde_json::to_string(&setup).unwrap());
    let replay = Replay::parse(&serde_json::to_vec(&edited).unwrap()).unwrap();
    let (mut session, _) = replay.start(&proof_cards()).unwrap();
    let mut answered = Vec::new();
    for i in 0..replay.answer_count() {
        let mut opened = send(
            &mut session,
            json!({ "v": 1, "kind": "advance", "body": {} }),
        );
        while opened["status"] == "still_running" {
            opened = send(
                &mut session,
                json!({ "v": 1, "kind": "advance", "body": {} }),
            );
        }
        assert_eq!(opened["status"], "decision_open", "answer {i}");
        let bytes = session.handle(&replay.answer_request(i).unwrap());
        let body = serde_json::from_slice::<Value>(&bytes).unwrap()["body"].clone();
        assert!(body.get("error").is_none(), "answer {i}: {body}");
        answered.push(body);
    }
    (session, answered)
}

#[test]
fn a_lapsed_player_still_answers_and_the_cap_does_not_change_the_game() {
    let seed = 5;
    let mut played = create(&setup(seed, DEFAULT_OUTBOX_CAP));
    let outcome = Bot::new(seed).play(&mut played, 50_000).unwrap();
    assert_eq!(outcome.rejected, 0);
    let exported = send(
        &mut played,
        json!({ "v": 1, "kind": "export_replay", "body": {} }),
    )["replay"]
        .clone();

    let (mut capped, answered) = replayed_with_cap(&exported, 1);
    assert!(answered.iter().all(|body| events(body).len() <= 1));
    assert!(
        answered.iter().any(resync),
        "a player Seat that only answers lapses under a cap of 1"
    );
    let last = answered.last().unwrap();
    assert_eq!(last["status"], "game_over");
    assert_eq!(last["winner"], json!(outcome.winner));

    let reexported = send(
        &mut capped,
        json!({ "v": 1, "kind": "export_replay", "body": {} }),
    )["replay"]
        .clone();
    assert_eq!(reexported["answers"], exported["answers"]);
    for seat in [0, 1, SPECTATOR] {
        let mut view = snapshot(&mut capped, seat);
        let mut expected = snapshot(&mut played, seat);
        assert!(!resync(&view));
        view.as_object_mut().unwrap().remove("events");
        expected.as_object_mut().unwrap().remove("events");
        assert_eq!(view, expected, "seat {seat}");
    }
}

#[test]
fn seats_that_keep_polling_never_lapse_under_the_default_cap() {
    let seed = 8;
    let [capped, unbounded] = [DEFAULT_OUTBOX_CAP, UNBOUNDED].map(|cap| {
        let mut session = create(&setup(seed, cap));
        let mut bot = Bot::new(seed).recording(&[SPECTATOR]);
        let outcome = bot.play(&mut session, 50_000).unwrap();
        (bot, outcome.winner, outcome.answers)
    });
    assert_eq!((capped.1, capped.2), (unbounded.1, unbounded.2));
    for seat in [0, 1, SPECTATOR] {
        assert!(!capped.0.events(seat).is_empty());
        assert_eq!(
            capped.0.events(seat),
            unbounded.0.events(seat),
            "seat {seat}"
        );
    }
}

#[test]
fn a_loaded_replay_keeps_the_whole_game_past_the_cap_until_the_next_event() {
    let seed = 4;
    let cap = 5;
    let mut original = create(&setup(seed, cap));
    let mut bot = Bot::new(seed);
    for _ in 0..40 {
        step(&mut bot, &mut original);
    }
    let replay = send(
        &mut original,
        json!({ "v": 1, "kind": "export_replay", "body": {} }),
    )["replay"]
        .clone();
    let bytes = serde_json::to_vec(&replay).unwrap();

    let (mut loaded, _) = Session::from_replay(&bytes, &proof_cards()).unwrap();
    let whole = poll(&mut loaded, SPECTATOR);
    assert!(!resync(&whole), "{whole}");
    assert!(events(&whole).len() > cap as usize);

    let (mut loaded, _) = Session::from_replay(&bytes, &proof_cards()).unwrap();
    step(&mut Bot::new(seed), &mut loaded);
    assert_lapsed(&poll(&mut loaded, SPECTATOR));
}
