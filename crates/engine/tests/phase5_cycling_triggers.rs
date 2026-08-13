//! Phase 5 vertical slice: CR 702.29c/f — "when you cycle this card"
//! triggered abilities.
//!
//! `ConditionPrimitive::Cycled` (types/ability.rs) is a new self-referential
//! sibling of `Discarded`: unlike `Discarded` (matches *any* hand ->
//! graveyard departure of this card, whatever the cause), `Cycled` matches
//! only `GameEventKind::Cycled`, a purpose-built marker event logged
//! alongside the ordinary `ZoneChange` exclusively when the discard paid a
//! Cycling/Typecycling ability's own `AdditionalCost::DiscardSelf` cost
//! (`Engine::submit_payment`'s `cycling_source` check, threaded through
//! `continue_cost_zone_moves`). CR 702.29f means Typecycling's own
//! `DiscardSelf` cost counts identically - no separate mechanism needed.
//!
//! Reuses the exact CR 603.10 departed/LKI fallback shape `Discarded`
//! itself uses (`triggers.rs::collect_triggers`'s `departed` loop), since
//! the cycled card's own "when you cycle this card" ability is purged from
//! `state.abilities` the moment the card leaves hand, same as any other
//! departure.
//!
//! CR 702.29d ("whenever a player cycles or discards a card") needed no new
//! work at all - `DiscardsCard`/`EachDiscardsCard` already match *any* Hand
//! -> Graveyard `ZoneChange`, and a cycling discard is exactly that shape.
//! `cycles_or_discards_condition_already_fires_for_a_cycling_discard` below
//! proves that coverage directly rather than leaving it assumed.

use std::collections::HashMap;

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, minimal_card_definition, two_player_state as base_two_player_state, zones};

fn cycler_definition(definition_id: u64, name: &str) -> CardDefinition {
    CardDefinition {
        oracle_text: "Cycling {2}".to_string(),
        ..minimal_card_definition(definition_id, name, "test_cycler", ManaCost { symbols: Vec::new() }, CardTypeSet::SORCERY)
    }
}

fn filler_definition(definition_id: u64, name: &str) -> CardDefinition {
    minimal_card_definition(definition_id, name, "test_filler", ManaCost { symbols: Vec::new() }, CardTypeSet::CREATURE)
}

fn basic_land_definition(definition_id: u64, name: &str) -> CardDefinition {
    CardDefinition {
        supertypes: SupertypeSet::BASIC,
        ..minimal_card_definition(definition_id, name, "test_land", ManaCost { symbols: Vec::new() }, CardTypeSet::LAND)
    }
}

fn discard_self_cost() -> CostSpec {
    CostSpec { mana: None, additional: vec![AdditionalCost::DiscardSelf], alternative: Vec::new() }
}

/// The Cycling activated ability itself - same shape `phase5_cycling.rs`
/// already established.
fn cycling_ability(source: ObjectId, effect: AbilityEffect) -> RegisteredAbility {
    RegisteredAbility {
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
        id: AbilityId(0),
        source,
        kind: AbilityKind::Activated,
        condition: None,
        cost: Some(discard_self_cost()),
        target: None,
        operation: ScriptHandle(0),
        effect: Some(effect),
        intervening_if: None,
    }
}

/// "When you cycle this card, you gain 1 life" - a self-referential
/// `Cycled`-conditioned trigger on the same card as its own Cycling
/// ability. `effect: GainLife(1)` is a convenient, easy-to-observe proof the
/// trigger actually resolved.
fn cycled_trigger(source: ObjectId) -> RegisteredAbility {
    RegisteredAbility {
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
        id: AbilityId(1),
        source,
        kind: AbilityKind::Triggered,
        condition: Some(ConditionSpec::Primitive(ConditionPrimitive::Cycled)),
        cost: None,
        target: None,
        operation: ScriptHandle(0),
        effect: Some(AbilityEffect::GainLife(1)),
        intervening_if: None,
    }
}

