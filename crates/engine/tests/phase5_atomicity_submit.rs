//! A rejected answer to a pause changes nothing. The first test runs every `submit_*` and `decline_*` method against an engine not waiting for it.
//! The rest give a real pause a bad answer. Scry, surveil, search, attach and ordering pauses get theirs from the proof-game test in `crates/host`.

use std::collections::HashMap;

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::replacement::ReplacementCandidate;
use stackfish_engine::*;
use stackfish_testutil::{
    assert_unchanged_after, card_object, drain, minimal_card_definition, player_state, two_player_state, zones,
};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

fn def(id: u64, name: &str, types: CardTypeSet) -> CardDefinition {
    minimal_card_definition(id, name, name, ManaCost { symbols: Vec::new() }, types)
}

fn base(pool: ManaPool, kinds: &[ZoneKind]) -> GameState {
    GameState {
        last_active_turn_number: HashMap::from([(P0, 1), (P1, 1)]),
        players: vec![PlayerState { mana_pool: pool, ..player_state(P0) }, player_state(P1)],
        ..two_player_state(zones(P0, P1, kinds))
    }
}

fn put(state: &mut GameState, kind: ZoneKind, owner: Option<PlayerId>, id: u64, definition: CardDefinition) -> ObjectId {
    let id = ObjectId(id);
    state.objects.insert(id, card_object(id, definition, P0, kind));
    state.zone_mut(kind, owner).unwrap().push_bottom(id);
    id
}

fn act(player: PlayerId, kind: PlayerActionKind) -> PlayerAction {
    PlayerAction { player, kind }
}

fn pass_both_and_drain(engine: &mut Engine) -> StepStatus {
    engine.submit_action(act(P0, PlayerActionKind::PassPriority)).unwrap();
    drain(engine);
    engine.submit_action(act(P1, PlayerActionKind::PassPriority)).unwrap();
    drain(engine)
}

fn red(n: usize) -> Vec<ManaUnit> {
    vec![ManaUnit::Colored(Color::Red); n]
}

fn pay(units: Vec<ManaUnit>) -> PaymentPlan {
    PaymentPlan { payments: vec![CostPayment::Mana { units, phyrexian_life: 0 }] }
}

