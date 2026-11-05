//! An exported replay loads into a Session that reproduces the recorded game, and a replay
//! whose card data, engine build or Answers no longer fit is refused.

use std::cell::Cell;

use serde_json::{Value, json};
use stackfish_engine::{BEHAVIOUR_REV, CardDefinition};
use stackfish_host::{
    Bot, CardSource, CardSourceError, FixtureCardSource, Replay, Session, proof_cards,
    proof_setup_request,
};

const ANSWER_LIMIT: usize = 50_000;
const WATCHER: u32 = 2;

// Seat 2 is an omniscient spectator nobody polls, so its outbox keeps the whole game. The
// cap is raised so the comparison never rests on how long a game runs.
fn watched_setup(seed: u64) -> Vec<u8> {
    let mut setup: Value = serde_json::from_slice(&proof_setup_request(2, seed)).unwrap();
    setup["body"]["spectators"] = json!([{ "omniscient": true }]);
    setup["body"]["outbox_cap"] = json!(u64::MAX);
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

fn export(session: &mut Session) -> Value {
    let body = send(
        session,
        json!({ "v": 1, "kind": "export_replay", "body": {} }),
    );
    assert_eq!(body["status"], "exported", "{body}");
    body["replay"].clone()
}

fn load(replay: &Value, cards: &dyn CardSource) -> Result<Session, Value> {
    Session::from_replay(&serde_json::to_vec(replay).unwrap(), cards)
        .map(|(session, _)| session)
        .map_err(|e| serde_json::from_slice::<Value>(&e).unwrap()["body"].clone())
}

fn played(seed: u64) -> Session {
    let mut session = create(&watched_setup(seed));
    let outcome = Bot::new(seed)
        .play(&mut session, ANSWER_LIMIT)
        .unwrap_or_else(|e| panic!("seed {seed}: {e}"));
    assert_eq!(outcome.rejected, 0);
    session
}

struct Counting<'a> {
    inner: &'a dyn CardSource,
    loads: Cell<usize>,
}

impl CardSource for Counting<'_> {
    fn load(&self, key: &str) -> Result<CardDefinition, CardSourceError> {
        self.loads.set(self.loads.get() + 1);
        self.inner.load(key)
    }

    fn fingerprint(&self, key: &str) -> Result<String, CardSourceError> {
        self.inner.fingerprint(key)
    }
}

fn first_keys(replay: &Value, n: usize) -> Vec<String> {
    replay["cards"]
        .as_object()
        .unwrap()
        .keys()
        .take(n)
        .cloned()
        .collect()
}

fn renamed(keys: &[String]) -> FixtureCardSource {
    let mut cards = proof_cards();
    for key in keys {
        let mut card = cards.load(key).unwrap();
        card.name.push_str(" (errata)");
        cards.insert(key.clone(), card);
    }
    cards
}

#[test]
fn a_replayed_game_gives_the_same_omniscient_events_and_the_same_end() {
    for seed in [1, 7] {
        let mut original = played(seed);
        let answers = export(&mut original)["answers"].clone();
        let late = send(
            &mut original,
            json!({ "v": 1, "kind": "answer", "seat": 0,
                    "body": { "decision": "1", "reply": { "kind": "yes_no", "value": true } } }),
        );
        assert_eq!(late["error"]["code"], "game_over");
        let replay = export(&mut original);
        assert_eq!(
            replay["answers"], answers,
            "an Answer after the end is not recorded"
        );
        let mut loaded = load(&replay, &proof_cards()).unwrap_or_else(|e| panic!("{e}"));

        let before = poll(&mut original, WATCHER);
        let after = poll(&mut loaded, WATCHER);
        assert_eq!(after["status"], "game_over");
        assert_eq!(before, after, "seed {seed}");
        assert_eq!(
            export(&mut loaded),
            replay,
            "seed {seed}: re-export differs"
        );
    }
}

