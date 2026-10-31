//! CR 601.2f-h: cost determination and payment for `CastSpell`, pausing on `AwaitingPayment` until `submit_payment`.
//! `CostPayment::Mana` carries the exact units spent (CR 118.13a). A colored pip needs that color, and generic can be covered by any leftover units.

use std::collections::HashMap;

use stackfish_engine::engine::{Engine, PaymentError, StepStatus};
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, single_player_state};

fn spell_definition(mana_cost: ManaCost) -> CardDefinition {
    CardDefinition {
        definition_id: 1,
        name: "Test Spell".to_string(),
        mana_cost,
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::SORCERY,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_spell".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn state_with_pool(pool: ManaPool) -> GameState {
    let p0 = PlayerId(0);
    GameState {
        step: Step::Untap,
        last_active_turn_number: HashMap::new(),
        players: vec![PlayerState {
            mana_pool: pool,
            ..stackfish_testutil::player_state(p0)
        }],
        ..single_player_state(
            p0,
            vec![
                Zone::new(ZoneKind::Hand, Some(p0)),
                Zone::new(ZoneKind::Battlefield, None),
                Zone::new(ZoneKind::Graveyard, Some(p0)),
            ],
        )
    }
}

fn empty_pool() -> ManaPool {
    ManaPool::default()
}

fn cast(engine: &mut Engine, player: PlayerId, card: ObjectId) {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::CastSpell {
                card,
                modes: Vec::new(),
                targets: Vec::new(),
                x_value: None,
                alternative_cost: None,
                pay_buyback: false,
            },
        })
        .expect("player holds priority, spell is in hand, main phase, empty stack");
}

fn last_zone_change(engine: &Engine) -> (ObjectId, ZoneKind, ZoneKind) {
    for event in engine.state().ledger.iter().rev() {
        if let GameEventKind::ZoneChange {
            object, from, to, ..
        } = event.kind
        {
            return (object, from, to);
        }
    }
    panic!("expected a ZoneChange event in the ledger");
}

#[test]
fn affordable_payment_succeeds_and_debits_pool() {
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    let mana_cost = ManaCost {
        symbols: vec![
            ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::Green)),
            ManaSymbol::Atom(AtomicManaSymbol::Generic(2)),
        ],
    };
    let mut state = state_with_pool(ManaPool {
        green: 1,
        colorless: 2,
        ..empty_pool()
    });
    state.objects.insert(
        card,
        card_object(card, spell_definition(mana_cost), p0, ZoneKind::Hand),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);

    let mut engine = Engine::new(state);
    cast(&mut engine, p0, card);
    let status = drain(&mut engine);
    let stack_id = match status {
        StepStatus::AwaitingPayment { player, stack_id } if player == p0 => stack_id,
        other => panic!("expected AwaitingPayment, got {other:?}"),
    };

    // 1 green for the {G} pip and 2 colorless for the {2}, as the payer chose (CR 118.13a).
    let units = vec![
        ManaUnit::Colored(Color::Green),
        ManaUnit::Colorless,
        ManaUnit::Colorless,
    ];
    engine
        .submit_payment(PaymentPlan {
            payments: vec![CostPayment::Mana {
                units,
                phyrexian_life: 0,
            }],
        })
        .expect("pool covers the cost exactly");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));

    let pool = &engine.state().player(p0).unwrap().mana_pool;
    assert_eq!((pool.green, pool.colorless), (0, 0), "pool fully debited");
    assert_eq!(engine.state().stack[0].stack_id, stack_id);
    assert!(
        !engine.state().stack[0].costs_paid.is_empty(),
        "costs_paid recorded"
    );
}

// The plan is correctly shaped, so this tests real unaffordability and not a malformed plan.
#[test]
fn short_on_generic_is_rejected_and_pool_unchanged() {
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    let mana_cost = ManaCost {
        symbols: vec![
            ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::Green)),
            ManaSymbol::Atom(AtomicManaSymbol::Generic(2)),
        ],
    };
    // Only 1 colorless floating - 1 short of the required 2 generic.
    let mut state = state_with_pool(ManaPool {
        green: 1,
        colorless: 1,
        ..empty_pool()
    });
    state.objects.insert(
        card,
        card_object(card, spell_definition(mana_cost), p0, ZoneKind::Hand),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);

    let mut engine = Engine::new(state);
    cast(&mut engine, p0, card);
    drain(&mut engine);

    let units = vec![
        ManaUnit::Colored(Color::Green),
        ManaUnit::Colorless,
        ManaUnit::Colored(Color::Blue),
    ];
    let err = engine
        .submit_payment(PaymentPlan {
            payments: vec![CostPayment::Mana {
                units,
                phyrexian_life: 0,
            }],
        })
        .unwrap_err();
    assert!(matches!(err, PaymentError::InvalidPlan));
    let pool = &engine.state().player(p0).unwrap().mana_pool;
    assert_eq!(
        (pool.green, pool.colorless),
        (1, 1),
        "no partial deduction on a rejected plan"
    );
    assert!(
        matches!(engine.state().stack[0].costs_paid.as_slice(), []),
        "cost not recorded"
    );
}