type Row = (&'static str, Box<dyn Fn(&mut Engine) -> bool>);

macro_rules! row {
    ($name:expr, |$e:ident| $call:expr) => {
        ($name, Box::new(move |$e: &mut Engine| $call.is_err()) as Box<dyn Fn(&mut Engine) -> bool>)
    };
}

fn rows() -> Vec<Row> {
    let card = ObjectId(1);
    vec![
        row!("submit_action (pregame action)", |e| e.submit_action(act(P0, PlayerActionKind::KeepHand))),
        row!("submit_payment", |e| e.submit_payment(pay(red(1)))),
        row!("unwind_payment", |e| e.unwind_payment()),
        row!("submit_scry_choice", |e| e.submit_scry_choice(vec![card], Vec::new())),
        row!("submit_surveil_choice", |e| e.submit_surveil_choice(vec![card], Vec::new())),
        row!("submit_search_choice", |e| e.submit_search_choice(vec![card])),
        row!("submit_entry_attach_choice", |e| e.submit_entry_attach_choice(Target::Object(card))),
        row!("submit_search_devour_choice", |e| e.submit_search_devour_choice(vec![card])),
        row!("submit_search_amplify_choice", |e| e.submit_search_amplify_choice(vec![card])),
        row!("submit_mulligan_decision", |e| e.submit_mulligan_decision(act(P0, PlayerActionKind::KeepHand))),
        row!("submit_bottom_cards", |e| e
            .submit_bottom_cards(act(P0, PlayerActionKind::BottomCards { cards: vec![card] }))),
        row!("submit_trigger_order", |e| e.submit_trigger_order(Vec::new())),
        row!("submit_trigger_targets", |e| e.submit_trigger_targets(vec![Target::Player(P1)])),
        row!("submit_trigger_mode_choice", |e| e.submit_trigger_mode_choice(vec![0])),
        row!("submit_discard", |e| e.submit_discard(vec![card])),
        row!("submit_attach_choice", |e| e.submit_attach_choice(card)),
        row!("submit_token_attach_choice", |e| e.submit_token_attach_choice(Target::Object(card))),
        row!("submit_script_choice", |e| e.submit_script_choice(Target::Object(card))),
        row!("submit_script_may_choice", |e| e.submit_script_may_choice(true)),
        row!("submit_devour_choice", |e| e.submit_devour_choice(vec![card])),
        row!("submit_amplify_choice", |e| e.submit_amplify_choice(vec![card])),
        row!("decline_madness_cast", |e| e.decline_madness_cast()),
        row!("submit_madness_cast", |e| e.submit_madness_cast(Vec::new(), Vec::new(), None)),
        row!("decline_cascade_cast", |e| e.decline_cascade_cast()),
        row!("submit_cascade_cast", |e| e.submit_cascade_cast(Vec::new(), Vec::new(), None)),
        row!("submit_unless_paid_payment", |e| e.submit_unless_paid_payment(P0, pay(red(1)))),
        row!("decline_unless_paid", |e| e.decline_unless_paid(P0)),
        row!("submit_discard_choice", |e| e.submit_discard_choice(P0, card)),
        row!("submit_mode_choice", |e| e.submit_mode_choice(vec![0])),
        row!("submit_copy_choice", |e| e.submit_copy_choice(Some(card))),
        row!("submit_replacement_choice", |e| e.submit_replacement_choice(ReplacementCandidate::Registered(card))),
        row!("submit_legend_rule_choice", |e| e.submit_legend_rule_choice(card)),
        row!("submit_commander_return_choice", |e| e.submit_commander_return_choice(true)),
    ]
}

#[test]
fn every_submit_method_rejects_an_answer_nobody_asked_for_and_changes_nothing() {
    let rows = rows();
    assert!(rows.len() >= 29, "one row per submit_* method (28 besides submit_action), plus declines");
    for (name, call) in rows {
        let mut state = base(ManaPool::default(), &[ZoneKind::Hand, ZoneKind::Battlefield, ZoneKind::Library]);
        put(&mut state, ZoneKind::Hand, Some(P0), 1, def(1, "Card", CardTypeSet::INSTANT));
        let mut engine = Engine::new(state);
        let rejected = assert_unchanged_after(&mut engine, name, |e| call(e));
        assert!(rejected, "{name} accepted an answer nobody asked for");
    }
}

#[test]
fn the_same_methods_reject_while_the_engine_waits_on_a_different_pause() {
    let mut state = base(ManaPool::default(), &[ZoneKind::Hand, ZoneKind::Battlefield, ZoneKind::Library]);
    let spell = put(
        &mut state,
        ZoneKind::Hand,
        Some(P0),
        1,
        CardDefinition {
            mana_cost: ManaCost { symbols: vec![ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::Red))] },
            ..def(1, "Spark", CardTypeSet::INSTANT)
        },
    );
    let mut engine = Engine::new(state);
    engine
        .submit_action(act(
            P0,
            PlayerActionKind::CastSpell {
                card: spell,
                modes: Vec::new(),
                targets: Vec::new(),
                x_value: None,
                alternative_cost: None,
                pay_buyback: false,
            },
        ))
        .unwrap();
    assert!(matches!(drain(&mut engine), StepStatus::AwaitingPayment { .. }));
    for (name, call) in rows() {
        // The pause being answered is the payment itself, so skip its own rows.
        if matches!(name, "submit_payment" | "unwind_payment") {
            continue;
        }
        let rejected = assert_unchanged_after(&mut engine, name, |e| call(e));
        assert!(rejected, "{name} accepted an answer while a payment was pending");
    }
}

fn paying_engine() -> Engine {
    let mut state = base(ManaPool { red: 2, ..ManaPool::default() }, &[ZoneKind::Hand, ZoneKind::Battlefield]);
    let spell = put(
        &mut state,
        ZoneKind::Hand,
        Some(P0),
        1,
        CardDefinition {
            mana_cost: ManaCost {
                symbols: vec![
                    ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::Red)),
                    ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::Red)),
                ],
            },
            ..def(1, "Double Spark", CardTypeSet::INSTANT)
        },
    );
    let mut engine = Engine::new(state);
    engine
        .submit_action(act(
            P0,
            PlayerActionKind::CastSpell {
                card: spell,
                modes: Vec::new(),
                targets: Vec::new(),
                x_value: None,
                alternative_cost: None,
                pay_buyback: false,
            },
        ))
        .unwrap();
    assert!(matches!(drain(&mut engine), StepStatus::AwaitingPayment { .. }));
    engine
}

#[test]
fn a_payment_plan_that_does_not_cover_the_cost_changes_nothing() {
    let mut engine = paying_engine();
    let plans = [
        ("empty plan", PaymentPlan { payments: Vec::new() }),
        ("one of two", pay(red(1))),
        ("too much", pay(red(3))),
        ("wrong color", pay(vec![ManaUnit::Colored(Color::Green); 2])),
        ("mana not in the pool", pay(vec![ManaUnit::Colored(Color::Red), ManaUnit::Colorless])),
    ];
    for (name, plan) in plans {
        let result = assert_unchanged_after(&mut engine, name, |e| e.submit_payment(plan));
        assert!(result.is_err(), "{name} was accepted");
    }
    assert_eq!(engine.state().player(P0).unwrap().mana_pool.red, 2, "no mana left the pool");
    engine.submit_payment(pay(red(2))).expect("the right plan still works");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == P0), "{status:?}");
}

