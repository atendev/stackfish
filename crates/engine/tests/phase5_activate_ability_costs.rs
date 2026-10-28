//! CR 602: activated-ability additional costs (`Sacrifice`, `Discard`, `TapPermanents`, `LifePayment`) validate and commit.
//! A combined `TapSelf` plus `Sacrifice` cost needs both parts. CR 118.3, 118.8, 118.10.

use std::collections::{HashMap, VecDeque};

use stackfish_engine::engine::{Engine, PaymentError, StepStatus};
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, single_player_state};

fn vanilla_definition(name: &str, card_types: CardTypeSet) -> CardDefinition {
    CardDefinition {
        definition_id: 1,
        name: name.to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: Some(PTValue::Fixed(1)),
        toughness: Some(PTValue::Fixed(1)),
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn object(id: ObjectId, definition: CardDefinition, owner: PlayerId, zone: ZoneKind) -> GameObject {
    card_object(id, definition, owner, zone)
}

fn state_with_cost(cost: CostSpec) -> (GameState, PlayerId, ObjectId, ObjectId, ObjectId, ObjectId) {
    let p0 = PlayerId(0);
    let source = ObjectId(0);
    let fodder = ObjectId(1);
    let hand_card = ObjectId(2);
    let tappable_a = ObjectId(3);
    let tappable_b = ObjectId(4);

    let mut objects = HashMap::new();
    objects.insert(
        source,
        object(source, vanilla_definition("Source", CardTypeSet::ARTIFACT), p0, ZoneKind::Battlefield),
    );
    objects.insert(
        fodder,
        object(fodder, vanilla_definition("Fodder", CardTypeSet::CREATURE), p0, ZoneKind::Battlefield),
    );
    objects.insert(
        hand_card,
        object(hand_card, vanilla_definition("Hand Card", CardTypeSet::CREATURE), p0, ZoneKind::Hand),
    );
    objects.insert(
        tappable_a,
        object(tappable_a, vanilla_definition("Tappable A", CardTypeSet::ARTIFACT), p0, ZoneKind::Battlefield),
    );
    objects.insert(
        tappable_b,
        object(tappable_b, vanilla_definition("Tappable B", CardTypeSet::ARTIFACT), p0, ZoneKind::Battlefield),
    );

    let state = GameState {
        objects,
        zones: vec![
            Zone {
                kind: ZoneKind::Battlefield,
                owner: None,
                contents: VecDeque::from([source, fodder, tappable_a, tappable_b]),
            },
            Zone { kind: ZoneKind::Hand, owner: Some(p0), contents: VecDeque::from([hand_card]) },
            Zone::new(ZoneKind::Graveyard, Some(p0)),
        ],
        abilities: vec![RegisteredAbility {
            modes: Vec::new(),
            mode_choice: ModeChoice::Exactly(1),
            id: AbilityId(0),
            source,
            kind: AbilityKind::Activated,
            condition: None,
            cost: Some(cost),
            target: None,
            operation: ScriptHandle(0),
            effect: Some(AbilityEffect::GainLife(1)),
            intervening_if: None,
        }],
        ..single_player_state(p0, Vec::new())
    };
    (state, p0, source, fodder, hand_card, tappable_a)
}


fn activate_and_await_payment(engine: &mut Engine, player: PlayerId, source: ObjectId) {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::ActivateAbility {
                source,
                ability_index: 0,
                targets: Vec::new(),
                chosen_colors: Vec::new(),
                x_value: None,
            },
        })
        .expect("player holds priority");
    let status = drain(engine);
    assert!(
        matches!(status, StepStatus::AwaitingPayment { .. }),
        "expected a pause for the ability's cost, got {status:?}"
    );
}

fn last_zone_change(engine: &Engine) -> (ObjectId, ZoneKind, ZoneKind) {
    for event in engine.state().ledger.iter().rev() {
        if let GameEventKind::ZoneChange { object, from, to, .. } = event.kind {
            return (object, from, to);
        }
    }
    panic!("expected a ZoneChange event in the ledger");
}

