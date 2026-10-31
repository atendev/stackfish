//! The host Session over bytes only, as a Godot adapter or a bot would drive it.

use serde_json::{Value, json};
use stackfish_engine::{CardTypeSet, ManaCost};
use stackfish_host::{FixtureCardSource, Session};
use stackfish_testutil::minimal_card_definition;

fn cards() -> FixtureCardSource {
    let creature = |id: u64, name: &str| {
        minimal_card_definition(
            id,
            name,
            name,
            ManaCost {
                symbols: Vec::new(),
            },
            CardTypeSet::CREATURE,
        )
    };
    FixtureCardSource::new()
        .with("card-a", creature(1, "Card A"))
        .with("card-b", creature(2, "Card B"))
}

fn deck(key: &str, count: u32) -> Value {
    json!({ "deck": [{ "card": key, "count": count }] })
}

fn setup_request(body: Value) -> Vec<u8> {
    serde_json::to_vec(&json!({ "v": 1, "kind": "setup", "body": body })).unwrap()
}

fn two_players() -> Value {
    json!([deck("card-a", 14), deck("card-b", 14)])
}

fn standard(overrides: Value) -> Value {
    json!({ "players": two_players(), "format": "standard", "overrides": overrides, "seed": "42" })
}

fn parse(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).expect("a response is JSON")
}

fn create(body: Value) -> Session {
    let (session, response) =
        Session::create(&setup_request(body), &cards()).expect("setup is valid");
    let response = parse(&response);
    assert_eq!(response["body"]["status"], "created");
    session
}

fn create_error(body: Value) -> Value {
    let Err(response) = Session::create(&setup_request(body), &cards()) else {
        panic!("setup should have failed");
    };
    parse(&response)
}

fn send(session: &mut Session, request: Value) -> Value {
    parse(&session.handle(&serde_json::to_vec(&request).unwrap()))
}

fn advance(session: &mut Session, budget: Option<u64>) -> Value {
    let body = budget.map_or(json!({}), |b| json!({ "budget": b }));
    send(
        session,
        json!({ "v": 1, "kind": "advance", "seat": 0, "body": body }),
    )
}

fn poll(session: &mut Session, seat: u32) -> Value {
    send(
        session,
        json!({ "v": 1, "kind": "poll", "seat": seat, "body": {} }),
    )
}

// A response shows the requesting Seat's view, so when someone else chooses next this polls as them.
fn chooser_view(session: &mut Session, response: &Value) -> Value {
    let decision = body(response)["decision"].clone();
    if decision["shape"] != "opaque" {
        return decision;
    }
    let player = decision["player"].as_u64().unwrap() as u32;
    let polled = poll(session, player);
    body(&polled)["decision"].clone()
}

fn answer(session: &mut Session, seat: u32, decision: &Value, reply: Value) -> Value {
    send(
        session,
        json!({ "v": 1, "kind": "answer", "seat": seat, "body": { "decision": decision, "reply": reply } }),
    )
}

fn body(response: &Value) -> &Value {
    &response["body"]
}

fn error_code(response: &Value) -> &str {
    body(response)["error"]["code"]
        .as_str()
        .expect("the response has an error")
}

// No shuffle and no mulligan, so the first Advance reaches a priority Decision.
fn quick_overrides() -> Value {
    json!({ "shuffle_libraries": false, "skip_mulligan": true })
}

