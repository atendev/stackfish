//! Phase 5 vertical slice: cost determination + payment (CR 601.2f-h) for
//! `PlayerActionKind::CastSpell`. Builds on `phase5_cast_spell.rs`'s free-cost
//! stub - the `CastSpell` arm now builds a real `CostSpec` from the card's
//! `mana_cost` and pauses the pipeline on `ProcessState::AwaitingPayment`
//! until `Engine::submit_payment` answers (see engine.rs/commands.rs/cost.rs).
//! No mana abilities/tapping exist yet (CR 605, its own future slice) - test
//! fixtures hand-set `PlayerState.mana_pool` directly, fake pre-seeded
//! floating mana.
//!
//! `CostPayment::Mana` takes a `Vec<ManaUnit>` - the exact literal units
//! spent (CR 118.13a's payment choice), not an echo of the cost's shape.
//! A colored pip must be paid with that exact color; the generic component
//! can be paid with any leftover units (extra of a required color, a
//! different color entirely, or colorless) - see cost.rs's `PaymentPlan::
//! validate` doc comment.

use std::collections::{HashMap, VecDeque};

use stackfish_engine::engine::{Engine, PaymentError, StepStatus};
use stackfish_engine::*;

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
        effect: None,
        modes: Vec::new(),
    }
}

fn card_object(id: ObjectId, definition: CardDefinition, owner: PlayerId, zone: ZoneKind) -> GameObject {
    GameObject {
        object_id: id,
        lineage: ObjectLineage { previous: None },
        definition,
        owner,
        controller: owner,
        zone,
        tapped: false,
        flipped: false,
        face_down: false,
        phased_out: false,
        damage_marked: 0,
        deathtouched: false,
        counters: CounterSet::new(),
        attachments: Vec::new(),
        attached_to: None,
        timestamp: SequenceNo(0),
        controlled_since_turn: 0,
        is_token: false,
        is_commander: false,
        visibility: VisibilityPolicy { public: true, visible_to: Vec::new() },
    }
}

/// One player, main phase, empty stack, a hand zone - `mana_pool` is
/// hand-set per test (CR 605's mana abilities don't exist yet, so a fixture
/// pre-seeding the pool directly is the only way to give a player mana to
/// pay with this slice).
fn state_with_pool(pool: ManaPool) -> GameState {
    let p0 = PlayerId(0);
    GameState {
        players: vec![PlayerState {
            player_id: p0,
            life: 20,
            mana_pool: pool,
            counters: CounterSet::new(),
            emblems: Vec::new(),
            commander_damage_taken: Vec::new(),
            eliminated: false,
            drew_from_empty_library: false,
            mulligans_taken: 0,
            hand_kept: false,
        }],
        turn_order: vec![p0],
        active_player: p0,
        turn_number: 1,
        phase: Phase::Main,
        step: Step::Untap,
        priority_player: p0,
        consecutive_passes: 0,
        lands_played_this_turn: HashMap::new(),
        combat: None,
        last_active_turn_number: HashMap::new(),
        objects: HashMap::new(),
        zones: vec![
            Zone { kind: ZoneKind::Hand, owner: Some(p0), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Battlefield, owner: None, contents: VecDeque::new() },
            Zone { kind: ZoneKind::Graveyard, owner: Some(p0), contents: VecDeque::new() },
        ],
        stack: Vec::new(),
        continuous_effects: Vec::new(),
        replacement_effects: Vec::new(),
        cost_modifiers: Vec::new(),
        abilities: Vec::new(),
        ledger: Vec::new(),
        rng: RngState { seed: 1, draws_consumed: 0 },
        next_event_id: 0,
        next_sequence_no: 0,
        // Not 0 - collides with the literal ObjectId(0) fixture cards use,
        // same reasoning as phase5_cast_spell.rs.
        next_object_id: 1000,
        pending_step_skips: Vec::new(),
        once_per_turn_triggered: std::collections::HashSet::new(),
        commander_return_offered: std::collections::HashSet::new(),
    }
}

fn empty_pool() -> ManaPool {
    ManaPool::default()
}

fn drain(engine: &mut Engine) -> StepStatus {
    loop {
        let result = engine.step();
        match result.status {
            StepStatus::Progressed => continue,
            other => return other,
        }
    }
}

fn cast(engine: &mut Engine, player: PlayerId, card: ObjectId) {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::CastSpell { card, modes: Vec::new(), targets: Vec::new(), x_value: None, alternative_cost: None },
        })
        .expect("player holds priority, spell is in hand, main phase, empty stack");
}

fn last_zone_change(engine: &Engine) -> (ObjectId, ZoneKind, ZoneKind) {
    for event in engine.state().ledger.iter().rev() {
        if let GameEventKind::ZoneChange { object, from, to, .. } = event.kind {
            return (object, from, to);
        }
    }
    panic!("expected a ZoneChange event in the ledger");
}