#[test]
fn a_replay_taken_with_a_decision_open_loads_with_the_same_decision_open() {
    let seed = 3;
    let mut original = create(&watched_setup(seed));
    let mut bot = Bot::new(seed);
    for _ in 0..25 {
        bot.step(&mut original).unwrap();
    }
    let replay = export(&mut original);
    let mut loaded = load(&replay, &proof_cards()).unwrap();
    let before = poll(&mut original, WATCHER);
    assert_eq!(before["status"], "decision_open");
    assert_eq!(poll(&mut loaded, WATCHER), before);

    // The original's next Answer fits the loaded Session too.
    bot.step(&mut original).unwrap();
    let next = serde_json::to_vec(&export(&mut original)).unwrap();
    let next = Replay::parse(&next).unwrap();
    let request = next.answer_request(next.answer_count() - 1).unwrap();
    let answered: Value = serde_json::from_slice(&loaded.handle(&request)).unwrap();
    assert!(answered["body"].get("error").is_none(), "{answered}");
}

#[test]
fn a_replay_exported_mid_run_loads_at_the_next_decision() {
    let mut original = create(&watched_setup(4));
    let ran = send(
        &mut original,
        json!({ "v": 1, "kind": "advance", "body": { "budget": 1 } }),
    );
    assert_eq!(ran["status"], "still_running", "{ran}");
    let replay = export(&mut original);
    let mut loaded = load(&replay, &proof_cards()).unwrap();

    let reached = send(
        &mut original,
        json!({ "v": 1, "kind": "advance", "body": {} }),
    );
    assert_eq!(reached["status"], "decision_open");
    assert_eq!(poll(&mut loaded, WATCHER), poll(&mut original, WATCHER));
}

#[test]
fn refused_answers_and_advance_budgets_are_not_recorded() {
    let seed = 5;
    let mut session = create(&watched_setup(seed));
    let outcome = Bot::new(seed)
        .injecting_illegal_answers()
        .play(&mut session, ANSWER_LIMIT)
        .unwrap();
    assert!(outcome.illegal_rejected > 0);
    let replay = export(&mut session);
    assert_eq!(replay["answers"].as_array().unwrap().len(), outcome.answers);
    assert!(load(&replay, &proof_cards()).is_ok());
}

#[test]
fn answers_the_session_refuses_are_not_recorded() {
    let mut session = create(&watched_setup(6));
    let opened = send(
        &mut session,
        json!({ "v": 1, "kind": "advance", "body": {} }),
    );
    assert_eq!(opened["status"], "decision_open");
    let chooser = opened["decision"]["player"].as_u64().unwrap();
    let id = opened["decision"]["id"].clone();
    for (seat, decision, code) in [
        (1 - chooser, id.clone(), "not_your_decision"),
        (u64::from(WATCHER), id, "not_your_decision"),
        (chooser, json!("999999"), "stale_decision"),
    ] {
        let refused = send(
            &mut session,
            json!({ "v": 1, "kind": "answer", "seat": seat,
                    "body": { "decision": decision, "reply": { "kind": "yes_no", "value": true } } }),
        );
        assert_eq!(refused["error"]["code"], code, "{refused}");
    }
    assert_eq!(export(&mut session)["answers"], json!([]));
}

#[test]
fn a_replay_whose_card_list_differs_from_its_setup_is_refused_naming_the_key() {
    let replay = export(&mut played(1));
    let mut extra = replay.clone();
    extra["cards"]["not-in-the-setup"] = json!("0");
    let mut missing = replay.clone();
    let key = first_keys(&replay, 1).remove(0);
    missing["cards"].as_object_mut().unwrap().remove(&key);
    for (tampered, named) in [(extra, "not-in-the-setup".to_string()), (missing, key)] {
        let refused = load(&tampered, &proof_cards()).err().unwrap();
        assert_eq!(refused["error"]["code"], "setup_invalid");
        assert!(
            refused["error"]["message"]
                .as_str()
                .unwrap()
                .contains(&named),
            "{refused}"
        );
    }
}

#[test]
fn the_setup_is_kept_byte_for_byte() {
    let key = first_keys(&export(&mut create(&watched_setup(1))), 1).remove(0);
    let setup = format!(
        r#"{{ "kind":"setup",  "v":1, "body":{{"seed":"2","format":"standard","players":[{{"deck":[{{"card":"{key}","count":1}}]}},{{"deck":[]}}]}} }}"#
    );
    let (mut session, _) = Session::create(setup.as_bytes(), &proof_cards()).unwrap();
    let replay = export(&mut session);
    assert_eq!(replay["setup"], setup.as_str());
    assert_eq!(replay["answers"], json!([]));
}