#[test]
fn creation_fails_with_a_named_error_for_each_structural_problem() {
    let one_player = json!({ "players": [deck("card-a", 3)], "format": "standard", "seed": "1" });
    let response = create_error(one_player);
    assert_eq!(error_code(&response), "setup_invalid");
    assert!(
        body(&response)["error"]["message"]
            .as_str()
            .unwrap()
            .contains("at least 2 players")
    );

    let unknown_card = json!({ "players": [deck("card-a", 3), deck("nope", 3)], "format": "standard", "seed": "1" });
    let response = create_error(unknown_card);
    assert_eq!(error_code(&response), "setup_invalid");
    assert!(
        body(&response)["error"]["message"]
            .as_str()
            .unwrap()
            .contains("\"nope\""),
        "the message names the key"
    );

    let mut with_commander = two_players();
    with_commander[1]["commanders"] = json!(["card-b"]);
    let response =
        create_error(json!({ "players": with_commander, "format": "standard", "seed": "1" }));
    assert_eq!(error_code(&response), "setup_invalid");
    assert!(
        body(&response)["error"]["message"]
            .as_str()
            .unwrap()
            .contains("player 1")
    );

    let response =
        create_error(json!({ "players": two_players(), "format": "pauper", "seed": "1" }));
    assert_eq!(error_code(&response), "setup_invalid");
    assert!(
        body(&response)["error"]["message"]
            .as_str()
            .unwrap()
            .contains("\"pauper\"")
    );

    let response = create_error(
        json!({ "players": two_players(), "format": "standard", "seed": "1", "step_cap": 0 }),
    );
    assert_eq!(error_code(&response), "setup_invalid");
}

#[test]
fn creation_rejects_bad_envelopes_and_bodies() {
    let attempt = |request: Value| {
        let Err(response) = Session::create(&serde_json::to_vec(&request).unwrap(), &cards())
        else {
            panic!("setup should have failed");
        };
        parse(&response)
    };
    let valid_body = standard(json!({}));

    let wrong_version = attempt(json!({ "v": 2, "kind": "setup", "body": valid_body }));
    assert_eq!(error_code(&wrong_version), "unsupported_version");

    let wrong_kind = attempt(json!({ "v": 1, "kind": "advance", "body": {} }));
    assert_eq!(error_code(&wrong_kind), "malformed_message");

    let extra_field = attempt(json!({ "v": 1, "kind": "setup", "body": valid_body, "extra": 1 }));
    assert_eq!(error_code(&extra_field), "malformed_message");

    let mut unknown_setting = valid_body.clone();
    unknown_setting["overrides"] = json!({ "starting_lyfe": 5 });
    let response = attempt(json!({ "v": 1, "kind": "setup", "body": unknown_setting }));
    assert_eq!(error_code(&response), "malformed_message");

    // A seed that is a JSON number loses the decimal-string rule.
    let mut numeric_seed = valid_body.clone();
    numeric_seed["seed"] = json!(42);
    let response = attempt(json!({ "v": 1, "kind": "setup", "body": numeric_seed }));
    assert_eq!(error_code(&response), "malformed_message");

    let (_, bytes) = Session::create(
        &serde_json::to_vec(&json!({ "v": 1, "kind": "setup", "body": valid_body })).unwrap(),
        &cards(),
    )
    .expect("the same body is fine with a string seed");
    assert_eq!(parse(&bytes)["body"]["seats"], json!([0, 1]));

    assert!(Session::create(b"not json", &cards()).is_err());
}

#[test]
fn the_first_advance_opens_a_priority_decision_for_the_first_player() {
    let mut session = create(standard(quick_overrides()));
    let response = advance(&mut session, None);
    let body = body(&response);
    assert_eq!(body["status"], "decision_open");
    let decision = &body["decision"];
    assert_eq!(
        decision["id"], "1",
        "ids are decimal strings and start at 1"
    );
    assert_eq!(decision["player"], 0);
    assert_eq!(decision["shape"], "action");
    let entries = decision["entries"].as_array().unwrap();
    assert!(
        entries.iter().any(|e| e["kind"] == "pass"),
        "priority always offers pass"
    );

    let events = body["events"].as_array().unwrap();
    assert!(!events.is_empty());
    let numbers: Vec<u64> = events
        .iter()
        .map(|e| e["sequence_no"].as_str().unwrap().parse().unwrap())
        .collect();
    assert!(
        numbers.windows(2).all(|w| w[0] < w[1]),
        "events arrive in ledger order"
    );
    assert!(
        events.iter().all(|e| e["kind"].is_string()),
        "every event has a kind tag"
    );
    assert!(events.iter().any(|e| e["kind"] == "turn_started"));
}