#[test]
fn wrong_color_is_rejected() {
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    let mana_cost = ManaCost {
        symbols: vec![ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::Green))],
    };
    let mut state = state_with_pool(ManaPool {
        blue: 1,
        ..empty_pool()
    });
    state.objects.insert(
        card,
        card_object(card, spell_definition(mana_cost), p0, ZoneKind::Hand),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);

    let mut engine = Engine::new(state);
    cast(&mut engine, p0, card);
    drain(&mut engine);

    let units = vec![ManaUnit::Colored(Color::Green)];
    let err = engine
        .submit_payment(PaymentPlan {
            payments: vec![CostPayment::Mana {
                units,
                phyrexian_life: 0,
            }],
        })
        .unwrap_err();
    assert!(matches!(err, PaymentError::InvalidPlan));
    assert_eq!(
        engine.state().player(p0).unwrap().mana_pool.blue,
        1,
        "pool untouched"
    );
}

// CR 118.13a
// Two blue units match the unit count, but the {G} pip must be paid with green.
#[test]
fn colored_pip_cannot_be_substituted_by_a_different_color() {
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    let mana_cost = ManaCost {
        symbols: vec![
            ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::Green)),
            ManaSymbol::Atom(AtomicManaSymbol::Generic(1)),
        ],
    };
    let mut state = state_with_pool(ManaPool {
        blue: 2,
        green: 1,
        ..empty_pool()
    });
    state.objects.insert(
        card,
        card_object(card, spell_definition(mana_cost), p0, ZoneKind::Hand),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);

    let mut engine = Engine::new(state);
    cast(&mut engine, p0, card);
    drain(&mut engine);

    let units = vec![
        ManaUnit::Colored(Color::Blue),
        ManaUnit::Colored(Color::Blue),
    ];
    let err = engine
        .submit_payment(PaymentPlan {
            payments: vec![CostPayment::Mana {
                units,
                phyrexian_life: 0,
            }],
        })
        .unwrap_err();
    assert!(matches!(err, PaymentError::InvalidPlan));
    let pool = &engine.state().player(p0).unwrap().mana_pool;
    assert_eq!((pool.blue, pool.green), (2, 1), "pool untouched");
}

// CR 118.13a
// Generic can be covered by any mix of leftover colors and colorless.
#[test]
fn generic_can_be_paid_with_a_mix_of_colors() {
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    let mana_cost = ManaCost {
        symbols: vec![ManaSymbol::Atom(AtomicManaSymbol::Generic(2))],
    };
    let mut state = state_with_pool(ManaPool {
        white: 1,
        blue: 1,
        ..empty_pool()
    });
    state.objects.insert(
        card,
        card_object(card, spell_definition(mana_cost), p0, ZoneKind::Hand),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);

    let mut engine = Engine::new(state);
    cast(&mut engine, p0, card);
    drain(&mut engine);

    let units = vec![
        ManaUnit::Colored(Color::White),
        ManaUnit::Colored(Color::Blue),
    ];
    engine
        .submit_payment(PaymentPlan {
            payments: vec![CostPayment::Mana {
                units,
                phyrexian_life: 0,
            }],
        })
        .expect("a {2} generic cost can be paid with any mix of colors");
    let pool = &engine.state().player(p0).unwrap().mana_pool;
    assert_eq!((pool.white, pool.blue), (0, 0), "both units spent");
}