#[test]
fn a_bad_discard_changes_nothing() {
    let mut state = GameState {
        turn_number: 2,
        phase: Phase::Ending,
        step: Step::End,
        last_active_turn_number: HashMap::from([(P0, 2), (P1, 1)]),
        ..base(ManaPool::default(), &[ZoneKind::Hand, ZoneKind::Battlefield, ZoneKind::Graveyard])
    };
    let hand: Vec<ObjectId> = (0..8).map(|i| put(&mut state, ZoneKind::Hand, Some(P0), 100 + i, def(100 + i, "Filler", CardTypeSet::CREATURE))).collect();
    let stranger = put(&mut state, ZoneKind::Battlefield, None, 200, def(200, "Bear", CardTypeSet::CREATURE));
    let mut engine = Engine::new(state);
    assert!(matches!(drain(&mut engine), StepStatus::AwaitingAction { player } if player == P0));
    engine.submit_action(act(P0, PlayerActionKind::PassPriority)).unwrap();
    drain(&mut engine);
    engine.submit_action(act(P1, PlayerActionKind::PassPriority)).unwrap();
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingDiscard { player, hand_size: 8, max_hand_size: 7 } if player == P0), "{status:?}");

    let answers: Vec<(&str, Vec<ObjectId>)> = vec![
        ("none", Vec::new()),
        ("two", vec![hand[0], hand[1]]),
        ("a duplicate", vec![hand[0], hand[0]]),
        ("a card not in hand", vec![stranger]),
        ("a card that does not exist", vec![ObjectId(777)]),
    ];
    for (name, cards) in answers {
        let result = assert_unchanged_after(&mut engine, name, |e| e.submit_discard(cards));
        assert!(result.is_err(), "discarding {name} was accepted");
    }
    engine.submit_discard(vec![hand[0]]).expect("one card is the right answer");
}

fn madness_bolt() -> CardDefinition {
    CardDefinition {
        mana_cost: ManaCost {
            symbols: vec![
                ManaSymbol::Atom(AtomicManaSymbol::Generic(1)),
                ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::Red)),
            ],
        },
        alternative_costs: vec![AlternativeCost::Replacement {
            description: "Madness {R}".to_string(),
            cost: CostSpec::mana_only(ManaCost { symbols: vec![ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::Red))] }),
            from_zone: ZoneKind::Exile,
            exile_on_leaving_stack: false,
            escape: false,
        }],
        ..def(1, "Madness Bolt", CardTypeSet::SORCERY)
    }
}

fn this_card_only() -> EffectFilter {
    EffectFilter {
        controller: None,
        card_types: None,
        self_only: true,
        subtypes: None,
        excluded_subtypes: None,
        supertypes: None,
        excluded_supertypes: None,
    }
}

fn madness_engine() -> Engine {
    let mut state = GameState {
        turn_number: 2,
        phase: Phase::Ending,
        step: Step::End,
        last_active_turn_number: HashMap::from([(P0, 2), (P1, 1)]),
        ..base(ManaPool::default(), &[ZoneKind::Library, ZoneKind::Hand, ZoneKind::Graveyard, ZoneKind::Battlefield, ZoneKind::Exile])
    };
    let card = put(&mut state, ZoneKind::Hand, Some(P0), 1, madness_bolt());
    state.replacement_effects.push(ReplacementEffect {
        source: card,
        spec: ReplacementSpec::Primitive {
            filter: EventFilter::WouldMoveZone {
                from: Some(ZoneKind::Hand),
                to: Some(ZoneKind::Graveyard),
                affected: this_card_only(),
            },
            payload: ReplacementPayload::ReplaceZoneDestination(ZoneKind::Exile),
        },
        consumption: ConsumptionPolicy::Repeatable,
    });
    state.abilities.push(RegisteredAbility {
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
        id: AbilityId(1),
        source: card,
        kind: AbilityKind::Triggered,
        condition: Some(ConditionSpec::Primitive(ConditionPrimitive::MadnessExiled)),
        cost: None,
        target: None,
        operation: ScriptHandle(0),
        effect: Some(AbilityEffect::MadnessCastOrDiscard),
        intervening_if: None,
    });
    for i in 0..7 {
        put(&mut state, ZoneKind::Hand, Some(P0), 100 + i, def(100 + i, "Filler", CardTypeSet::CREATURE));
    }
    let mut engine = Engine::new(state);
    assert!(matches!(drain(&mut engine), StepStatus::AwaitingAction { .. }));
    engine.submit_action(act(P0, PlayerActionKind::PassPriority)).unwrap();
    drain(&mut engine);
    engine.submit_action(act(P1, PlayerActionKind::PassPriority)).unwrap();
    assert!(matches!(drain(&mut engine), StepStatus::AwaitingDiscard { .. }));
    engine.submit_discard(vec![card]).unwrap();
    drain(&mut engine);
    let status = pass_both_and_drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingMadnessChoice { .. }), "{status:?}");
    engine
}