#[test]
fn mulligan_decisions_come_in_turn_order_and_a_paid_mulligan_asks_for_a_bottom_card() {
    let mut session = create(standard(json!({ "shuffle_libraries": false })));
    let first = advance(&mut session, None);
    let decision = body(&first)["decision"].clone();
    assert_eq!(
        (decision["shape"].as_str(), decision["player"].as_u64()),
        (Some("yes_no"), Some(0))
    );

    // Player 0 takes a mulligan, then everyone keeps.
    let mut response = answer(
        &mut session,
        0,
        &decision["id"],
        json!({ "kind": "yes_no", "value": true }),
    );
    let mut order = vec![0];
    let bottom = loop {
        let decision = chooser_view(&mut session, &response);
        match decision["shape"].as_str().unwrap() {
            "yes_no" => {
                let player = decision["player"].as_u64().unwrap() as u32;
                order.push(player);
                response = answer(
                    &mut session,
                    player,
                    &decision["id"],
                    json!({ "kind": "yes_no", "value": false }),
                );
            }
            "pick" => break decision,
            other => panic!("unexpected shape {other}"),
        }
    };
    assert_eq!(
        order,
        vec![0, 1, 0],
        "the next player decides before the mulligan player sees the new hand"
    );
    assert_eq!(bottom["player"], 0);
    assert_eq!(
        (bottom["min"].as_u64(), bottom["max"].as_u64()),
        (Some(1), Some(1))
    );
    assert_eq!(bottom["candidates"].as_array().unwrap().len(), 7);
    assert_eq!(bottom["candidates"][0]["kind"], "object");
}

#[test]
fn a_rejected_answer_returns_the_same_decision_with_no_events() {
    let mut session = create(standard(json!({ "shuffle_libraries": false })));
    let opened = advance(&mut session, None);
    let decision = body(&opened)["decision"].clone();
    let id = decision["id"].clone();

    let attempts = [
        (
            "stale_decision",
            0,
            json!("99"),
            json!({ "kind": "yes_no", "value": true }),
        ),
        (
            "not_your_decision",
            1,
            id.clone(),
            json!({ "kind": "yes_no", "value": true }),
        ),
        (
            "invalid_answer",
            0,
            id.clone(),
            json!({ "kind": "pick", "picks": [{ "kind": "object", "id": "3" }] }),
        ),
    ];
    for (code, seat, decision_id, reply) in attempts {
        let response = answer(&mut session, seat, &decision_id, reply);
        assert_eq!(error_code(&response), code);
        assert_eq!(body(&response)["status"], "decision_open", "{code}");
        if seat == 0 {
            assert_eq!(
                body(&response)["decision"],
                decision,
                "{code}: the same Decision is still open"
            );
        } else {
            let seen = &body(&response)["decision"];
            assert_eq!(
                (&seen["id"], &seen["shape"]),
                (&id, &json!("opaque")),
                "{code}: the other Seat sees it opaque"
            );
        }
        assert_eq!(body(&response)["events"], json!([]), "{code}");
    }

    // The retry works, so nothing was disturbed.
    let response = answer(
        &mut session,
        0,
        &id,
        json!({ "kind": "yes_no", "value": false }),
    );
    assert_eq!(body(&response)["status"], "decision_open");
    assert_eq!(body(&response)["decision"]["id"], "2");
}

