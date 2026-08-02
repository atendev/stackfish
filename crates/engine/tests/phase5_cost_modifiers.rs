//! Phase 5 vertical slice: cost modification (CR 601.2f/118.9) — a static
//! "spells [you cast/of some type] cost `{N}` more/less to cast" effect
//! (Goblin Warchief, Thalia). Deliberately not a `ContinuousEffect`/CR 613
//! layer — cost modification is a one-time adjustment applied while
//! determining a spell's total cost, not a characteristic query. See
//! `CostModifier`'s own doc comment (cost.rs) and `ManaCost::apply_cost_delta`
//! (mana.rs). Scoped to spells only, flat fixed amounts only — same
//! narrowing precedent `{X}`/alternative-costs both already established for
//! their own slices. CR 118.7b/c/d's colored/colorless-pip-specific
//! reduction is real now too (`phase5/cost-modifier-colored-pip`,
//! `CostModifierTarget`), see the `colored_target_*`/`colorless_target_*`
//! tests below.

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::*;
use stackfish_testutil::{
    card_object, drain, hand_battlefield_graveyard_zones, minimal_card_definition, player_state,
    two_player_state as base_two_player_state,
};

fn spell_definition(mana_cost: ManaCost, card_types: CardTypeSet) -> CardDefinition {
    minimal_card_definition(1, "Test Spell", "test_spell", mana_cost, card_types)
}

/// Two players, `p0`'s mana pool and the game's `cost_modifiers` set per
/// test, plus a graveyard for each player alongside the usual hand/
/// battlefield (some cost-modifier tests move cards through discard/death).
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

/// Test case 1: a single reduction effect lowers the generic component;
/// the colored pip is untouched.
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

/// Test case 2: a single increase effect raises the generic component.
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

/// Test case 3: reduction can't take the generic component below {0} - a
/// {1}{R} spell with a {2} reduction pays {R} only, not negative generic.
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

    // Only 1 red in the pool at all - if the floor didn't hold, there'd be
    // no way to pay a negative-generic cost and this would be rejected.
    let units = vec![ManaUnit::Colored(Color::Red)];
    engine
        .submit_payment(PaymentPlan { payments: vec![CostPayment::Mana { units, phyrexian_life: 0 }] })
        .expect("floored at {R} - the reduction cannot go past {0} generic");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
}

/// Test case 4: a modifier whose filter doesn't match (wrong card type) has
/// no effect - the plain mana cost still applies.
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

/// CR 118.7: a {1}{W}{W} spell with a {W}-targeted {W} reduction pays
/// {1}{W} - the colored pip disappears, the generic component is untouched
/// (contrast with `reduction_lowers_generic_component_only` above, which is
/// exactly the mirror image).
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

/// CR 118.7c: a {1}{W} spell with a {W}-targeted reduction of {2} zeroes the
/// {W} and spills the extra {1} onto the generic component - pays {0}, not
/// a negative-white impossibility.
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

/// CR 118.7b: a {R} spell reduced by a {W}-targeted modifier has no white
/// mana to reduce, so the whole amount redirects to the generic component
/// instead - a cost with no generic component at all still ends up
/// reduced (to {0} generic), not left untouched.
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

    // The {R} pip is completely untouched - the reduction had nowhere to
    // land but generic, and there was no generic component to begin with.
    let units = vec![ManaUnit::Colored(Color::Red)];
    engine
        .submit_payment(PaymentPlan { payments: vec![CostPayment::Mana { units, phyrexian_life: 0 }] })
        .expect("a {W} reduction on an all-red cost redirects to generic, leaving the {R} pip alone");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
}

/// A colored reduction and a plain generic reduction both matching the same
/// spell fold into one net generic delta via colored spillover, same
/// "sum once, don't step" guarantee `two_modifiers_both_apply_in_order`
/// already proves for two Generic-target modifiers.
#[test]
fn colored_and_generic_modifiers_combine_via_spillover() {
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    // {2}{W}: a {W}-targeted reduction of {1} takes the {W} pip cleanly (no
    // spillover), and a separate Generic reduction of {1} lowers {2} to {1}
    // - net result {1}, paid with 1 generic only.
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

/// Test case 5: two modifiers both apply, in registration order - a {2}{R}
/// spell with a {1} reduction and a {1} increase nets back to {2}{R}
/// (proving both were consulted, not just the first).
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

/// Test case 6: a modifier scoped to the opponent's spells (`FilterController::Opponent`
/// relative to the modifier's own controller) doesn't apply when its
/// controller is the one casting.
#[test]
fn modifier_scoped_to_opponent_does_not_apply_to_its_own_controllers_spell() {
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    let mana_cost = ManaCost { symbols: vec![ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::Red))] };
    // "Spells your opponents cast cost {1} more" from p0's perspective -
    // p0 is casting their own spell here, so this should not apply.
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

/// Test case 7: with no cost modifiers registered at all, casting is
/// completely unaffected - regression guard against this slice changing
/// the existing plain-cost path.
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

/// `phase5/cost-modifier-order`: `apply_cost_modifiers` sums every matching
/// modifier's `amount` before flooring at `{0}`, rather than flooring after
/// each one - so a reduction and an increase together must resolve to the
/// same total regardless of which order they're registered in. Base `{1}`,
/// `-2` then `+1`: the pre-fix stepwise behavior gave `{1}` (the reduction
/// floors to `{0}` first, then the increase adds back to `{1}`) for this
/// order and `{0}` for the reverse order - registration order silently
/// changed the resolved cost. Both orders must now resolve to the same
/// `{0}` (net delta `-2 + 1 = -1`, `max(1 - 1, 0) = 0`).
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
