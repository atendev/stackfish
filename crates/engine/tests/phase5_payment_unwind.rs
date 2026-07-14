//! Phase 5 vertical slice: backlog item 15 — CR 601.6 payment unwind.
//! `Engine::submit_payment` rejecting an invalid `PaymentPlan` already left
//! `AwaitingPayment` untouched so the caller could retry - but there was no
//! way to declare "I cannot or will not pay" and get out. If a cost has no
//! valid plan at all, the engine was stuck in `AwaitingPayment` forever: the
//! card had already moved hand->stack (CR 601.2a happens before payment),
//! and nothing returned it. `Engine::unwind_payment` is the new explicit
//! entry point for this - reuses `move_resolving_spell` (the same
//! Stack-departure helper CR 608.2b's fizzle path already uses) for a
//! `Spell`, and just removes the stack entry for an `ActivatedAbility` (its
//! source permanent never left the battlefield).

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
    }
}

fn permanent_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 2,
        name: "Test Permanent".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::ARTIFACT,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_permanent".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        effect: None,
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
        visibility: VisibilityPolicy { public: true, visible_to: Vec::new() },
    }
}

fn empty_pool() -> ManaPool {
    ManaPool::default()
}

/// One player, main phase, empty stack - `abilities` starts empty; the
/// activated-ability test pushes its own entry after construction.
fn base_state() -> GameState {
    let p0 = PlayerId(0);
    GameState {
        players: vec![PlayerState {
            player_id: p0,
            life: 20,
            mana_pool: empty_pool(),
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
        step: Step::PreCombatMain,
        priority_player: p0,
        consecutive_passes: 0,
        lands_played_this_turn: HashMap::new(),
        combat: None,
        last_active_turn_number: HashMap::from([(p0, 1)]),
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
        next_object_id: 1000,
        pending_step_skips: Vec::new(),
        once_per_turn_triggered: std::collections::HashSet::new(),
    }
}

fn drain(engine: &mut Engine) -> StepStatus {
    loop {
        match engine.step().status {
            StepStatus::Progressed => continue,
            other => return other,
        }
    }
}

fn cast_and_await_payment(engine: &mut Engine, player: PlayerId, card: ObjectId) -> ObjectId {
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
        .expect("player holds priority, spell is in hand, main phase, empty stack");
    match drain(engine) {
        StepStatus::AwaitingPayment { player: p, stack_id } if p == player => stack_id,
        other => panic!("expected AwaitingPayment, got {other:?}"),
    }
}

// ---------------------------------------------------------------------
// Unwinding a cast spell.
// ---------------------------------------------------------------------

#[test]
fn unwinding_a_cast_spell_returns_the_card_to_hand() {
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    let mana_cost = ManaCost::parse("{1}").unwrap();
    let mut state = base_state();
    state.objects.insert(card, card_object(card, spell_definition(mana_cost), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    let stack_id = cast_and_await_payment(&mut engine, p0, card);
    assert!(engine.state().stack.iter().any(|s| s.stack_id == stack_id));

    engine.unwind_payment().expect("a real AwaitingPayment pause must accept an unwind");

    assert!(engine.state().stack.is_empty(), "the stack object is gone");
    assert!(engine.state().object(stack_id).is_none(), "the stack-side id is retired");
    let hand = engine.state().zone(ZoneKind::Hand, Some(p0)).unwrap();
    assert_eq!(hand.contents.len(), 1, "the card is back in hand under a new id");
    let new_id = hand.contents[0];
    assert_ne!(new_id, stack_id, "CR 400.7: a new object id, not the old stack id");
    assert_eq!(engine.state().object(new_id).unwrap().zone, ZoneKind::Hand);

    let tail: Vec<&GameEventKind> = engine.state().ledger.iter().rev().take(2).map(|e| &e.kind).collect();
    assert!(
        matches!(tail[0], GameEventKind::CastingUnwound { stack_id: s } if *s == stack_id),
        "expected CastingUnwound last, got {:?}",
        tail[0]
    );
    assert!(
        matches!(tail[1], GameEventKind::ZoneChange { object, from: ZoneKind::Stack, to: ZoneKind::Hand, .. } if *object == new_id),
        "expected the accompanying ZoneChange just before it, got {:?}",
        tail[1]
    );
}

#[test]
fn priority_reopens_for_the_same_player_after_unwinding() {
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    let mana_cost = ManaCost::parse("{1}").unwrap();
    let mut state = base_state();
    state.objects.insert(card, card_object(card, spell_definition(mana_cost), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_and_await_payment(&mut engine, p0, card);

    // Confirm submit_payment's existing retry path is unaffected: an
    // invalid plan still just errors, still AwaitingPayment, before the
    // caller decides to unwind instead.
    let err = engine.submit_payment(PaymentPlan { payments: Vec::new() }).unwrap_err();
    assert!(matches!(err, PaymentError::InvalidPlan));
    assert!(matches!(engine.step().status, StepStatus::AwaitingPayment { .. }));

    engine.unwind_payment().expect("must accept the unwind after a rejected plan too");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
}

// ---------------------------------------------------------------------
// Unwinding an activated ability - no zone move, source stays put.
// ---------------------------------------------------------------------

#[test]
fn unwinding_an_activated_ability_removes_it_without_a_zone_move() {
    let p0 = PlayerId(0);
    let source = ObjectId(0);
    let mut state = base_state();
    state.objects.insert(source, card_object(source, permanent_definition(), p0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(source);
    state.abilities.push(RegisteredAbility {
        source,
        kind: AbilityKind::Activated,
        condition: None,
        cost: Some(CostSpec { mana: ManaCost::parse("{1}").ok(), additional: Vec::new(), alternative: Vec::new() }),
        target: None,
        operation: ScriptHandle(0),
        effect: Some(AbilityEffect::GainLife(1)),
        intervening_if: None,
    });

    let mut engine = Engine::new(state);
    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::ActivateAbility { source, ability_index: 0, targets: Vec::new() },
        })
        .expect("player holds priority");
    let stack_id = match drain(&mut engine) {
        StepStatus::AwaitingPayment { player, stack_id } if player == p0 => stack_id,
        other => panic!("expected AwaitingPayment, got {other:?}"),
    };

    engine.unwind_payment().expect("a real AwaitingPayment pause must accept an unwind");

    assert!(engine.state().stack.is_empty(), "the ability's stack entry is gone");
    // The source permanent itself never left the battlefield - same id,
    // same zone, completely untouched.
    assert!(engine.state().object(source).is_some(), "the source permanent is untouched");
    assert_eq!(engine.state().object(source).unwrap().zone, ZoneKind::Battlefield);

    let last = &engine.state().ledger.last().unwrap().kind;
    assert!(
        matches!(last, GameEventKind::CastingUnwound { stack_id: s } if *s == stack_id),
        "expected CastingUnwound as the only new ledger event, got {last:?}"
    );
    assert!(
        !matches!(last, GameEventKind::ZoneChange { .. }),
        "an ability's unwind must not produce a ZoneChange - nothing moved"
    );
}

// ---------------------------------------------------------------------
// Rejection when not awaiting payment.
// ---------------------------------------------------------------------

#[test]
fn unwind_payment_rejects_when_not_awaiting() {
    let mut engine = Engine::new(base_state());
    let err = engine.unwind_payment().unwrap_err();
    assert!(matches!(err, PaymentError::NotAwaitingPayment));
}
