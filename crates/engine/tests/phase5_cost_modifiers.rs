//! Phase 5 vertical slice: cost modification (CR 601.2f/118.9) — a static
//! "spells [you cast/of some type] cost `{N}` more/less to cast" effect
//! (Goblin Warchief, Thalia). Deliberately not a `ContinuousEffect`/CR 613
//! layer — cost modification is a one-time adjustment applied while
//! determining a spell's total cost, not a characteristic query. See
//! `CostModifier`'s own doc comment (cost.rs) and `ManaCost::apply_cost_delta`
//! (mana.rs). Scoped to spells only, flat fixed amounts only, generic
//! component only — same narrowing precedent `{X}`/alternative-costs both
//! already established for their own slices.

use std::collections::{HashMap, VecDeque};

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::*;

fn spell_definition(mana_cost: ManaCost, card_types: CardTypeSet) -> CardDefinition {
    CardDefinition {
        definition_id: 1,
        name: "Test Spell".to_string(),
        mana_cost,
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types,
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

fn two_player_state(pool: ManaPool, cost_modifiers: Vec<CostModifier>) -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        players: vec![
            PlayerState {
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
                commander_casts: std::collections::HashMap::new(),
            },
            PlayerState {
                player_id: p1,
                life: 20,
                mana_pool: ManaPool::default(),
                counters: CounterSet::new(),
                emblems: Vec::new(),
                commander_damage_taken: Vec::new(),
                eliminated: false,
                drew_from_empty_library: false,
                mulligans_taken: 0,
                hand_kept: false,
                commander_casts: std::collections::HashMap::new(),
            },
        ],
        turn_order: vec![p0, p1],
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
            Zone { kind: ZoneKind::Hand, owner: Some(p1), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Battlefield, owner: None, contents: VecDeque::new() },
            Zone { kind: ZoneKind::Graveyard, owner: Some(p0), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Graveyard, owner: Some(p1), contents: VecDeque::new() },
        ],
        stack: Vec::new(),
        continuous_effects: Vec::new(),
        replacement_effects: Vec::new(),
        cost_modifiers,
        abilities: Vec::new(),
        ledger: Vec::new(),
        rng: RngState { seed: 1, draws_consumed: 0 },
        next_event_id: 0,
        next_sequence_no: 0,
        next_object_id: 1000,
        pending_step_skips: Vec::new(),
        once_per_turn_triggered: std::collections::HashSet::new(),
        commander_return_offered: std::collections::HashSet::new(),
    }
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

fn cast_spell(engine: &mut Engine, player: PlayerId, card: ObjectId) {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::CastSpell {
                card,
                modes: Vec::new(),
                targets: Vec::new(),
                x_value: None,
                alternative_cost: None,
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
    };
    let increase = CostModifier {
        source: ObjectId(99),
        controller: p0,
        filter: FilterSpec { card_types: None, supertypes: None, subtypes: None, controller: None, random: false, extra: None },
        amount: 2,
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