// CR 118.5, 118.5a
// A {0} spell still needs an explicit empty payment.
#[test]
fn zero_cost_spell_requires_explicit_empty_payment() {
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    let mana_cost = ManaCost {
        symbols: Vec::new(),
    };
    let mut state = state_with_pool(empty_pool());
    state.objects.insert(
        card,
        card_object(card, spell_definition(mana_cost), p0, ZoneKind::Hand),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);

    let mut engine = Engine::new(state);
    cast(&mut engine, p0, card);
    let status = drain(&mut engine);
    assert!(
        matches!(status, StepStatus::AwaitingPayment { player, .. } if player == p0),
        "even a free spell must pause for an explicit pay acknowledgment"
    );

    engine
        .submit_payment(PaymentPlan {
            payments: Vec::new(),
        })
        .expect("empty plan pays a {0} cost");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
}

#[test]
fn exact_amount_drains_pool_to_zero() {
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    let mana_cost = ManaCost {
        symbols: vec![
            ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::Red)),
            ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::Red)),
            ManaSymbol::Atom(AtomicManaSymbol::Generic(1)),
        ],
    };
    let mut state = state_with_pool(ManaPool {
        red: 2,
        white: 1,
        ..empty_pool()
    });
    state.objects.insert(
        card,
        card_object(card, spell_definition(mana_cost), p0, ZoneKind::Hand),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);

    let mut engine = Engine::new(state);
    cast(&mut engine, p0, card);
    drain(&mut engine);

    // 2 red for the two {R} pips, the spare white covering the {1}.
    let units = vec![
        ManaUnit::Colored(Color::Red),
        ManaUnit::Colored(Color::Red),
        ManaUnit::Colored(Color::White),
    ];
    engine
        .submit_payment(PaymentPlan {
            payments: vec![CostPayment::Mana {
                units,
                phyrexian_life: 0,
            }],
        })
        .expect("2 red + 1 generic from the leftover white covers it exactly");
    let pool = &engine.state().player(p0).unwrap().mana_pool;
    assert_eq!(
        (pool.red, pool.white),
        (0, 0),
        "pool drained to exactly zero"
    );
}

#[test]
fn submit_payment_while_not_awaiting_is_rejected() {
    let p0 = PlayerId(0);
    let mut engine = Engine::new(state_with_pool(empty_pool()));
    let err = engine
        .submit_payment(PaymentPlan {
            payments: Vec::new(),
        })
        .unwrap_err();
    assert!(matches!(err, PaymentError::NotAwaitingPayment));
    let _ = p0;
}

// A plan spending more units than owed is rejected, even though the pool could afford it.
#[test]
fn mismatched_plan_with_extra_payment_is_rejected() {
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    let mana_cost = ManaCost {
        symbols: vec![ManaSymbol::Atom(AtomicManaSymbol::Generic(1))],
    };
    let mut state = state_with_pool(ManaPool {
        colorless: 5,
        ..empty_pool()
    });
    state.objects.insert(
        card,
        card_object(card, spell_definition(mana_cost), p0, ZoneKind::Hand),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);

    let mut engine = Engine::new(state);
    cast(&mut engine, p0, card);
    drain(&mut engine);

    // 3 colorless units for a {1} cost is more than owed.
    let overpay = vec![
        ManaUnit::Colorless,
        ManaUnit::Colorless,
        ManaUnit::Colorless,
    ];
    let err = engine
        .submit_payment(PaymentPlan {
            payments: vec![CostPayment::Mana {
                units: overpay,
                phyrexian_life: 0,
            }],
        })
        .unwrap_err();
    assert!(matches!(err, PaymentError::InvalidPlan));
    assert_eq!(
        engine.state().player(p0).unwrap().mana_pool.colorless,
        5,
        "pool untouched"
    );
}

#[test]
fn paid_cast_still_resolves_to_graveyard() {
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    let mana_cost = ManaCost {
        symbols: vec![ManaSymbol::Atom(AtomicManaSymbol::Generic(1))],
    };
    let mut state = state_with_pool(ManaPool {
        colorless: 1,
        ..empty_pool()
    });
    state.objects.insert(
        card,
        card_object(card, spell_definition(mana_cost), p0, ZoneKind::Hand),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);

    let mut engine = Engine::new(state);
    cast(&mut engine, p0, card);
    drain(&mut engine);
    engine
        .submit_payment(PaymentPlan {
            payments: vec![CostPayment::Mana {
                units: vec![ManaUnit::Colorless],
                phyrexian_life: 0,
            }],
        })
        .unwrap();
    drain(&mut engine);

    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::PassPriority,
        })
        .unwrap();
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));

    assert!(
        engine.state().stack.is_empty(),
        "spell resolved off the stack"
    );
    let (_, from, to) = last_zone_change(&engine);
    assert_eq!((from, to), (ZoneKind::Stack, ZoneKind::Graveyard));
}