// An illegal Madness cast must not use up a command id.
#[test]
fn an_illegal_madness_cast_keeps_the_pause_and_the_command_id() {
    let mut engine = madness_engine();
    let bad_casts: Vec<(&str, Vec<u32>, Option<u32>)> = vec![("modes on a plain card", vec![0], None)];
    for (name, modes, x) in bad_casts {
        let result = assert_unchanged_after(&mut engine, name, |e| e.submit_madness_cast(modes, Vec::new(), x));
        assert!(result.is_err(), "{name} was accepted");
    }
    let result = assert_unchanged_after(&mut engine, "a target the card cannot take", |e| {
        e.submit_madness_cast(Vec::new(), vec![Target::Player(P1)], None)
    });
    assert!(result.is_err());
    engine.submit_madness_cast(Vec::new(), Vec::new(), None).expect("the legal cast still works");
}

fn cascade_beast() -> CardDefinition {
    CardDefinition {
        mana_cost: ManaCost {
            symbols: vec![
                ManaSymbol::Atom(AtomicManaSymbol::Generic(3)),
                ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::Red)),
            ],
        },
        power: Some(PTValue::Fixed(5)),
        toughness: Some(PTValue::Fixed(5)),
        ..def(1, "Cascade Beast", CardTypeSet::CREATURE)
    }
}

fn cascade_engine() -> Engine {
    let mut state = base(
        ManaPool { red: 1, colorless: 3, ..ManaPool::default() },
        &[ZoneKind::Hand, ZoneKind::Library, ZoneKind::Battlefield, ZoneKind::Exile, ZoneKind::Graveyard],
    );
    let beast = put(&mut state, ZoneKind::Hand, Some(P0), 0, cascade_beast());
    put(
        &mut state,
        ZoneKind::Library,
        Some(P0),
        13,
        CardDefinition {
            mana_cost: ManaCost { symbols: vec![ManaSymbol::Atom(AtomicManaSymbol::Generic(2))] },
            power: Some(PTValue::Fixed(1)),
            toughness: Some(PTValue::Fixed(1)),
            ..def(13, "Candidate", CardTypeSet::CREATURE)
        },
    );
    // The cast takes the first id the fixture hands out.
    state.abilities.push(RegisteredAbility {
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
        id: AbilityId(1),
        source: ObjectId(1000),
        kind: AbilityKind::Triggered,
        condition: Some(ConditionSpec::Primitive(ConditionPrimitive::Cast)),
        cost: None,
        target: None,
        operation: ScriptHandle(0),
        effect: Some(AbilityEffect::Cascade),
        intervening_if: None,
    });
    let mut engine = Engine::new(state);
    engine
        .submit_action(act(
            P0,
            PlayerActionKind::CastSpell {
                card: beast,
                modes: Vec::new(),
                targets: Vec::new(),
                x_value: None,
                alternative_cost: None,
                pay_buyback: false,
            },
        ))
        .unwrap();
    assert!(matches!(drain(&mut engine), StepStatus::AwaitingPayment { .. }));
    engine
        .submit_payment(pay(vec![
            ManaUnit::Colored(Color::Red),
            ManaUnit::Colorless,
            ManaUnit::Colorless,
            ManaUnit::Colorless,
        ]))
        .unwrap();
    drain(&mut engine);
    let status = pass_both_and_drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingCascadeChoice { candidate: Some(_), .. }), "{status:?}");
    engine
}

// An illegal Cascade cast must not use up a command id.
#[test]
fn an_illegal_cascade_cast_keeps_the_pause_and_the_command_id() {
    let mut engine = cascade_engine();
    let result = assert_unchanged_after(&mut engine, "modes on a plain card", |e| {
        e.submit_cascade_cast(vec![0], Vec::new(), None)
    });
    assert!(result.is_err());
    let result = assert_unchanged_after(&mut engine, "a target the card cannot take", |e| {
        e.submit_cascade_cast(Vec::new(), vec![Target::Player(P1)], None)
    });
    assert!(result.is_err());
    engine.submit_cascade_cast(Vec::new(), Vec::new(), None).expect("the legal cast still works");
}
