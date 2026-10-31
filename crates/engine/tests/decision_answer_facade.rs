//! The Decision/Answer facade (`Engine::open_decision`, `Engine::answer`) drives `Engine::new_game` through the mulligan, bottom-card choice and first priority windows using only Decisions and Answers.

use stackfish_engine::decision::{Answer, Candidate, DecisionId, RejectionCode, Reply, ReplyShape};
use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::*;

fn plain_card_def(name: &str) -> CardDefinition {
    CardDefinition {
        definition_id: 0,
        name: name.to_string(),
        mana_cost: ManaCost {
            symbols: Vec::new(),
        },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::CREATURE,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey(name.to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn two_player_game() -> Engine {
    let player = |id: u32, cards: usize| PlayerConfig {
        player_id: PlayerId(id),
        library: (0..cards)
            .map(|i| plain_card_def(&format!("p{id}_card_{i}")))
            .collect(),
        commanders: Vec::new(),
        life_override: None,
    };
    Engine::new_game(GameConfig {
        players: vec![player(0, 14), player(1, 14)],
        turn_order: vec![PlayerId(0), PlayerId(1)],
        settings: GameSettings::default(),
        rng_seed: 42,
    })
}

fn run(engine: &mut Engine) -> StepStatus {
    loop {
        match engine.step().status {
            StepStatus::Progressed => continue,
            other => return other,
        }
    }
}

fn answer(decision: DecisionId, reply: Reply) -> Answer {
    Answer { decision, reply }
}

fn hand(engine: &Engine, player: PlayerId) -> Vec<ObjectId> {
    engine
        .state()
        .zone(ZoneKind::Hand, Some(player))
        .unwrap()
        .contents
        .iter()
        .copied()
        .collect()
}

#[test]
fn no_decision_is_open_until_the_first_step_runs_pregame() {
    let mut engine = two_player_game();
    assert!(engine.open_decision().is_none());
    run(&mut engine);
    let decision = engine.open_decision().expect("the first mulligan decision");
    assert_eq!(decision.id, DecisionId(1));
    assert!(matches!(decision.shape, ReplyShape::YesNo));
}

#[test]
fn a_mulligan_and_bottom_choice_play_through_as_decisions() {
    let mut engine = two_player_game();
    let p0 = PlayerId(0);
    run(&mut engine);

    // P0 takes one mulligan, then everyone keeps.
    let first = engine.open_decision().unwrap();
    assert_eq!(first.player, p0);
    engine
        .answer(p0, answer(first.id, Reply::YesNo(true)))
        .unwrap();
    run(&mut engine);

    let mut last_id = first.id;
    let bottom = loop {
        let decision = engine
            .open_decision()
            .expect("a Decision stays open until the opening hands settle");
        assert!(decision.id > last_id, "each new Decision gets a larger id");
        last_id = decision.id;
        match decision.shape {
            ReplyShape::YesNo => {
                engine
                    .answer(decision.player, answer(decision.id, Reply::YesNo(false)))
                    .unwrap();
                run(&mut engine);
            }
            ReplyShape::Pick { .. } => break decision,
            other => panic!("unexpected shape during the mulligan: {other:?}"),
        }
    };

    // One paid mulligan means one card goes to the bottom.
    assert_eq!(bottom.player, p0);
    let ReplyShape::Pick {
        candidates,
        min,
        max,
    } = &bottom.shape
    else {
        unreachable!()
    };
    let in_hand = hand(&engine, p0);
    assert_eq!(candidates.len(), in_hand.len());
    assert_eq!((*min, *max), (1, 1));

    // The wrong seat, a card outside the hand and an empty reply all fail,
    // and the hand does not change.
    let card = Candidate::Target(Target::Object(in_hand[0]));
    let stranger = Candidate::Target(Target::Object(ObjectId(9999)));
    let attempts = [
        (
            PlayerId(1),
            Reply::Pick(vec![card]),
            RejectionCode::WrongSeat,
        ),
        (
            p0,
            Reply::Pick(vec![stranger]),
            RejectionCode::InvalidChoice,
        ),
        (p0, Reply::Pick(vec![]), RejectionCode::InvalidChoice),
        (p0, Reply::YesNo(true), RejectionCode::WrongAnswerShape),
    ];
    for (player, reply, code) in attempts {
        let rejection = engine
            .answer(player, answer(bottom.id, reply))
            .expect_err("should be rejected");
        assert_eq!(rejection.code, code, "{}", rejection.message);
    }
    assert_eq!(hand(&engine, p0), in_hand);
    assert_eq!(
        engine.open_decision().unwrap().id,
        bottom.id,
        "a rejection leaves the same Decision open"
    );

    engine
        .answer(p0, answer(bottom.id, Reply::Pick(vec![card])))
        .unwrap();
    assert_eq!(hand(&engine, p0).len(), in_hand.len() - 1);
    let stale = engine.answer(p0, answer(bottom.id, Reply::Pick(vec![card])));
    assert!(stale.is_err(), "the answered Decision is closed");
}

#[test]
fn priority_decisions_alternate_and_pass_priority_advances_the_game() {
    let mut engine = Engine::new_game(GameConfig {
        players: vec![
            PlayerConfig {
                player_id: PlayerId(0),
                library: (0..14).map(|i| plain_card_def(&format!("a{i}"))).collect(),
                commanders: Vec::new(),
                life_override: None,
            },
            PlayerConfig {
                player_id: PlayerId(1),
                library: (0..14).map(|i| plain_card_def(&format!("b{i}"))).collect(),
                commanders: Vec::new(),
                life_override: None,
            },
        ],
        turn_order: vec![PlayerId(0), PlayerId(1)],
        settings: GameSettings {
            skip_mulligan: true,
            ..GameSettings::default()
        },
        rng_seed: 7,
    });
    run(&mut engine);

    let first = engine.open_decision().expect("a priority Decision");
    assert!(matches!(first.shape, ReplyShape::Action { .. }));
    let pass = PlayerAction::new(first.player, PlayerActionKind::PassPriority);
    engine
        .answer(first.player, answer(first.id, Reply::Action(pass)))
        .unwrap();
    assert!(
        engine.open_decision().is_none(),
        "the engine is running again until it is stepped"
    );

    run(&mut engine);
    let second = engine.open_decision().expect("the next priority Decision");
    assert!(second.id > first.id);
}