#[test]
fn sacrifice_cost_accepts_legal_payment() {
    let cost = CostSpec {
        mana: None,
        additional: vec![AdditionalCost::Sacrifice {
            filter: FilterSpec { card_types: Some(CardTypeSet::CREATURE), supertypes: None, subtypes: None, controller: None, random: false, extra: None },
        }],
        alternative: Vec::new(),
    };
    let (state, p0, source, fodder, _hand_card, _tappable_a) = state_with_cost(cost);
    let mut engine = Engine::new(state);

    activate_and_await_payment(&mut engine, p0, source);
    engine
        .submit_payment(PaymentPlan { payments: vec![CostPayment::Sacrifice(fodder)] })
        .expect("a legal Sacrifice(fodder) payment pays a Sacrifice{Creature} cost");
    drain(&mut engine);

    assert!(engine.state().object(fodder).is_none(), "the original fodder id is retired");
    let (new_id, from, to) = last_zone_change(&engine);
    assert_eq!((from, to), (ZoneKind::Battlefield, ZoneKind::Graveyard));
    assert!(engine.state().object(new_id).is_some(), "fodder now exists in the graveyard under a new id");
}

#[test]
fn sacrifice_cost_rejects_illegal_payment() {
    let cost = CostSpec {
        mana: None,
        additional: vec![AdditionalCost::Sacrifice {
            filter: FilterSpec { card_types: Some(CardTypeSet::CREATURE), supertypes: None, subtypes: None, controller: None, random: false, extra: None },
        }],
        alternative: Vec::new(),
    };
    let (state, p0, source, fodder, _hand_card, tappable_a) = state_with_cost(cost);
    let mut engine = Engine::new(state);

    activate_and_await_payment(&mut engine, p0, source);

    // tappable_a is an Artifact, not a Creature, so it fails the filter.
    let err = engine
        .submit_payment(PaymentPlan { payments: vec![CostPayment::Sacrifice(tappable_a)] })
        .unwrap_err();
    assert!(matches!(err, PaymentError::InvalidPlan));
    assert!(engine.state().object(tappable_a).is_some(), "no partial mutation - nothing sacrificed");
    assert!(engine.state().object(fodder).is_some(), "no partial mutation - fodder untouched");
}

#[test]
fn discard_cost_accepts_legal_payment() {
    let cost = CostSpec {
        mana: None,
        additional: vec![AdditionalCost::Discard {
            filter: FilterSpec { card_types: None, supertypes: None, subtypes: None, controller: None, random: false, extra: None },
        }],
        alternative: Vec::new(),
    };
    let (state, p0, source, _fodder, hand_card, _tappable_a) = state_with_cost(cost);
    let mut engine = Engine::new(state);

    activate_and_await_payment(&mut engine, p0, source);
    engine
        .submit_payment(PaymentPlan { payments: vec![CostPayment::Discard(hand_card)] })
        .expect("a legal Discard(hand_card) payment pays a Discard cost");
    drain(&mut engine);

    assert!(engine.state().object(hand_card).is_none(), "the original hand_card id is retired");
    let (new_id, from, to) = last_zone_change(&engine);
    assert_eq!((from, to), (ZoneKind::Hand, ZoneKind::Graveyard));
    assert!(engine.state().object(new_id).is_some(), "the card now exists in the graveyard under a new id");
}

#[test]
fn discard_cost_rejects_illegal_payment() {
    let cost = CostSpec {
        mana: None,
        additional: vec![AdditionalCost::Discard {
            filter: FilterSpec { card_types: None, supertypes: None, subtypes: None, controller: None, random: false, extra: None },
        }],
        alternative: Vec::new(),
    };
    let (state, p0, source, fodder, hand_card, _tappable_a) = state_with_cost(cost);
    let mut engine = Engine::new(state);

    activate_and_await_payment(&mut engine, p0, source);

    // fodder is on the battlefield, not in hand.
    let err = engine
        .submit_payment(PaymentPlan { payments: vec![CostPayment::Discard(fodder)] })
        .unwrap_err();
    assert!(matches!(err, PaymentError::InvalidPlan));
    assert!(engine.state().object(hand_card).is_some(), "no partial mutation - hand_card untouched");
}

#[test]
fn tap_permanents_cost_accepts_legal_payment() {
    let cost = CostSpec {
        mana: None,
        additional: vec![AdditionalCost::TapPermanents {
            filter: FilterSpec { card_types: Some(CardTypeSet::ARTIFACT), supertypes: None, subtypes: None, controller: None, random: false, extra: None },
            count: 2,
        }],
        alternative: Vec::new(),
    };
    let (state, p0, source, _fodder, _hand_card, tappable_a) = state_with_cost(cost);
    let tappable_b = ObjectId(4);
    let mut engine = Engine::new(state);

    activate_and_await_payment(&mut engine, p0, source);
    engine
        .submit_payment(PaymentPlan {
            payments: vec![CostPayment::Tap(tappable_a), CostPayment::Tap(tappable_b)],
        })
        .expect("two legal Tap payments pay a TapPermanents{count: 2} cost");
    drain(&mut engine);

    assert!(engine.state().object(tappable_a).unwrap().tapped);
    assert!(engine.state().object(tappable_b).unwrap().tapped);
}