#[test]
fn changed_card_data_is_refused_naming_every_changed_key_before_any_card_loads() {
    let replay = export(&mut played(1));
    for n in [1, 2] {
        let keys = first_keys(&replay, n);
        let changed = renamed(&keys);
        let counting = Counting {
            inner: &changed,
            loads: Cell::new(0),
        };
        let refused = load(&replay, &counting).err().unwrap();
        assert_eq!(refused["error"]["code"], "setup_invalid");
        let message = refused["error"]["message"].as_str().unwrap();
        assert_eq!(message.matches(", ").count(), n - 1, "{message}");
        for key in &keys {
            assert!(message.contains(key.as_str()), "{message}");
        }
        assert_eq!(counting.loads.get(), 0);
    }
}

#[test]
fn a_replay_from_another_engine_build_is_refused_before_any_card_loads() {
    let mut replay = export(&mut played(1));
    replay["engine_build"] = json!("stackfish 0.0.0 rev 0");
    let cards = proof_cards();
    let counting = Counting {
        inner: &cards,
        loads: Cell::new(0),
    };
    let refused = load(&replay, &counting).err().unwrap();
    assert_eq!(refused["error"]["code"], "setup_invalid");
    assert!(
        refused["error"]["message"]
            .as_str()
            .unwrap()
            .contains("rev 0")
    );
    assert_eq!(counting.loads.get(), 0);
}

#[test]
fn a_dropped_answer_or_a_wrong_seat_is_refused_naming_the_answer() {
    let replay = export(&mut played(1));
    assert!(replay["answers"].as_array().unwrap().len() >= 2);

    let mut dropped = replay.clone();
    dropped["answers"].as_array_mut().unwrap().remove(0);
    let mut wrong_seat = replay.clone();
    let seat = wrong_seat["answers"][0]["seat"].as_u64().unwrap();
    wrong_seat["answers"][0]["seat"] = json!(1 - seat);

    for tampered in [dropped, wrong_seat] {
        let refused = load(&tampered, &proof_cards()).err().unwrap();
        assert_eq!(refused["error"]["code"], "setup_invalid");
        assert!(
            refused["error"]["message"]
                .as_str()
                .unwrap()
                .contains("answer 0"),
            "{refused}"
        );
    }
}

#[test]
fn a_started_replay_can_be_stepped_one_recorded_answer_at_a_time() {
    let exported = serde_json::to_vec(&export(&mut played(2))).unwrap();
    let replay = Replay::parse(&exported).unwrap();
    let (mut session, _) = replay.start(&proof_cards()).unwrap();
    for i in 0..replay.answer_count().min(10) {
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
        let answered: Value =
            serde_json::from_slice(&session.handle(&replay.answer_request(i).unwrap())).unwrap();
        assert!(
            answered["body"].get("error").is_none(),
            "answer {i}: {answered}"
        );
    }
}

#[test]
fn export_replay_comes_from_the_host_with_an_empty_body() {
    let mut session = create(&watched_setup(1));
    let seated = send(
        &mut session,
        json!({ "v": 1, "kind": "export_replay", "seat": 0, "body": {} }),
    );
    assert_eq!(seated["error"]["code"], "malformed_message");
    let bodied = send(
        &mut session,
        json!({ "v": 1, "kind": "export_replay", "body": { "x": 1 } }),
    );
    assert_eq!(bodied["error"]["code"], "malformed_message");
    let unknown = send(
        &mut session,
        json!({ "v": 1, "kind": "export_replay", "seat": 9, "body": {} }),
    );
    assert_eq!(unknown["error"]["code"], "unknown_seat");
}

fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xCBF2_9CE4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01B3)
    })
}

// A change to how the proof game plays out must come with a new build id, or old replays
// would load and play differently. The omniscient Seat sees every event.
#[test]
fn the_build_id_revision_moves_with_the_proof_game() {
    let events = poll(&mut played(9), WATCHER)["events"].clone();
    let hash = fnv(&serde_json::to_vec(&events).unwrap());
    assert_eq!(
        (BEHAVIOUR_REV, hash),
        (7, 8_689_343_603_236_921_321),
        "the proof game changed: bump BEHAVIOUR_REV in crates/engine/src/lib.rs and pin the new hash"
    );
}
