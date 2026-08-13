//! Phase 5 vertical slice: CR 601.2g/605.3a — a mana ability may be
//! activated "even if it's in the middle of casting or resolving a spell or
//! activating or resolving an ability" that itself requires a mana payment.
//! Before this slice, `Engine::submit_action` only accepted input while
//! `process_state == PriorityWindow` — once `CastSpell`/`ActivateAbility`
//! paused on `AwaitingPayment`, tapping a land for mana to cover that very
//! cost was rejected outright, forcing every test fixture to pre-seed the
//! mana pool by hand instead. `submit_action` now also accepts a nested
//! `ActivateAbility` for the *payer's own* mana ability while `AwaitingPayment`
//! is open, suspending the outer payment (`Engine::suspended_payments`) and
//! resuming it once the nested activation's own pipeline run settles (see
//! `submit_action`'s own doc comment for the full mechanism, including the
//! `pipeline_context`/`pipeline_exit` save/restore a real-cost nested mana
//! ability needs).

use stackfish_engine::engine::{ActionError, Engine, PaymentError, StepStatus};
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, hand_battlefield_graveyard_zones, minimal_card_definition, two_player_state};

fn spell_definition(mana_cost: ManaCost) -> CardDefinition {
    minimal_card_definition(1, "Test Spell", "test_spell", mana_cost, CardTypeSet::SORCERY)
}

fn land_definition() -> CardDefinition {
    CardDefinition {
        oracle_text: "{T}: Add {R}.".to_string(),
        ..minimal_card_definition(2, "Test Mountain", "test_mountain", ManaCost { symbols: Vec::new() }, CardTypeSet::LAND)
    }
}

fn creature_definition() -> CardDefinition {
    CardDefinition {
        power: Some(PTValue::Fixed(1)),
        toughness: Some(PTValue::Fixed(1)),
        oracle_text: "{T}: You gain 1 life.".to_string(),
        ..minimal_card_definition(3, "Test Cleric", "test_cleric", ManaCost { symbols: Vec::new() }, CardTypeSet::CREATURE)
    }
}

fn treasure_definition() -> CardDefinition {
    CardDefinition {
        oracle_text: "Sacrifice this artifact: Add one blue mana.".to_string(),
        ..minimal_card_definition(4, "Test Treasure", "test_treasure", ManaCost { symbols: Vec::new() }, CardTypeSet::ARTIFACT)
    }
}

fn mana_ability(id: u64, source: ObjectId, colors: Vec<ManaUnit>) -> RegisteredAbility {
    RegisteredAbility {
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
        id: AbilityId(id),
        source,
        kind: AbilityKind::Activated,
        condition: None,
        cost: None,
        target: None,
        operation: ScriptHandle(0),
        effect: Some(AbilityEffect::AddMana(colors)),
        intervening_if: None,
    }
}

fn non_mana_ability(id: u64, source: ObjectId) -> RegisteredAbility {
    RegisteredAbility {
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
        id: AbilityId(id),
        source,
        kind: AbilityKind::Activated,
        condition: None,
        cost: Some(CostSpec { mana: None, additional: vec![AdditionalCost::TapSelf], alternative: Vec::new() }),
        target: None,
        operation: ScriptHandle(0),
        effect: Some(AbilityEffect::GainLife(1)),
        intervening_if: None,
    }
}

/// Sacrifice-cost mana ability, Treasure/Lotus-Petal-style — has a *real*
/// cost, so activating it pauses `AwaitingPayment` on its own (CR 605.1a/
/// 605.3b: still never touches the stack).
fn sacrifice_mana_ability(id: u64, source: ObjectId) -> RegisteredAbility {
    RegisteredAbility {
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
        id: AbilityId(id),
        source,
        kind: AbilityKind::Activated,
        condition: None,
        cost: Some(CostSpec {
            mana: None,
            additional: vec![AdditionalCost::Sacrifice {
                filter: FilterSpec { card_types: Some(CardTypeSet::ARTIFACT), supertypes: None, subtypes: None, controller: None, random: false, extra: None },
            }],
            alternative: Vec::new(),
        }),
        target: None,
        operation: ScriptHandle(0),
        effect: Some(AbilityEffect::AddMana(vec![ManaUnit::Colored(Color::Blue)])),
        intervening_if: None,
    }
}

