//! CR 601.2f, 118.9: a static "spells cost {N} more or less" effect (Goblin Warchief, Thalia), applied while determining a spell's total cost and not as a CR 613 layer.
//! Colored and colorless pip reductions (CR 118.7b/c/d) use `CostModifierTarget`.

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::*;
use stackfish_testutil::{
    card_object, drain, hand_battlefield_graveyard_zones, minimal_card_definition, player_state,
    two_player_state as base_two_player_state,
};

fn spell_definition(mana_cost: ManaCost, card_types: CardTypeSet) -> CardDefinition {
    minimal_card_definition(1, "Test Spell", "test_spell", mana_cost, card_types)
}

fn two_player_state(pool: ManaPool, cost_modifiers: Vec<CostModifier>) -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        players: vec![PlayerState { mana_pool: pool, ..player_state(p0) }, player_state(p1)],
        cost_modifiers,
        ..base_two_player_state(hand_battlefield_graveyard_zones(p0, p1))
    }
}

fn cast_spell(engine: &mut Engine, player: PlayerId, card: ObjectId) {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::CastSpell {
                card,
                modes: Vec::new(),
                targets: Vec::new(),
                x_value: None,
                alternative_cost: None, pay_buyback: false,
            },
        })
        .expect("player holds priority, spell is in hand");
}

#[test]
fn reduction_lowers_generic_component_only() {
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    let mana_cost = ManaCost {
        symbols: vec![
            ManaSymbol::Atom(AtomicManaSymbol::Generic(2)),
            ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::Red)),
        ],
    };
    let modifier = CostModifier {
        source: ObjectId(99),
        controller: p0,
        filter: FilterSpec { card_types: None, supertypes: None, subtypes: None, controller: None, random: false, extra: None },
        amount: -1,
        target: stackfish_engine::CostModifierTarget::Generic,
    };
    let mut state = two_player_state(ManaPool { red: 1, colorless: 1, ..ManaPool::default() }, vec![modifier]);
    state.objects.insert(card, card_object(card, spell_definition(mana_cost, CardTypeSet::SORCERY), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }));

    // {2}{R} reduced by {1} -> {1}{R}: 1 red + 1 generic pays it exactly.
    let units = vec![ManaUnit::Colored(Color::Red), ManaUnit::Colorless];
    engine
        .submit_payment(PaymentPlan { payments: vec![CostPayment::Mana { units, phyrexian_life: 0 }] })
        .expect("{1}{R} (post-reduction) paid with 1 red + 1 generic");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
}