/// Test case 1: affordable cost, pool has enough - succeeds, pool debited
/// the right amounts, spell proceeds same as the free-cost slice already
/// covers.
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
    let mut state = state_with_pool(ManaPool { green: 1, colorless: 2, ..empty_pool() });
    state.objects.insert(card, card_object(card, spell_definition(mana_cost), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast(&mut engine, p0, card);
    let status = drain(&mut engine);
    let stack_id = match status {
        StepStatus::AwaitingPayment { player, stack_id } if player == p0 => stack_id,
        other => panic!("expected AwaitingPayment, got {other:?}"),
    };

    // 1 green for the {G} pip, 2 colorless covering the {2} - the payer's
    // explicit choice (CR 118.13a), not an echo of the cost's shape.
    let units = vec![ManaUnit::Colored(Color::Green), ManaUnit::Colorless, ManaUnit::Colorless];
    engine
        .submit_payment(PaymentPlan { payments: vec![CostPayment::Mana { units, phyrexian_life: 0 }] })
        .expect("pool covers the cost exactly");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));

    let pool = &engine.state().player(p0).unwrap().mana_pool;
    assert_eq!((pool.green, pool.colorless), (0, 0), "pool fully debited");
    assert_eq!(engine.state().stack[0].stack_id, stack_id);
    assert!(!engine.state().stack[0].costs_paid.is_empty(), "costs_paid recorded");
}