#[test]
fn a_bottom_choice_that_is_not_a_candidate_is_invalid() {
    let mut session = create(standard(json!({ "shuffle_libraries": false })));
    let opened = advance(&mut session, None);
    let first = body(&opened)["decision"]["id"].clone();
    let mut response = answer(
        &mut session,
        0,
        &first,
        json!({ "kind": "yes_no", "value": true }),
    );
    let bottom = loop {
        let decision = chooser_view(&mut session, &response);
        if decision["shape"] == "pick" {
            break decision;
        }
        let player = decision["player"].as_u64().unwrap() as u32;
        response = answer(
            &mut session,
            player,
            &decision["id"],
            json!({ "kind": "yes_no", "value": false }),
        );
    };
    let stranger = json!({ "kind": "pick", "picks": [{ "kind": "object", "id": "9999" }] });
    let rejected = answer(&mut session, 0, &bottom["id"], stranger);
    assert_eq!(error_code(&rejected), "invalid_answer");
    assert_eq!(body(&rejected)["decision"], bottom);

    let card = bottom["candidates"][0].clone();
    let accepted = answer(
        &mut session,
        0,
        &bottom["id"],
        json!({ "kind": "pick", "picks": [card] }),
    );
    assert_eq!(body(&accepted)["status"], "decision_open");
    assert_eq!(
        body(&accepted)["decision"]["shape"],
        "action",
        "everyone has kept, so the game starts"
    );
}

#[test]
fn requests_outside_their_window_are_refused() {
    let mut session = create(standard(quick_overrides()));
    let early = answer(
        &mut session,
        0,
        &json!("1"),
        json!({ "kind": "yes_no", "value": true }),
    );
    assert_eq!(error_code(&early), "no_open_decision");
    assert_eq!(body(&early)["status"], "error");

    let opened = advance(&mut session, None);
    let decision = body(&opened)["decision"].clone();

    let pending = advance(&mut session, None);
    assert_eq!(error_code(&pending), "decision_pending");
    assert_eq!(body(&pending)["decision"], decision);
    assert_eq!(body(&pending)["events"], json!([]));

    let bad_seat = send(
        &mut session,
        json!({ "v": 1, "kind": "advance", "seat": 5, "body": {} }),
    );
    assert_eq!(error_code(&bad_seat), "unknown_seat");

    let no_seat = send(
        &mut session,
        json!({ "v": 1, "kind": "answer", "body": { "decision": "1", "reply": { "kind": "yes_no", "value": true } } }),
    );
    assert_eq!(error_code(&no_seat), "malformed_message");

    let number_id = send(
        &mut session,
        json!({ "v": 1, "kind": "answer", "seat": 0, "body": { "decision": 1, "reply": { "kind": "yes_no", "value": true } } }),
    );
    assert_eq!(
        error_code(&number_id),
        "malformed_message",
        "a Decision id must be a decimal string"
    );

    let unknown_kind = send(&mut session, json!({ "v": 1, "kind": "dance", "body": {} }));
    assert_eq!(error_code(&unknown_kind), "malformed_message");

    let wrong_version = send(
        &mut session,
        json!({ "v": 9, "kind": "advance", "body": {} }),
    );
    assert_eq!(error_code(&wrong_version), "unsupported_version");

    let extra_field = send(
        &mut session,
        json!({ "v": 1, "kind": "advance", "body": { "budjet": 3 } }),
    );
    assert_eq!(error_code(&extra_field), "malformed_message");

    let zero = send(
        &mut session,
        json!({ "v": 1, "kind": "advance", "body": { "budget": 0 } }),
    );
    assert_eq!(error_code(&zero), "malformed_message");

    // None of those refusals disturbed the Decision.
    let still = answer(
        &mut session,
        0,
        &decision["id"],
        json!({ "kind": "action", "action": { "kind": "pass_priority" } }),
    );
    assert_eq!(body(&still)["status"], "decision_open");
}

#[test]
fn both_requests_are_errors_after_game_over() {
    // Both players start at 0 life, so the first state-based action check ends the game as a draw.
    let mut session = create(standard(
        json!({ "shuffle_libraries": false, "skip_mulligan": true, "starting_life": 0 }),
    ));
    let over = advance(&mut session, None);
    assert_eq!(body(&over)["status"], "game_over");
    assert_eq!(
        body(&over)["winner"],
        Value::Null,
        "a draw has a null winner"
    );
    assert!(!body(&over)["events"].as_array().unwrap().is_empty());

    let again = advance(&mut session, None);
    assert_eq!(error_code(&again), "game_over");
    assert_eq!(body(&again)["events"], json!([]));
    let answered = answer(
        &mut session,
        0,
        &json!("1"),
        json!({ "kind": "yes_no", "value": true }),
    );
    assert_eq!(error_code(&answered), "game_over");
}