#[test]
fn tap_permanents_cost_rejects_duplicate_id() {
    let cost = CostSpec {
        mana: None,
        additional: vec![AdditionalCost::TapPermanents {
            filter: FilterSpec { card_types: Some(CardTypeSet::ARTIFACT), supertypes: None, subtypes: None, controller: None, random: false, extra: None },
            count: 2,
        }],
        alternative: Vec::new(),
    };
    let (state, p0, source, _fodder, _hand_card, tappable_a) = state_with_cost(cost);
    let mut engine = Engine::new(state);

    activate_and_await_payment(&mut engine, p0, source);

    // The same object twice cannot satisfy a count of 2.
    let err = engine
        .submit_payment(PaymentPlan {
            payments: vec![CostPayment::Tap(tappable_a), CostPayment::Tap(tappable_a)],
        })
        .unwrap_err();
    assert!(matches!(err, PaymentError::InvalidPlan));
    assert!(!engine.state().object(tappable_a).unwrap().tapped, "no partial mutation");
}

#[test]
fn life_payment_cost_accepts_legal_payment() {
    let cost = CostSpec {
        mana: None,
        additional: vec![AdditionalCost::LifePayment { amount: 3 }],
        alternative: Vec::new(),
    };
    let (state, p0, source, ..) = state_with_cost(cost);
    let mut engine = Engine::new(state);

    activate_and_await_payment(&mut engine, p0, source);
    engine
        .submit_payment(PaymentPlan { payments: vec![CostPayment::Life(3)] })
        .expect("paying exactly the required amount pays a LifePayment{3} cost");
    drain(&mut engine);
    assert_eq!(engine.state().player(p0).unwrap().life, 17, "20 - 3 paid, ability not resolved yet");

    // Passing priority with a non-empty stack resolves the ability.
    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(&mut engine);
    assert_eq!(engine.state().player(p0).unwrap().life, 18, "17 + 1 gained from the resolved ability");
}

#[test]
fn life_payment_cost_rejects_insufficient_life() {
    let cost = CostSpec {
        mana: None,
        additional: vec![AdditionalCost::LifePayment { amount: 25 }],
        alternative: Vec::new(),
    };
    let (state, p0, source, ..) = state_with_cost(cost);
    let mut engine = Engine::new(state);

    activate_and_await_payment(&mut engine, p0, source);

    // Only 20 life is available, so paying 25 fails (CR 119.4).
    let err = engine
        .submit_payment(PaymentPlan { payments: vec![CostPayment::Life(25)] })
        .unwrap_err();
    assert!(matches!(err, PaymentError::InvalidPlan));
    assert_eq!(engine.state().player(p0).unwrap().life, 20, "no partial mutation");
}

#[test]
fn combined_tap_self_and_sacrifice_requires_both() {
    let cost = CostSpec {
        mana: None,
        additional: vec![
            AdditionalCost::TapSelf,
            AdditionalCost::Sacrifice {
                filter: FilterSpec { card_types: Some(CardTypeSet::CREATURE), supertypes: None, subtypes: None, controller: None, random: false, extra: None },
            },
        ],
        alternative: Vec::new(),
    };
    let (state, p0, source, fodder, _hand_card, _tappable_a) = state_with_cost(cost);
    let mut engine = Engine::new(state);

    activate_and_await_payment(&mut engine, p0, source);

    // Only the Sacrifice half, so the TapSelf entry is missing.
    let err = engine
        .submit_payment(PaymentPlan { payments: vec![CostPayment::Sacrifice(fodder)] })
        .unwrap_err();
    assert!(matches!(err, PaymentError::InvalidPlan));

    // Both components, in order.
    engine
        .submit_payment(PaymentPlan {
            payments: vec![CostPayment::Tap(source), CostPayment::Sacrifice(fodder)],
        })
        .expect("TapSelf + Sacrifice together pays the combined cost");
    drain(&mut engine);

    assert!(engine.state().object(source).unwrap().tapped);
    assert!(engine.state().object(fodder).is_none());
}
