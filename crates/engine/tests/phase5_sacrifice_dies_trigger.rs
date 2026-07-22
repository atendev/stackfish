//! Phase 5 vertical slice: extends the CR 603.10 last-known-information
//! stash (`phase5_dies_triggers.rs`) from the SBA-driven death path to
//! `commands::move_to_graveyard_for_cost` (Sacrifice/Discard cost payment,
//! `commit_payment` -> `Engine::submit_payment`). A "when this dies" ability
//! on a sacrificed permanent now fires the same way it does off a combat
//! death - same mechanism (`Engine::departed_triggered_abilities`), a
//! different producer.
//!
//! Discard's own half of `move_to_graveyard_for_cost` is exercised for
//! regression only (no behavior change expected - no `ConditionPrimitive`
//! shape matches a Hand-departure LKI yet, see that function's own doc
//! comment).

use std::collections::{HashMap, VecDeque};

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, two_player_state as base_two_player_state};

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
        effect: None,
        modes: Vec::new(),
    }
}

fn object(id: ObjectId, definition: CardDefinition, owner: PlayerId, zone: ZoneKind) -> GameObject {
    card_object(id, definition, owner, zone)
}

fn sacrifice_cost() -> CostSpec {
    CostSpec {
        mana: None,
        additional: vec![AdditionalCost::Sacrifice {
            filter: FilterSpec { card_types: Some(CardTypeSet::CREATURE), supertypes: None, subtypes: None, controller: None, random: false, extra: None },
        }],
        alternative: Vec::new(),
    }
}

fn discard_cost() -> CostSpec {
    CostSpec {
        mana: None,
        additional: vec![AdditionalCost::Discard {
            filter: FilterSpec { card_types: Some(CardTypeSet::CREATURE), supertypes: None, subtypes: None, controller: None, random: false, extra: None },
        }],
        alternative: Vec::new(),
    }
}

fn dies_ability(source: ObjectId) -> RegisteredAbility {
    RegisteredAbility { id: AbilityId(0),
        source,
        kind: AbilityKind::Triggered,
        condition: Some(ConditionSpec::Primitive(ConditionPrimitive::Dies)),
        cost: None,
        target: None,
        operation: ScriptHandle(0),
        effect: Some(AbilityEffect::GainLife(1)),
        intervening_if: None,
    }
}

/// Two players; `source` is a battlefield artifact with the given cost and
/// no effect of its own (so any life-total change observed can only have
/// come from `fodder`'s Dies trigger); `fodder` is a battlefield creature
/// with a Dies trigger; `hand_card` is a hand creature for the Discard
/// regression test.
fn state_with_cost(cost: CostSpec) -> (GameState, PlayerId, PlayerId, ObjectId, ObjectId, ObjectId) {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let source = ObjectId(0);
    let fodder = ObjectId(1);
    let hand_card = ObjectId(2);

    let mut objects = HashMap::new();
    objects.insert(source, object(source, vanilla_definition("Source", CardTypeSet::ARTIFACT), p0, ZoneKind::Battlefield));
    objects.insert(fodder, object(fodder, vanilla_definition("Fodder", CardTypeSet::CREATURE), p0, ZoneKind::Battlefield));
    objects.insert(hand_card, object(hand_card, vanilla_definition("Hand Card", CardTypeSet::CREATURE), p0, ZoneKind::Hand));

    let state = GameState {
        last_active_turn_number: HashMap::from([(p0, 1), (p1, 1)]),
        objects,
        zones: vec![
            Zone { kind: ZoneKind::Battlefield, owner: None, contents: VecDeque::from([source, fodder]) },
            Zone { kind: ZoneKind::Hand, owner: Some(p0), contents: VecDeque::from([hand_card]) },
            Zone::new(ZoneKind::Graveyard, Some(p0)),
            Zone::new(ZoneKind::Graveyard, Some(p1)),
        ],
        abilities: vec![
            RegisteredAbility { id: AbilityId(1),
                source,
                kind: AbilityKind::Activated,
                condition: None,
                cost: Some(cost),
                target: None,
                operation: ScriptHandle(0),
                effect: None,
                intervening_if: None,
            },
            dies_ability(fodder),
        ],
        ..base_two_player_state(Vec::new())
    };
    (state, p0, p1, source, fodder, hand_card)
}