/// Test case 2: pool short on generic - rejected, pool unchanged. Submits a
/// correctly-*shaped* plan (1 green pip + 2 generic-covering units, matching
/// the cost exactly) so this tests real unaffordability, not a malformed
/// plan - the pool just doesn't have a second unit to spare.
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
    let mut state = state_with_pool(ManaPool { green: 1, colorless: 1, ..empty_pool() });
    state.objects.insert(card, card_object(card, spell_definition(mana_cost), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast(&mut engine, p0, card);
    drain(&mut engine);

    // Shape is correct (1 green pip + 2 generic-covering units) but the
    // pool has no second unit to spend on the second one.
    let units = vec![ManaUnit::Colored(Color::Green), ManaUnit::Colorless, ManaUnit::Colored(Color::Blue)];
    let err = engine.submit_payment(PaymentPlan { payments: vec![CostPayment::Mana { units, phyrexian_life: 0 }] }).unwrap_err();
    assert!(matches!(err, PaymentError::InvalidPlan));
    let pool = &engine.state().player(p0).unwrap().mana_pool;
    assert_eq!((pool.green, pool.colorless), (1, 1), "no partial deduction on a rejected plan");
    assert!(matches!(engine.state().stack[0].costs_paid.as_slice(), []), "cost not recorded");
}

/// Test case 3: pool has enough total mana but the wrong color (needs
/// {G}, pool only has {U}) - rejected.
#[test]
fn wrong_color_is_rejected() {
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    let mana_cost = ManaCost { symbols: vec![ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::Green))] };
    let mut state = state_with_pool(ManaPool { blue: 1, ..empty_pool() });
    state.objects.insert(card, card_object(card, spell_definition(mana_cost), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast(&mut engine, p0, card);
    drain(&mut engine);

    let units = vec![ManaUnit::Colored(Color::Green)];
    let err = engine.submit_payment(PaymentPlan { payments: vec![CostPayment::Mana { units, phyrexian_life: 0 }] }).unwrap_err();
    assert!(matches!(err, PaymentError::InvalidPlan));
    assert_eq!(engine.state().player(p0).unwrap().mana_pool.blue, 1, "pool untouched");
}

/// CR 118.13a: a colored pip can't be substituted by another color, even
/// when the *total* unit count would otherwise line up with the cost
/// (1 pip + 1 generic = 2 units here, and 2 blue units are offered) - the
/// {G} pip specifically must be paid with green.
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
    let mut state = state_with_pool(ManaPool { blue: 2, green: 1, ..empty_pool() });
    state.objects.insert(card, card_object(card, spell_definition(mana_cost), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast(&mut engine, p0, card);
    drain(&mut engine);

    let units = vec![ManaUnit::Colored(Color::Blue), ManaUnit::Colored(Color::Blue)];
    let err = engine.submit_payment(PaymentPlan { payments: vec![CostPayment::Mana { units, phyrexian_life: 0 }] }).unwrap_err();
    assert!(matches!(err, PaymentError::InvalidPlan));
    let pool = &engine.state().player(p0).unwrap().mana_pool;
    assert_eq!((pool.blue, pool.green), (2, 1), "pool untouched");
}

/// CR 118.13a's actual choice: the generic component can be covered by
/// *any* leftover units, including a mix of different colors and colorless
/// in the same payment - not just "one type of leftover."
#[test]
fn generic_can_be_paid_with_a_mix_of_colors() {
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    let mana_cost = ManaCost { symbols: vec![ManaSymbol::Atom(AtomicManaSymbol::Generic(2))] };
    let mut state = state_with_pool(ManaPool { white: 1, blue: 1, ..empty_pool() });
    state.objects.insert(card, card_object(card, spell_definition(mana_cost), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast(&mut engine, p0, card);
    drain(&mut engine);

    let units = vec![ManaUnit::Colored(Color::White), ManaUnit::Colored(Color::Blue)];
    engine
        .submit_payment(PaymentPlan { payments: vec![CostPayment::Mana { units, phyrexian_life: 0 }] })
        .expect("a {2} generic cost can be paid with any mix of colors");
    let pool = &engine.state().player(p0).unwrap().mana_pool;
    assert_eq!((pool.white, pool.blue), (0, 0), "both units spent");
}

/// Test case 4: a {0}-cost spell still needs an explicit empty payment -
/// not skipped/auto-cast (CR 118.5/118.5a).
#[test]
fn zero_cost_spell_requires_explicit_empty_payment() {
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    let mana_cost = ManaCost { symbols: Vec::new() };
    let mut state = state_with_pool(empty_pool());
    state.objects.insert(card, card_object(card, spell_definition(mana_cost), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast(&mut engine, p0, card);
    let status = drain(&mut engine);
    assert!(
        matches!(status, StepStatus::AwaitingPayment { player, .. } if player == p0),
        "even a free spell must pause for an explicit pay acknowledgment"
    );

    engine.submit_payment(PaymentPlan { payments: Vec::new() }).expect("empty plan pays a {0} cost");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
}

/// Test case 5: exact affordable amount - pool fully drained to 0, succeeds.
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
    let mut state = state_with_pool(ManaPool { red: 2, white: 1, ..empty_pool() });
    state.objects.insert(card, card_object(card, spell_definition(mana_cost), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast(&mut engine, p0, card);
    drain(&mut engine);

    // 2 red for the two {R} pips, the spare white covering the {1}.
    let units =
        vec![ManaUnit::Colored(Color::Red), ManaUnit::Colored(Color::Red), ManaUnit::Colored(Color::White)];
    engine
        .submit_payment(PaymentPlan { payments: vec![CostPayment::Mana { units, phyrexian_life: 0 }] })
        .expect("2 red + 1 generic from the leftover white covers it exactly");
    let pool = &engine.state().player(p0).unwrap().mana_pool;
    assert_eq!((pool.red, pool.white), (0, 0), "pool drained to exactly zero");
}

/// Test case 6: submitting a payment while not `AwaitingPayment` is
/// rejected, same shape as `ReplacementChoiceError::NotAwaitingChoice`.
#[test]
fn submit_payment_while_not_awaiting_is_rejected() {
    let p0 = PlayerId(0);
    let mut engine = Engine::new(state_with_pool(empty_pool()));
    let err = engine.submit_payment(PaymentPlan { payments: Vec::new() }).unwrap_err();
    assert!(matches!(err, PaymentError::NotAwaitingPayment));
    let _ = p0;
}

/// Test case 7: a plan that spends more units than the cost requires (extra
/// payment beyond what's owed) is rejected, pool untouched - even though the
/// pool could easily afford it.
#[test]
fn mismatched_plan_with_extra_payment_is_rejected() {
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    let mana_cost = ManaCost { symbols: vec![ManaSymbol::Atom(AtomicManaSymbol::Generic(1))] };
    let mut state = state_with_pool(ManaPool { colorless: 5, ..empty_pool() });
    state.objects.insert(card, card_object(card, spell_definition(mana_cost), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast(&mut engine, p0, card);
    drain(&mut engine);

    // 3 colorless units for a {1} cost - more than owed, not just
    // "affordable from a big pool".
    let overpay = vec![ManaUnit::Colorless, ManaUnit::Colorless, ManaUnit::Colorless];
    let err =
        engine
            .submit_payment(PaymentPlan {
                payments: vec![CostPayment::Mana { units: overpay, phyrexian_life: 0 }],
            })
            .unwrap_err();
    assert!(matches!(err, PaymentError::InvalidPlan));
    assert_eq!(engine.state().player(p0).unwrap().mana_pool.colorless, 5, "pool untouched");
}

/// Test case 8: resolution after a paid cast still lands on the
/// battlefield/graveyard correctly - cheap regression check against
/// phase5_cast_spell.rs's existing coverage, now with a real (paid) cost.
#[test]
fn paid_cast_still_resolves_to_graveyard() {
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    let mana_cost = ManaCost { symbols: vec![ManaSymbol::Atom(AtomicManaSymbol::Generic(1))] };
    let mut state = state_with_pool(ManaPool { colorless: 1, ..empty_pool() });
    state.objects.insert(card, card_object(card, spell_definition(mana_cost), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast(&mut engine, p0, card);
    drain(&mut engine);
    engine
        .submit_payment(PaymentPlan {
            payments: vec![CostPayment::Mana { units: vec![ManaUnit::Colorless], phyrexian_life: 0 }],
        })
        .unwrap();
    drain(&mut engine);

    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).unwrap();
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));

    assert!(engine.state().stack.is_empty(), "spell resolved off the stack");
    let (_, from, to) = last_zone_change(&engine);
    assert_eq!((from, to), (ZoneKind::Stack, ZoneKind::Graveyard));
}

/// Test case 9 (6a-ii): a Phyrexian pip paid via life (CR 107.4f) actually
/// deducts life from the payer and records it in the ledger - not just a
/// validator-only check.
#[test]
fn phyrexian_pip_paid_with_life_deducts_life_and_logs_it() {
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    let mana_cost =
        ManaCost { symbols: vec![ManaSymbol::Atom(AtomicManaSymbol::Phyrexian(Color::White))] };
    let mut state = state_with_pool(empty_pool());
    state.objects.insert(card, card_object(card, spell_definition(mana_cost), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast(&mut engine, p0, card);
    drain(&mut engine);

    engine
        .submit_payment(PaymentPlan {
            payments: vec![CostPayment::Mana { units: Vec::new(), phyrexian_life: 2 }],
        })
        .expect("no white mana available, but 2 life covers the Phyrexian pip");
    drain(&mut engine);

    assert_eq!(engine.state().player(p0).unwrap().life, 18, "life actually spent, not just checked");
    let logged = engine.state().ledger.iter().any(|event| {
        matches!(event.kind, GameEventKind::LifeChanged { player, delta } if player == p0 && delta == -2)
    });
    assert!(logged, "expected a LifeChanged {{ player: p0, delta: -2 }} event in the ledger");
}

/// Test case 10 (6a-ii): CR 119.4 - can't pay more life toward a Phyrexian
/// pip than the payer has, even though the payment is otherwise well-formed.
#[test]
fn phyrexian_life_payment_rejected_below_zero_life() {
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    let mana_cost =
        ManaCost { symbols: vec![ManaSymbol::Atom(AtomicManaSymbol::Phyrexian(Color::White))] };
    let mut state = state_with_pool(empty_pool());
    state.players[0].life = 1;
    state.objects.insert(card, card_object(card, spell_definition(mana_cost), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast(&mut engine, p0, card);
    drain(&mut engine);

    let err = engine
        .submit_payment(PaymentPlan {
            payments: vec![CostPayment::Mana { units: Vec::new(), phyrexian_life: 2 }],
        })
        .unwrap_err();
    assert!(matches!(err, PaymentError::InvalidPlan));
    assert_eq!(engine.state().player(p0).unwrap().life, 1, "life untouched by a rejected plan");
}

/// Test case 11 (6a-iii): a `{S}` pip must debit the specific snow-tagged
/// unit declared, not just any unit of the same color (CR 107.4h) - the
/// pool here has both a snow and a non-snow white mana, only the snow one
/// is a legal `{S}` payment.
#[test]
fn snow_pip_debits_the_snow_tagged_unit_specifically() {
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    let mana_cost = ManaCost { symbols: vec![ManaSymbol::Atom(AtomicManaSymbol::Snow)] };
    let mut state = state_with_pool(ManaPool { white: 2, snow_white: 1, ..empty_pool() });
    state.objects.insert(card, card_object(card, spell_definition(mana_cost), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast(&mut engine, p0, card);
    drain(&mut engine);

    // Declaring the ordinary (non-snow) white unit for {S} is rejected -
    // right color, wrong source.
    let err = engine
        .submit_payment(PaymentPlan {
            payments: vec![CostPayment::Mana { units: vec![ManaUnit::Colored(Color::White)], phyrexian_life: 0 }],
        })
        .unwrap_err();
    assert!(matches!(err, PaymentError::InvalidPlan));
    let pool = &engine.state().player(p0).unwrap().mana_pool;
    assert_eq!((pool.white, pool.snow_white), (2, 1), "rejected plan leaves the pool untouched");

    // Declaring the snow-tagged one is legal, and debits both the color
    // total and the snow subcount together.
    engine
        .submit_payment(PaymentPlan {
            payments: vec![CostPayment::Mana {
                units: vec![ManaUnit::SnowColored(Color::White)],
                phyrexian_life: 0,
            }],
        })
        .expect("the snow-tagged white unit legally pays {S}");
    let pool = &engine.state().player(p0).unwrap().mana_pool;
    assert_eq!((pool.white, pool.snow_white), (1, 0), "snow unit and its color total both debited");
}