#[test]
fn increase_raises_generic_component() {
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    let mana_cost = ManaCost { symbols: vec![ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::Red))] };
    let modifier = CostModifier {
        source: ObjectId(99),
        controller: p0,
        filter: FilterSpec { card_types: None, supertypes: None, subtypes: None, controller: None, random: false, extra: None },
        amount: 1,
        target: stackfish_engine::CostModifierTarget::Generic,
    };
    let mut state = two_player_state(ManaPool { red: 1, colorless: 1, ..ManaPool::default() }, vec![modifier]);
    state.objects.insert(card, card_object(card, spell_definition(mana_cost, CardTypeSet::SORCERY), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    drain(&mut engine);

    // {R} increased by {1} -> {1}{R}: needs the extra generic to pay it.
    let units = vec![ManaUnit::Colored(Color::Red), ManaUnit::Colorless];
    engine
        .submit_payment(PaymentPlan { payments: vec![CostPayment::Mana { units, phyrexian_life: 0 }] })
        .expect("{1}{R} (post-increase) paid with 1 red + 1 generic");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
}

// A reduction cannot take the generic component below {0}.
#[test]
fn reduction_floors_at_zero_generic() {
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    let mana_cost = ManaCost {
        symbols: vec![
            ManaSymbol::Atom(AtomicManaSymbol::Generic(1)),
            ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::Red)),
        ],
    };
    let modifier = CostModifier {
        source: ObjectId(99),
        controller: p0,
        filter: FilterSpec { card_types: None, supertypes: None, subtypes: None, controller: None, random: false, extra: None },
        amount: -2,
        target: stackfish_engine::CostModifierTarget::Generic,
    };
    let mut state = two_player_state(ManaPool { red: 1, ..ManaPool::default() }, vec![modifier]);
    state.objects.insert(card, card_object(card, spell_definition(mana_cost, CardTypeSet::SORCERY), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    drain(&mut engine);

    // Only 1 red in the pool, so a missing floor would leave a negative-generic cost unpayable.
    let units = vec![ManaUnit::Colored(Color::Red)];
    engine
        .submit_payment(PaymentPlan { payments: vec![CostPayment::Mana { units, phyrexian_life: 0 }] })
        .expect("floored at {R} - the reduction cannot go past {0} generic");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
}

// A modifier whose filter does not match has no effect.
#[test]
fn nonmatching_filter_has_no_effect() {
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    let mana_cost = ManaCost { symbols: vec![ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::Red))] };
    // Only reduces Creature spells - this is a Sorcery, so it shouldn't apply.
    let modifier = CostModifier {
        source: ObjectId(99),
        controller: p0,
        filter: FilterSpec {
            card_types: Some(CardTypeSet::CREATURE),
            supertypes: None,
            subtypes: None,
            controller: None,
            random: false,
            extra: None,
        },
        amount: -1,
        target: stackfish_engine::CostModifierTarget::Generic,
    };
    let mut state = two_player_state(ManaPool { red: 1, ..ManaPool::default() }, vec![modifier]);
    state.objects.insert(card, card_object(card, spell_definition(mana_cost, CardTypeSet::SORCERY), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    drain(&mut engine);

    // Cost is still the unmodified {R} - pay with 1 red.
    let units = vec![ManaUnit::Colored(Color::Red)];
    engine
        .submit_payment(PaymentPlan { payments: vec![CostPayment::Mana { units, phyrexian_life: 0 }] })
        .expect("cost unaffected - the modifier's filter didn't match a Sorcery");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
}

// CR 118.7
// A {W}-targeted {W} reduction removes the colored pip and leaves generic untouched.
#[test]
fn colored_target_reduces_only_the_matching_pip() {
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    let mana_cost = ManaCost {
        symbols: vec![
            ManaSymbol::Atom(AtomicManaSymbol::Generic(1)),
            ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::White)),
            ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::White)),
        ],
    };
    let modifier = CostModifier {
        source: ObjectId(99),
        controller: p0,
        filter: FilterSpec { card_types: None, supertypes: None, subtypes: None, controller: None, random: false, extra: None },
        target: CostModifierTarget::Colored(Color::White),
        amount: -1,
    };
    let mut state = two_player_state(ManaPool { white: 1, colorless: 1, ..ManaPool::default() }, vec![modifier]);
    state.objects.insert(card, card_object(card, spell_definition(mana_cost, CardTypeSet::SORCERY), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    drain(&mut engine);

    let units = vec![ManaUnit::Colored(Color::White), ManaUnit::Colorless];
    engine
        .submit_payment(PaymentPlan { payments: vec![CostPayment::Mana { units, phyrexian_life: 0 }] })
        .expect("{1}{W} (post-reduction) paid with 1 white + 1 generic");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
}

// CR 118.7c
// A {W}-targeted reduction of {2} zeroes {W} and spills the extra {1} onto generic.
#[test]
fn colored_target_over_reduction_spills_excess_to_generic() {
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    let mana_cost = ManaCost {
        symbols: vec![
            ManaSymbol::Atom(AtomicManaSymbol::Generic(1)),
            ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::White)),
        ],
    };
    let modifier = CostModifier {
        source: ObjectId(99),
        controller: p0,
        filter: FilterSpec { card_types: None, supertypes: None, subtypes: None, controller: None, random: false, extra: None },
        target: CostModifierTarget::Colored(Color::White),
        amount: -2,
    };
    let mut state = two_player_state(ManaPool::default(), vec![modifier]);
    state.objects.insert(card, card_object(card, spell_definition(mana_cost, CardTypeSet::SORCERY), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    drain(&mut engine);

    engine
        .submit_payment(PaymentPlan { payments: Vec::new() })
        .expect("the {W} is zeroed and the excess {1} spills onto generic, reducing it to {0} too");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
}

// CR 118.7b
// With no white to reduce, the amount redirects to generic, so a cost with no generic component still ends reduced.
#[test]
fn colored_target_with_no_matching_color_redirects_to_generic() {
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    let mana_cost = ManaCost { symbols: vec![ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::Red))] };
    let modifier = CostModifier {
        source: ObjectId(99),
        controller: p0,
        filter: FilterSpec { card_types: None, supertypes: None, subtypes: None, controller: None, random: false, extra: None },
        target: CostModifierTarget::Colored(Color::White),
        amount: -1,
    };
    let mut state = two_player_state(ManaPool { red: 1, ..ManaPool::default() }, vec![modifier]);
    state.objects.insert(card, card_object(card, spell_definition(mana_cost, CardTypeSet::SORCERY), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    drain(&mut engine);

    // The {R} pip is untouched: the reduction had nowhere to land but generic, and there was none.
    let units = vec![ManaUnit::Colored(Color::Red)];
    engine
        .submit_payment(PaymentPlan { payments: vec![CostPayment::Mana { units, phyrexian_life: 0 }] })
        .expect("a {W} reduction on an all-red cost redirects to generic, leaving the {R} pip alone");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
}

// A colored reduction and a generic reduction on one spell fold into one net generic delta.
#[test]
fn colored_and_generic_modifiers_combine_via_spillover() {
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    // {2}{W}: the {W}-targeted reduction takes the {W} pip with no spillover, and the generic reduction lowers {2} to {1}, paid with 1 generic.
    let mana_cost = ManaCost {
        symbols: vec![
            ManaSymbol::Atom(AtomicManaSymbol::Generic(2)),
            ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::White)),
        ],
    };
    let colored = CostModifier {
        source: ObjectId(98),
        controller: p0,
        filter: FilterSpec { card_types: None, supertypes: None, subtypes: None, controller: None, random: false, extra: None },
        target: CostModifierTarget::Colored(Color::White),
        amount: -1,
    };
    let generic = CostModifier {
        source: ObjectId(99),
        controller: p0,
        filter: FilterSpec { card_types: None, supertypes: None, subtypes: None, controller: None, random: false, extra: None },
        target: CostModifierTarget::Generic,
        amount: -1,
    };
    let mut state = two_player_state(ManaPool { colorless: 1, ..ManaPool::default() }, vec![colored, generic]);
    state.objects.insert(card, card_object(card, spell_definition(mana_cost, CardTypeSet::SORCERY), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    drain(&mut engine);

    let units = vec![ManaUnit::Colorless];
    engine
        .submit_payment(PaymentPlan { payments: vec![CostPayment::Mana { units, phyrexian_life: 0 }] })
        .expect("{2}{W} minus the {W} pip minus {1} generic = {1}, paid with 1 generic");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
}

// Two modifiers apply in registration order: a {1} reduction and a {1} increase net back to {2}{R}.
#[test]
fn two_modifiers_both_apply_in_order() {
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    let mana_cost = ManaCost {
        symbols: vec![
            ManaSymbol::Atom(AtomicManaSymbol::Generic(2)),
            ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::Red)),
        ],
    };
    let reduce = CostModifier {
        source: ObjectId(98),
        controller: p0,
        filter: FilterSpec { card_types: None, supertypes: None, subtypes: None, controller: None, random: false, extra: None },
        amount: -1,
        target: stackfish_engine::CostModifierTarget::Generic,
    };
    let increase = CostModifier {
        source: ObjectId(99),
        controller: p0,
        filter: FilterSpec { card_types: None, supertypes: None, subtypes: None, controller: None, random: false, extra: None },
        amount: 2,
        target: stackfish_engine::CostModifierTarget::Generic,
    };
    let mut state =
        two_player_state(ManaPool { red: 1, colorless: 3, ..ManaPool::default() }, vec![reduce, increase]);
    state.objects.insert(card, card_object(card, spell_definition(mana_cost, CardTypeSet::SORCERY), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    drain(&mut engine);

    // {2}{R} - {1} + {2} = {3}{R}: 1 red + 3 generic.
    let units =
        vec![ManaUnit::Colored(Color::Red), ManaUnit::Colorless, ManaUnit::Colorless, ManaUnit::Colorless];
    engine
        .submit_payment(PaymentPlan { payments: vec![CostPayment::Mana { units, phyrexian_life: 0 }] })
        .expect("{3}{R} - both modifiers applied in order");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
}

// A modifier scoped to the opponent's spells does not apply when its controller casts.
#[test]
fn modifier_scoped_to_opponent_does_not_apply_to_its_own_controllers_spell() {
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    let mana_cost = ManaCost { symbols: vec![ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::Red))] };
    // "Spells your opponents cast cost {1} more" from p0's view, and p0 casts their own spell.
    let modifier = CostModifier {
        source: ObjectId(99),
        controller: p0,
        filter: FilterSpec {
            card_types: None,
            supertypes: None,
            subtypes: None,
            controller: Some(FilterController::Opponent),
            random: false,
            extra: None,
        },
        amount: 1,
        target: stackfish_engine::CostModifierTarget::Generic,
    };
    let mut state = two_player_state(ManaPool { red: 1, ..ManaPool::default() }, vec![modifier]);
    state.objects.insert(card, card_object(card, spell_definition(mana_cost, CardTypeSet::SORCERY), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    drain(&mut engine);

    let units = vec![ManaUnit::Colored(Color::Red)];
    engine
        .submit_payment(PaymentPlan { payments: vec![CostPayment::Mana { units, phyrexian_life: 0 }] })
        .expect("cost unaffected - p0 is not p0's own opponent");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
}

// With no cost modifiers registered, casting is unaffected.
#[test]
fn no_modifiers_registered_is_unaffected() {
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    let mana_cost = ManaCost { symbols: vec![ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::Red))] };
    let mut state = two_player_state(ManaPool { red: 1, ..ManaPool::default() }, Vec::new());
    state.objects.insert(card, card_object(card, spell_definition(mana_cost, CardTypeSet::SORCERY), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    drain(&mut engine);

    let units = vec![ManaUnit::Colored(Color::Red)];
    engine
        .submit_payment(PaymentPlan { payments: vec![CostPayment::Mana { units, phyrexian_life: 0 }] })
        .expect("plain {R} cost, unaffected by an empty cost_modifiers list");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
}

// apply_cost_modifiers sums every matching amount before flooring at {0}, so base {1} with -2 and +1 resolves to {0} in either registration order.
fn resolves_to_a_free_spell(modifiers: Vec<CostModifier>) {
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    let mana_cost = ManaCost { symbols: vec![ManaSymbol::Atom(AtomicManaSymbol::Generic(1))] };
    let mut state = two_player_state(ManaPool::default(), modifiers);
    state.objects.insert(card, card_object(card, spell_definition(mana_cost, CardTypeSet::SORCERY), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    drain(&mut engine);

    engine
        .submit_payment(PaymentPlan { payments: Vec::new() })
        .expect("net delta -2 + 1 = -1 against base {1}, floored to {0} - must be free regardless of order");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
}

#[test]
fn reduction_then_increase_and_increase_then_reduction_resolve_to_the_same_cost() {
    let reduce =
        CostModifier { source: ObjectId(98), controller: PlayerId(0), filter: FilterSpec { card_types: None, supertypes: None, subtypes: None, controller: None, random: false, extra: None }, target: CostModifierTarget::Generic, amount: -2 };
    let increase =
        CostModifier { source: ObjectId(99), controller: PlayerId(0), filter: FilterSpec { card_types: None, supertypes: None, subtypes: None, controller: None, random: false, extra: None }, target: CostModifierTarget::Generic, amount: 1 };

    resolves_to_a_free_spell(vec![reduce.clone(), increase.clone()]);
    resolves_to_a_free_spell(vec![increase, reduce]);
}