// CR 107.4f
// A Phyrexian pip paid with life deducts it and records it in the ledger.
#[test]
fn phyrexian_pip_paid_with_life_deducts_life_and_logs_it() {
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    let mana_cost = ManaCost {
        symbols: vec![ManaSymbol::Atom(AtomicManaSymbol::Phyrexian(Color::White))],
    };
    let mut state = state_with_pool(empty_pool());
    state.objects.insert(
        card,
        card_object(card, spell_definition(mana_cost), p0, ZoneKind::Hand),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);

    let mut engine = Engine::new(state);
    cast(&mut engine, p0, card);
    drain(&mut engine);

    engine
        .submit_payment(PaymentPlan {
            payments: vec![CostPayment::Mana {
                units: Vec::new(),
                phyrexian_life: 2,
            }],
        })
        .expect("no white mana available, but 2 life covers the Phyrexian pip");
    drain(&mut engine);

    assert_eq!(
        engine.state().player(p0).unwrap().life,
        18,
        "life actually spent, not just checked"
    );
    let logged = engine.state().ledger.iter().any(|event| {
        matches!(event.kind, GameEventKind::LifeChanged { player, delta } if player == p0 && delta == -2)
    });
    assert!(
        logged,
        "expected a LifeChanged {{ player: p0, delta: -2 }} event in the ledger"
    );
}

// CR 119.4
// A Phyrexian pip cannot take more life than the payer has.
#[test]
fn phyrexian_life_payment_rejected_below_zero_life() {
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    let mana_cost = ManaCost {
        symbols: vec![ManaSymbol::Atom(AtomicManaSymbol::Phyrexian(Color::White))],
    };
    let mut state = state_with_pool(empty_pool());
    state.players[0].life = 1;
    state.objects.insert(
        card,
        card_object(card, spell_definition(mana_cost), p0, ZoneKind::Hand),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);

    let mut engine = Engine::new(state);
    cast(&mut engine, p0, card);
    drain(&mut engine);

    let err = engine
        .submit_payment(PaymentPlan {
            payments: vec![CostPayment::Mana {
                units: Vec::new(),
                phyrexian_life: 2,
            }],
        })
        .unwrap_err();
    assert!(matches!(err, PaymentError::InvalidPlan));
    assert_eq!(
        engine.state().player(p0).unwrap().life,
        1,
        "life untouched by a rejected plan"
    );
}

// CR 107.4h
// An {S} pip must debit the declared snow unit, not any unit of that color.
#[test]
fn snow_pip_debits_the_snow_tagged_unit_specifically() {
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    let mana_cost = ManaCost {
        symbols: vec![ManaSymbol::Atom(AtomicManaSymbol::Snow)],
    };
    let mut state = state_with_pool(ManaPool {
        white: 2,
        snow_white: 1,
        ..empty_pool()
    });
    state.objects.insert(
        card,
        card_object(card, spell_definition(mana_cost), p0, ZoneKind::Hand),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);

    let mut engine = Engine::new(state);
    cast(&mut engine, p0, card);
    drain(&mut engine);

    // The non-snow white unit is the right color from the wrong source.
    let err = engine
        .submit_payment(PaymentPlan {
            payments: vec![CostPayment::Mana {
                units: vec![ManaUnit::Colored(Color::White)],
                phyrexian_life: 0,
            }],
        })
        .unwrap_err();
    assert!(matches!(err, PaymentError::InvalidPlan));
    let pool = &engine.state().player(p0).unwrap().mana_pool;
    assert_eq!(
        (pool.white, pool.snow_white),
        (2, 1),
        "rejected plan leaves the pool untouched"
    );

    // The snow unit is legal and debits the color total and the snow subcount.
    engine
        .submit_payment(PaymentPlan {
            payments: vec![CostPayment::Mana {
                units: vec![ManaUnit::SnowColored(Color::White)],
                phyrexian_life: 0,
            }],
        })
        .expect("the snow-tagged white unit legally pays {S}");
    let pool = &engine.state().player(p0).unwrap().mana_pool;
    assert_eq!(
        (pool.white, pool.snow_white),
        (1, 0),
        "snow unit and its color total both debited"
    );
}