fn activate(engine: &mut Engine, player: PlayerId, source: ObjectId) -> Result<(), ActionError> {
    engine.submit_action(PlayerAction {
        player,
        kind: PlayerActionKind::ActivateAbility { source, ability_index: 0, targets: Vec::new(), chosen_colors: Vec::new(),
        x_value: None,
        },
    })
}

/// p0: hand with a {R}{R} spell, battlefield with two untapped Mountains
/// (own mana abilities) and a Cleric (non-mana {T}: gain 1 life ability).
/// p1: battlefield with its own untapped Mountain, for the "not the payer"
/// rejection test.
fn state_with_fixtures() -> (GameState, ObjectId, ObjectId, ObjectId, ObjectId, ObjectId, ObjectId) {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let spell = ObjectId(0);
    let land1 = ObjectId(1);
    let land2 = ObjectId(2);
    let cleric = ObjectId(3);
    let opponent_land = ObjectId(4);
    let treasure = ObjectId(5);

    let mana_cost = ManaCost {
        symbols: vec![ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::Red)), ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::Red))],
    };

    let mut state = two_player_state(hand_battlefield_graveyard_zones(p0, p1));
    state.objects.insert(spell, card_object(spell, spell_definition(mana_cost), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(spell);

    for &land in &[land1, land2] {
        state.objects.insert(land, card_object(land, land_definition(), p0, ZoneKind::Battlefield));
        state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(land);
    }
    state.objects.insert(cleric, card_object(cleric, creature_definition(), p0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(cleric);
    state.objects.insert(opponent_land, card_object(opponent_land, land_definition(), p1, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(opponent_land);
    state.objects.insert(treasure, card_object(treasure, treasure_definition(), p0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(treasure);

    state.abilities = vec![
        mana_ability(0, land1, vec![ManaUnit::Colored(Color::Red)]),
        mana_ability(1, land2, vec![ManaUnit::Colored(Color::Red)]),
        non_mana_ability(2, cleric),
        mana_ability(3, opponent_land, vec![ManaUnit::Colored(Color::Red)]),
        sacrifice_mana_ability(4, treasure),
    ];

    (state, spell, land1, land2, cleric, opponent_land, treasure)
}

fn cast(engine: &mut Engine, player: PlayerId, card: ObjectId) {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::CastSpell { card, modes: Vec::new(), targets: Vec::new(), x_value: None, alternative_cost: None, pay_buyback: false },
        })
        .expect("player holds priority, spell is in hand, main phase, empty stack");
}

/// The direct repro: tap two lands mid-payment to cover a spell's mana cost,
/// then pay with the mana that produced.
#[test]
fn tapping_a_land_mid_payment_floats_mana_the_payment_then_spends() {
    let p0 = PlayerId(0);
    let (state, spell, land1, land2, ..) = state_with_fixtures();
    let mut engine = Engine::new(state);

    cast(&mut engine, p0, spell);
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { player, .. } if player == p0));

    activate(&mut engine, p0, land1).expect("CR 601.2g/605.3a: a mana ability may be activated mid-payment");
    let status = drain(&mut engine);
    assert!(
        matches!(status, StepStatus::AwaitingPayment { player, .. } if player == p0),
        "resuming the outer payment after the nested mana ability settles, got {status:?}"
    );
    assert!(engine.state().object(land1).unwrap().tapped);
    assert_eq!(engine.state().player(p0).unwrap().mana_pool.red, 1);

    activate(&mut engine, p0, land2).expect("second nested mana ability, same as the first");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { player, .. } if player == p0));
    assert_eq!(engine.state().player(p0).unwrap().mana_pool.red, 2);

    engine
        .submit_payment(PaymentPlan {
            payments: vec![CostPayment::Mana { units: vec![ManaUnit::Colored(Color::Red), ManaUnit::Colored(Color::Red)], phyrexian_life: 0 }],
        })
        .expect("the mana tapped mid-payment covers the {R}{R} cost exactly");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert_eq!(engine.state().player(p0).unwrap().mana_pool.red, 0);
}

/// CR 605.3a's exception is for the payer's own mana ability only — an
/// opponent doesn't get to interject one into someone else's payment.
#[test]
fn a_different_player_cannot_activate_a_mana_ability_mid_payment() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let (state, spell, _, _, _, opponent_land, _) = state_with_fixtures();
    let mut engine = Engine::new(state);

    cast(&mut engine, p0, spell);
    drain(&mut engine);

    let err = activate(&mut engine, p1, opponent_land).unwrap_err();
    assert!(matches!(err, ActionError::IllegalAction(_)));
    assert!(matches!(engine.state().object(opponent_land), Some(obj) if !obj.tapped));
    assert!(matches!(drain(&mut engine), StepStatus::AwaitingPayment { player, .. } if player == p0), "outer payment still open, untouched");
}