fn pass_both(engine: &mut Engine, p0: PlayerId, p1: PlayerId) -> StepStatus {
    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine);
    engine.submit_action(PlayerAction { player: p1, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine)
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
            },
        })
        .expect("player holds priority");
    let status = drain(engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }), "expected a pause for the ability's cost, got {status:?}");
}

#[test]
fn dies_trigger_fires_and_resolves_after_a_sacrifice_cost_payment() {
    let (state, p0, p1, source, fodder, _hand_card) = state_with_cost(sacrifice_cost());
    let mut engine = Engine::new(state);

    activate_and_await_payment(&mut engine, p0, source);
    engine
        .submit_payment(PaymentPlan { payments: vec![CostPayment::Sacrifice(fodder)] })
        .expect("a legal Sacrifice(fodder) payment pays a Sacrifice{Creature} cost");
    let status = drain(&mut engine);

    assert!(
        matches!(status, StepStatus::AwaitingAction { player } if player == p0),
        "activator keeps priority after paying (CR 601.2i-equivalent for abilities): {status:?}"
    );
    assert!(engine.state().object(fodder).is_none(), "fodder is gone - sacrificed to the graveyard");
    assert_eq!(
        engine.state().stack.len(),
        2,
        "the activated ability itself plus fodder's Dies trigger, collected and pushed on top"
    );

    // Both players pass - the top of the stack (the Dies trigger, put there
    // after the activated ability) resolves first.
    let status = pass_both(&mut engine, p0, p1);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert_eq!(engine.state().stack.len(), 1, "the Dies trigger resolved, the activated ability remains");
    assert_eq!(engine.state().player(p0).unwrap().life, 21, "GainLife(1) applied from fodder's Dies trigger");
}

#[test]
fn dies_trigger_does_not_fire_for_a_permanent_that_was_not_sacrificed() {
    // A creature that survives on the battlefield never leaves it, so its
    // own Dies trigger must not fire just because *some* sacrifice happened
    // elsewhere in the same payment.
    let (mut state, p0, p1, source, fodder, _hand_card) = state_with_cost(sacrifice_cost());
    let bystander = ObjectId(3);
    state.objects.insert(bystander, object(bystander, vanilla_definition("Bystander", CardTypeSet::CREATURE), p0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(bystander);
    state.abilities.push(dies_ability(bystander));

    let mut engine = Engine::new(state);
    activate_and_await_payment(&mut engine, p0, source);
    engine
        .submit_payment(PaymentPlan { payments: vec![CostPayment::Sacrifice(fodder)] })
        .expect("a legal Sacrifice(fodder) payment pays a Sacrifice{Creature} cost");
    drain(&mut engine);

    assert_eq!(engine.state().stack.len(), 2, "only fodder's Dies trigger fired, not bystander's");
    assert!(engine.state().object(bystander).is_some(), "bystander is untouched, still on the battlefield");
}

#[test]
fn discard_cost_captures_departed_abilities_without_a_behavior_change() {
    // Regression: Discard's half of move_to_graveyard_for_cost captures the
    // departed abilities the same way Sacrifice's does, but stays dormant -
    // no ConditionPrimitive matches a Hand-departure LKI yet, so a Dies
    // ability on a discarded card must not fire.
    let (state, p0, p1, source, _fodder, hand_card) = state_with_cost(discard_cost());
    let mut engine = Engine::new(state);
    // dies_ability was registered against `fodder` in state_with_cost, not
    // `hand_card` - register one against hand_card directly so this test
    // proves the discard path itself, not an absent registration.
    engine.state_mut().abilities.push(dies_ability(hand_card));

    activate_and_await_payment(&mut engine, p0, source);
    engine
        .submit_payment(PaymentPlan { payments: vec![CostPayment::Discard(hand_card)] })
        .expect("a legal Discard(hand_card) payment pays a Discard{Creature} cost");
    let status = drain(&mut engine);

    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert!(engine.state().object(hand_card).is_none(), "hand_card discarded to the graveyard");
    assert_eq!(
        engine.state().stack.len(),
        1,
        "only the activated ability itself is on the stack - Dies doesn't fire off a Hand departure"
    );
    assert_eq!(engine.state().player(p0).unwrap().life, 20, "no GainLife - the Dies condition never matched");
}