fn two_player_state() -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        turn_number: 2,
        phase: Phase::Ending,
        step: Step::End,
        last_active_turn_number: HashMap::from([(p0, 2), (p1, 1)]),
        ..base_two_player_state(zones(p0, p1, &[ZoneKind::Library, ZoneKind::Hand, ZoneKind::Graveyard, ZoneKind::Battlefield, ZoneKind::Exile]))
    }
}

fn activate(engine: &mut Engine, player: PlayerId, source: ObjectId) {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::ActivateAbility { source, ability_index: 0, targets: Vec::new(), chosen_colors: Vec::new(),
            x_value: None,
            },
        })
        .expect("player controls source and holds priority");
}

fn pass_both_and_drain(engine: &mut Engine, p0: PlayerId, p1: PlayerId) -> StepStatus {
    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine);
    engine.submit_action(PlayerAction { player: p1, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine)
}

#[test]
fn cycled_trigger_fires_off_a_plain_cycling_activation() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(0);
    let filler = ObjectId(1);

    state.objects.insert(card, card_object(card, cycler_definition(1, "Cycler"), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);
    state.objects.insert(filler, card_object(filler, filler_definition(2, "Filler"), p0, ZoneKind::Library));
    state.zone_mut(ZoneKind::Library, Some(p0)).unwrap().push_bottom(filler);
    state.abilities.push(cycling_ability(card, AbilityEffect::DrawCards(1)));
    state.abilities.push(cycled_trigger(card));

    let mut engine = Engine::new(state);
    activate(&mut engine, p0, card);
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }), "got {status:?}");
    engine.submit_payment(PaymentPlan { payments: vec![CostPayment::Discard(card)] }).expect("discarding the source pays DiscardSelf");
    let status = drain(&mut engine);

    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0), "expected priority reopened by the Cycled trigger, got {status:?}");
    // 2, not 1: the Cycling activated ability itself (pushed on activation,
    // before its cost was even paid) plus the Cycled trigger collected off
    // paying that cost.
    assert_eq!(engine.state().stack.len(), 2, "the Cycling ability plus the Cycled trigger it paid for");

    // Resolve both stack entries (Cycled trigger resolves first, LIFO).
    pass_both_and_drain(&mut engine, p0, p1);
    pass_both_and_drain(&mut engine, p0, p1);
    assert!(engine.state().stack.is_empty());
    assert_eq!(engine.state().player(p0).unwrap().life, 21, "GainLife(1) applied from the Cycled trigger");
    assert_eq!(engine.state().zone(ZoneKind::Hand, Some(p0)).unwrap().contents.len(), 1, "drew the filler card");
}

#[test]
fn cycled_trigger_fires_off_typecycling_too() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(0);
    let forest = ObjectId(1);

    state.objects.insert(card, card_object(card, cycler_definition(1, "Typecycler"), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);
    state.objects.insert(forest, card_object(forest, basic_land_definition(2, "Forest"), p0, ZoneKind::Library));
    state.zone_mut(ZoneKind::Library, Some(p0)).unwrap().push_bottom(forest);
    let search_filter =
        FilterSpec { card_types: Some(CardTypeSet::LAND), supertypes: None, subtypes: None, controller: None, random: false, extra: None };
    state.abilities.push(cycling_ability(
        card,
        AbilityEffect::Search { filter: search_filter, count: 1, destination: SearchDestination::Hand, shuffle_after: true },
    ));
    state.abilities.push(cycled_trigger(card));

    let mut engine = Engine::new(state);
    activate(&mut engine, p0, card);
    drain(&mut engine);
    engine.submit_payment(PaymentPlan { payments: vec![CostPayment::Discard(card)] }).expect("discarding the source pays DiscardSelf");
    let status = drain(&mut engine);

    assert_eq!(engine.state().stack.len(), 2, "the Typecycling ability plus the Cycled trigger it paid for (CR 702.29f)");
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));

    pass_both_and_drain(&mut engine, p0, p1);
    assert_eq!(engine.state().player(p0).unwrap().life, 21, "GainLife(1) applied - Typecycling's DiscardSelf counts as a Cycling cost too");
}