/// CR 601.2g/605.3a only widens the window for *mana* abilities — a normal
/// activated ability still needs a real priority window, even for the payer.
#[test]
fn a_non_mana_ability_cannot_be_activated_mid_payment() {
    let p0 = PlayerId(0);
    let (state, spell, _, _, cleric, ..) = state_with_fixtures();
    let mut engine = Engine::new(state);

    cast(&mut engine, p0, spell);
    drain(&mut engine);

    let err = activate(&mut engine, p0, cleric).unwrap_err();
    assert!(matches!(err, ActionError::IllegalAction(_)));
    assert_eq!(engine.state().player(p0).unwrap().life, 20, "gain-life ability never ran");
    assert!(matches!(drain(&mut engine), StepStatus::AwaitingPayment { player, .. } if player == p0), "outer payment still open, untouched");
}

/// Regression: casting a second spell mid-payment is still illegal - this
/// slice only widens the exception to mana abilities, nothing else.
#[test]
fn casting_a_second_spell_mid_payment_is_still_illegal() {
    let p0 = PlayerId(0);
    let (mut state, spell, ..) = state_with_fixtures();
    let second_spell = ObjectId(6);
    state.objects.insert(second_spell, card_object(second_spell, spell_definition(ManaCost { symbols: Vec::new() }), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(second_spell);
    let mut engine = Engine::new(state);

    cast(&mut engine, p0, spell);
    drain(&mut engine);

    let err = engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::CastSpell {
                card: second_spell,
                modes: Vec::new(),
                targets: Vec::new(),
                x_value: None,
                alternative_cost: None,
                pay_buyback: false,
            },
        })
        .unwrap_err();
    assert!(matches!(err, ActionError::IllegalAction(_)));
}

