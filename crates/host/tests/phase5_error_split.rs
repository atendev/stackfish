//! 035: a rejected Answer that gets past the facade's own checks is a normal
//! refusal, not a fatal error. The modal entries still list a pair of modes
//! that `combine_chosen_modes` cannot combine (TODO(036)), so the engine
//! refuses that cast in `submit_action`. The Session must answer with
//! `invalid_answer`, leave the same Decision open and keep playing.

use serde_json::{Value, json};
use stackfish_engine::{
    AbilityEffect, CardDefinition, CardTypeSet, ManaCost, ModeChoice, SpellMode,
};
use stackfish_host::{FixtureCardSource, Session};
use stackfish_testutil::minimal_card_definition;

/// "Choose two: you gain 1 life; or cascade." A mode with `Cascade` cannot join
/// another mode, so the pair is refused when the cast is announced.
fn twin_modes() -> CardDefinition {
    let mode = |effect| SpellMode { target_requirement: None, effect: Some(effect), additional_cost: None, pawprint_weight: 0 };
    CardDefinition {
        modes: vec![mode(AbilityEffect::GainLife(1)), mode(AbilityEffect::Cascade)],
        mode_choice: ModeChoice::Exactly(2),
        ..minimal_card_definition(1, "Twin Modes", "twin", ManaCost { symbols: Vec::new() }, CardTypeSet::INSTANT)
    }
}

fn session() -> Session {
    let cards = FixtureCardSource::new().with("twin", twin_modes());
    let deck = json!({ "deck": [{ "card": "twin", "count": 20 }] });
    let setup = json!({
        "v": 1, "kind": "setup",
        "body": {
            "players": [deck.clone(), deck], "format": "standard", "seed": "7",
            "overrides": { "shuffle_libraries": false, "skip_mulligan": true },
        },
    });
    let (session, _) = Session::create(&serde_json::to_vec(&setup).unwrap(), &cards).expect("setup is valid");
    session
}

fn send(session: &mut Session, request: Value) -> Value {
    serde_json::from_slice(&session.handle(&serde_json::to_vec(&request).unwrap())).expect("a response is JSON")
}

#[test]
fn a_cast_the_entries_list_but_the_engine_refuses_leaves_the_game_running() {
    let mut session = session();
    let opened = send(&mut session, json!({ "v": 1, "kind": "advance", "seat": 0, "body": {} }));
    let mut decision = opened["body"]["decision"].clone();
    if decision["shape"] == "opaque" {
        let chooser = decision["player"].as_u64().unwrap();
        let polled = send(&mut session, json!({ "v": 1, "kind": "poll", "seat": chooser, "body": {} }));
        decision = polled["body"]["decision"].clone();
    }
    assert_eq!(decision["shape"], "action", "{decision}");
    let seat = decision["player"].as_u64().unwrap();
    let cast = decision["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["kind"] == "cast")
        .unwrap_or_else(|| panic!("the instant is castable: {decision}"));
    assert_eq!(cast["modes"]["min"], 2, "the modal entry lists the card");
    let card = cast["card"].clone();

    let before = session.snapshot();
    let refused = send(
        &mut session,
        json!({
            "v": 1, "kind": "answer", "seat": seat,
            "body": {
                "decision": decision["id"],
                "reply": { "kind": "action", "action": { "kind": "cast_spell", "card": card, "modes": [0, 1] } },
            },
        }),
    );
    assert_eq!(refused["body"]["error"]["code"], "invalid_answer", "{refused}");
    assert_eq!(refused["body"]["status"], "decision_open", "the same Decision stays open");
    assert_eq!(refused["body"]["decision"]["id"], decision["id"]);
    let after = session.snapshot();
    assert!(before == after, "a refusal changes nothing");

    // The game goes on: the same Decision still takes a legal Answer.
    let passed = send(
        &mut session,
        json!({
            "v": 1, "kind": "answer", "seat": seat,
            "body": { "decision": decision["id"], "reply": { "kind": "action", "action": { "kind": "pass_priority" } } },
        }),
    );
    assert_eq!(passed["body"]["status"], "decision_open", "{passed}");
    assert!(session.internal_error().is_none(), "nothing broke");
}