#[test]
fn cycled_trigger_does_not_fire_for_the_cleanup_hand_size_discard() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(0);

    state.objects.insert(card, card_object(card, cycler_definition(1, "Cycler"), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);
    state.abilities.push(cycling_ability(card, AbilityEffect::DrawCards(1)));
    state.abilities.push(cycled_trigger(card));
    // 7 more vanilla cards - hand_size 8, one over the limit.
    for i in 0..7 {
        let id = ObjectId(100 + i);
        state.objects.insert(id, card_object(id, filler_definition(100 + i, "Vanilla"), p0, ZoneKind::Hand));
        state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(id);
    }

    let mut engine = Engine::new(state);
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0), "expected p0 end-step priority, got {status:?}");
    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).unwrap();
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p1), "expected p1 end-step priority, got {status:?}");
    engine.submit_action(PlayerAction { player: p1, kind: PlayerActionKind::PassPriority }).unwrap();
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingDiscard { player, hand_size: 8, max_hand_size: 7 } if player == p0), "got {status:?}");

    // Cleanup-discard the Cycling card itself - a plain discard, not a
    // Cycling activation, so the DiscardSelf-cost cost path never runs.
    engine.submit_discard(vec![card]).expect("valid discard");
    let status = drain(&mut engine);

    assert!(engine.state().stack.is_empty(), "the Cycled trigger must not fire for an ordinary discard");
    assert!(
        matches!(status, StepStatus::AwaitingAction { player } if player == p1),
        "no trigger pending - the turn should advance straight to p1's Upkeep, got {status:?}"
    );
    assert_eq!(engine.state().player(p0).unwrap().life, 20, "no GainLife - the Cycled trigger never fired");
}

/// CR 702.29d: "whenever a player cycles or discards a card" already works
/// off the pre-existing `EachDiscardsCard` condition - no new mechanism
/// needed, since a cycling discard produces the exact same Hand ->
/// Graveyard `ZoneChange` shape any other discard does. This proves that
/// coverage directly rather than leaving it assumed.
#[test]
fn cycles_or_discards_condition_already_fires_for_a_cycling_discard() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(0);
    let watcher = ObjectId(1);

    state.objects.insert(card, card_object(card, cycler_definition(1, "Cycler"), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);
    state.abilities.push(cycling_ability(card, AbilityEffect::DrawCards(1)));

    state.objects.insert(
        watcher,
        GameObject { controlled_since_turn: 1, ..card_object(watcher, filler_definition(2, "Watcher"), p1, ZoneKind::Battlefield) },
    );
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(watcher);
    state.abilities.push(RegisteredAbility {
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
        id: AbilityId(1),
        source: watcher,
        kind: AbilityKind::Triggered,
        condition: Some(ConditionSpec::Primitive(ConditionPrimitive::EachDiscardsCard)),
        cost: None,
        target: None,
        operation: ScriptHandle(0),
        effect: Some(AbilityEffect::GainLife(1)),
        intervening_if: None,
    });

    let mut engine = Engine::new(state);
    activate(&mut engine, p0, card);
    drain(&mut engine);
    engine.submit_payment(PaymentPlan { payments: vec![CostPayment::Discard(card)] }).expect("discarding the source pays DiscardSelf");
    let status = drain(&mut engine);

    assert_eq!(engine.state().stack.len(), 2, "the Cycling ability plus the watcher's EachDiscardsCard trigger");
    assert!(matches!(status, StepStatus::AwaitingAction { .. }), "got {status:?}");

    pass_both_and_drain(&mut engine, p0, p1);
    assert_eq!(engine.state().player(p1).unwrap().life, 21, "the watcher's EachDiscardsCard trigger fired off the cycling discard");
}