/// A real-cost (Treasure-style) mana ability activated mid-payment pauses on
/// its own nested `AwaitingPayment` instead of resolving instantly - paying
/// *that* cost (sacrificing the Treasure) then correctly resumes the outer
/// spell's own payment, which can then still be completed.
#[test]
fn a_real_cost_mana_ability_activated_mid_payment_nests_and_resumes_the_outer_payment() {
    let p0 = PlayerId(0);
    let (state, spell, land1, land2, _, _, treasure) = state_with_fixtures();
    let mut engine = Engine::new(state);

    cast(&mut engine, p0, spell);
    drain(&mut engine);

    activate(&mut engine, p0, treasure).expect("a mana ability, even a real-cost one, may be activated mid-payment");
    let status = drain(&mut engine);
    assert!(
        matches!(status, StepStatus::AwaitingPayment { player, .. } if player == p0),
        "the Treasure's own Sacrifice cost must pause too, got {status:?}"
    );
    assert!(engine.state().object(treasure).is_some(), "not sacrificed until its own payment commits");

    engine
        .submit_payment(PaymentPlan { payments: vec![CostPayment::Sacrifice(treasure)] })
        .expect("sacrificing the Treasure pays its own cost");
    let status = drain(&mut engine);
    assert!(
        matches!(status, StepStatus::AwaitingPayment { player, .. } if player == p0),
        "outer spell payment resumed after the nested one settled, got {status:?}"
    );
    assert!(engine.state().object(treasure).is_none(), "the Treasure is gone");
    assert_eq!(engine.state().player(p0).unwrap().mana_pool.blue, 1, "its mana was added, even though the outer {{R}}{{R}} cost can't use it");

    // Finish paying the outer {R}{R} cost with both Mountains - the
    // Treasure's blue mana has no generic slot to cover in a pure-{R}{R}
    // cost, so it's left floating unspent, same as any other excess mana a
    // player produces but doesn't need.
    activate(&mut engine, p0, land1).expect("still mid the outer payment, still the payer's own mana ability");
    drain(&mut engine);
    activate(&mut engine, p0, land2).expect("still mid the outer payment");
    drain(&mut engine);
    engine
        .submit_payment(PaymentPlan {
            payments: vec![CostPayment::Mana { units: vec![ManaUnit::Colored(Color::Red), ManaUnit::Colored(Color::Red)], phyrexian_life: 0 }],
        })
        .expect("both Mountains cover the {R}{R} cost");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert_eq!(engine.state().player(p0).unwrap().mana_pool.blue, 1, "the Treasure's mana is still floating, untouched");
}

/// Declining to pay a nested real-cost mana ability's own cost
/// (`unwind_payment`) must resume the outer spell's payment rather than
/// abandoning it - the player can still pay a different way.
#[test]
fn unwinding_a_nested_payment_resumes_the_outer_payment() {
    let p0 = PlayerId(0);
    let (state, spell, land1, land2, _, _, treasure) = state_with_fixtures();
    let mut engine = Engine::new(state);

    cast(&mut engine, p0, spell);
    drain(&mut engine);

    activate(&mut engine, p0, treasure).expect("mana ability, legal mid-payment");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { player, .. } if player == p0));

    engine.unwind_payment().expect("declining the Treasure's own cost");
    let status = drain(&mut engine);
    assert!(
        matches!(status, StepStatus::AwaitingPayment { player, .. } if player == p0),
        "outer spell payment resumed, not abandoned, got {status:?}"
    );
    assert!(engine.state().object(treasure).is_some(), "the Treasure survives - its activation was undone, not committed");

    // Pay the outer cost a different way instead, proving the outer pause
    // genuinely survived intact.
    activate(&mut engine, p0, land1).expect("still mid the outer payment");
    drain(&mut engine);
    activate(&mut engine, p0, land2).expect("still mid the outer payment");
    drain(&mut engine);
    engine
        .submit_payment(PaymentPlan {
            payments: vec![CostPayment::Mana { units: vec![ManaUnit::Colored(Color::Red), ManaUnit::Colored(Color::Red)], phyrexian_life: 0 }],
        })
        .expect("both Mountains cover the {R}{R} cost");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
}

/// `submit_payment` (the sub-decision entry point) still rejects outright
/// when nothing is pending - unaffected by this slice.
#[test]
fn submit_payment_while_not_awaiting_is_still_rejected() {
    let (state, ..) = state_with_fixtures();
    let mut engine = Engine::new(state);
    let err = engine.submit_payment(PaymentPlan { payments: Vec::new() }).unwrap_err();
    assert!(matches!(err, PaymentError::NotAwaitingPayment));
}