#[test]
fn a_budget_slices_the_run_without_changing_the_result() {
    let setup = standard(json!({ "shuffle_libraries": true, "skip_mulligan": true }));
    let mut whole = create(setup.clone());
    let all_at_once = advance(&mut whole, None);
    assert_eq!(body(&all_at_once)["status"], "decision_open");

    let mut sliced = create(setup);
    let mut events = Vec::new();
    let mut calls = 0;
    let last = loop {
        let response = advance(&mut sliced, Some(1));
        calls += 1;
        events.extend(body(&response)["events"].as_array().unwrap().clone());
        match body(&response)["status"].as_str().unwrap() {
            "still_running" => assert!(body(&response)["decision"].is_null()),
            "decision_open" => break response,
            other => panic!("unexpected status {other}"),
        }
    };
    assert!(calls > 1, "a budget of 1 needs several calls");
    assert_eq!(events, *body(&all_at_once)["events"].as_array().unwrap());
    assert_eq!(body(&last)["decision"], body(&all_at_once)["decision"]);
}

#[test]
fn the_same_setup_and_answers_give_identical_bytes() {
    let run = || {
        let mut session = create(standard(json!({})));
        let mut transcript = vec![session.handle(
            &serde_json::to_vec(&json!({ "v": 1, "kind": "advance", "body": {} })).unwrap(),
        )];
        let mut response = parse(&transcript[0]);
        for _ in 0..6 {
            let decision = chooser_view(&mut session, &response);
            if decision.is_null() {
                break;
            }
            let player = decision["player"].as_u64().unwrap() as u32;
            let reply = match decision["shape"].as_str().unwrap() {
                "yes_no" => json!({ "kind": "yes_no", "value": false }),
                "action" => json!({ "kind": "action", "action": { "kind": "pass_priority" } }),
                "pick" => {
                    let count = decision["min"].as_u64().unwrap() as usize;
                    let picks: Vec<Value> =
                        decision["candidates"].as_array().unwrap()[..count].to_vec();
                    json!({ "kind": "pick", "picks": picks })
                }
                other => panic!("unexpected shape {other}"),
            };
            let request = json!({ "v": 1, "kind": "answer", "seat": player, "body": { "decision": decision["id"], "reply": reply } });
            let bytes = session.handle(&serde_json::to_vec(&request).unwrap());
            response = parse(&bytes);
            transcript.push(bytes);
        }
        transcript
    };
    let first = run();
    assert!(first.len() > 3, "the run passed through several Decisions");
    assert_eq!(first, run());
}

#[test]
fn passing_priority_walks_the_game_forward() {
    let mut session = create(standard(quick_overrides()));
    let opened = advance(&mut session, None);
    let mut decision = body(&opened)["decision"].clone();
    let mut seen_steps = Vec::new();
    for _ in 0..12 {
        if decision["shape"] != "action" {
            break;
        }
        let player = decision["player"].as_u64().unwrap() as u32;
        let response = answer(
            &mut session,
            player,
            &decision["id"],
            json!({ "kind": "action", "action": { "kind": "pass_priority" } }),
        );
        assert_ne!(body(&response)["status"], "error", "{response}");
        for event in body(&response)["events"].as_array().unwrap() {
            if event["kind"] == "step_started" {
                seen_steps.push(event["step"].as_str().unwrap().to_string());
            }
        }
        if body(&response)["status"] == "game_over" {
            break;
        }
        decision = chooser_view(&mut session, &response);
    }
    assert!(
        seen_steps.len() >= 2,
        "passing priority moves through the turn: {seen_steps:?}"
    );
}
