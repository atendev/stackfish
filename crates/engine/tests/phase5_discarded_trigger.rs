//! Phase 5 vertical slice: `ConditionPrimitive::Discarded` — "when this card
//! is discarded" (the self-referential sibling `DiscardsCard`/
//! `EachDiscardsCard` don't cover, since those are player-relative:
//! "whenever you/a player discards *a* card", not "whenever *this* card is
//! discarded"). Reuses the exact CR 603.10 look-back-in-time mechanism
//! `Dies`/`LeavesBattlefield` already use for a battlefield departure,
//! extended to a hand departure - `triggers.rs::collect_triggers`'s own
//! departed-ability fallback loop needed `Discarded` added alongside
//! `Dies | LeavesBattlefield` to actually reach it (see that loop's own
//! comment).
//!
//! Both real discard sources already stash their departed `Triggered`
//! abilities (closed by the earlier `DiscardsCard` fix) - this proves
//! `Discarded` rides that same plumbing for both: a cost-payment `Discard`
//! and the cleanup hand-size discard.

use std::collections::{HashMap, VecDeque};

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::*;

fn creature_definition(name: &str) -> CardDefinition {
    CardDefinition {
        definition_id: 1,
        name: name.to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::CREATURE,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: Some(PTValue::Fixed(2)),
        toughness: Some(PTValue::Fixed(2)),
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("bear".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        effect: None,
        modes: Vec::new(),
    }
}

fn permanent(id: ObjectId, definition: CardDefinition, owner: PlayerId) -> GameObject {
    GameObject {
        object_id: id,
        copiable_override: None,
        lineage: ObjectLineage { previous: None },
        definition,
        owner,
        controller: owner,
        zone: ZoneKind::Battlefield,
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
        controlled_since_turn: 1,
        is_token: false,
        is_commander: false,
        visibility: VisibilityPolicy { public: true, visible_to: Vec::new() },
    }
}

/// A hand card that carries its own `Discarded` triggered ability - the
/// self-referential shape this slice adds. `effect: GainLife(1)` is a
/// convenient, easy-to-observe proof the trigger actually resolved.
fn hand_card_with_discarded_trigger(id: ObjectId, owner: PlayerId) -> GameObject {
    GameObject {
        object_id: id,
        copiable_override: None,
        lineage: ObjectLineage { previous: None },
        definition: creature_definition("Self-Discard Watcher"),
        owner,
        controller: owner,
        zone: ZoneKind::Hand,
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
        controlled_since_turn: 1,
        is_token: false,
        is_commander: false,
        visibility: VisibilityPolicy { public: true, visible_to: Vec::new() },
    }
}

fn vanilla_hand_card(id: ObjectId, owner: PlayerId) -> GameObject {
    let mut obj = hand_card_with_discarded_trigger(id, owner);
    obj.definition = CardDefinition {
        definition_id: id.0,
        name: "Vanilla".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::CREATURE,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("bear".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        effect: None,
        modes: Vec::new(),
    };
    obj
}

fn discarded_trigger(source: ObjectId) -> RegisteredAbility {
    RegisteredAbility {
        source,
        kind: AbilityKind::Triggered,
        condition: Some(ConditionSpec::Primitive(ConditionPrimitive::Discarded)),
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
        players: vec![
            PlayerState {
                player_id: p0,
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
        turn_number: 2,
        phase: Phase::Ending,
        step: Step::End,
        priority_player: p0,
        consecutive_passes: 0,
        lands_played_this_turn: HashMap::new(),
        combat: None,
        last_active_turn_number: HashMap::from([(p0, 2), (p1, 1)]),
        objects: HashMap::new(),
        zones: vec![
            Zone { kind: ZoneKind::Library, owner: Some(p0), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Hand, owner: Some(p0), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Graveyard, owner: Some(p0), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Library, owner: Some(p1), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Hand, owner: Some(p1), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Graveyard, owner: Some(p1), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Battlefield, owner: None, contents: VecDeque::new() },
            Zone { kind: ZoneKind::Exile, owner: None, contents: VecDeque::new() },
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
        commander_return_offered: std::collections::HashSet::new(),
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

fn pass(engine: &mut Engine, player: PlayerId) {
    engine.submit_action(PlayerAction { player, kind: PlayerActionKind::PassPriority }).expect("PassPriority should be accepted");
}

fn pass_through_end_step(engine: &mut Engine, p0: PlayerId, p1: PlayerId) -> StepStatus {
    let s = drain(engine);
    assert!(matches!(s, StepStatus::AwaitingAction { player } if player == p0), "expected p0 end-step priority, got {s:?}");
    pass(engine, p0);
    let s = drain(engine);
    assert!(matches!(s, StepStatus::AwaitingAction { player } if player == p1), "expected p1 end-step priority, got {s:?}");
    pass(engine, p1);
    drain(engine)
}

fn pass_both_and_drain(engine: &mut Engine, p0: PlayerId, p1: PlayerId) -> StepStatus {
    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine);
    engine.submit_action(PlayerAction { player: p1, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine)
}

#[test]
fn discarded_trigger_fires_off_a_cost_payment_discard() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let source = ObjectId(0);
    let watching_card = ObjectId(1);

    state.objects.insert(source, permanent(source, creature_definition("Sac Outlet"), p0));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(source);
    state.abilities.push(RegisteredAbility {
        source,
        kind: AbilityKind::Activated,
        condition: None,
        cost: Some(CostSpec {
            mana: None,
            additional: vec![AdditionalCost::Discard {
                filter: FilterSpec { card_types: None, supertypes: None, subtypes: None, controller: None, random: false, extra: None },
            }],
            alternative: Vec::new(),
        }),
        target: None,
        operation: ScriptHandle(0),
        effect: None,
        intervening_if: None,
    });
    state.objects.insert(watching_card, hand_card_with_discarded_trigger(watching_card, p0));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(watching_card);
    state.abilities.push(discarded_trigger(watching_card));

    let mut engine = Engine::new(state);
    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::ActivateAbility { source, ability_index: 0, targets: Vec::new(), chosen_colors: Vec::new() },
        })
        .expect("player holds priority");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }), "expected a pause for the Discard cost, got {status:?}");
    engine.submit_payment(PaymentPlan { payments: vec![CostPayment::Discard(watching_card)] }).expect("legal Discard payment");
    let status = drain(&mut engine);

    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0), "expected priority reopened by the Discarded trigger, got {status:?}");
    // 2, not 1: the activated ability itself (pushed on activation, CR
    // 602.2b, before its cost was even paid) plus the Discarded trigger
    // collected off paying that cost - proof the trigger was collected at
    // all, not proof it's the only thing on the stack.
    assert_eq!(engine.state().stack.len(), 2, "the activated ability plus the Discarded trigger it paid for");

    // Resolve both stack entries.
    pass_both_and_drain(&mut engine, p0, p1);
    pass_both_and_drain(&mut engine, p0, p1);
    assert!(engine.state().stack.is_empty());
    assert_eq!(engine.state().player(p0).unwrap().life, 21, "GainLife(1) applied from the Discarded trigger");
}

#[test]
fn discarded_trigger_fires_off_the_cleanup_hand_size_discard() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let watching_card = ObjectId(0);

    state.objects.insert(watching_card, hand_card_with_discarded_trigger(watching_card, p0));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(watching_card);
    state.abilities.push(discarded_trigger(watching_card));
    // 7 more vanilla cards - hand_size 8, one over the limit.
    for i in 0..7 {
        let id = ObjectId(100 + i);
        state.objects.insert(id, vanilla_hand_card(id, p0));
        state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(id);
    }

    let mut engine = Engine::new(state);
    let status = pass_through_end_step(&mut engine, p0, p1);
    assert!(matches!(status, StepStatus::AwaitingDiscard { player, hand_size: 8, max_hand_size: 7 } if player == p0));

    engine.submit_discard(vec![watching_card]).expect("valid discard");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0), "expected priority reopened by the Discarded trigger, got {status:?}");
    assert_eq!(engine.state().stack.len(), 1, "the Discarded trigger was collected and pushed");

    pass_both_and_drain(&mut engine, p0, p1);
    assert_eq!(engine.state().player(p0).unwrap().life, 21, "GainLife(1) applied from the Discarded trigger");
}

#[test]
fn discarded_trigger_does_not_fire_when_a_different_card_is_discarded() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let watching_card = ObjectId(0);
    let other_card = ObjectId(1);

    state.objects.insert(watching_card, hand_card_with_discarded_trigger(watching_card, p0));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(watching_card);
    state.abilities.push(discarded_trigger(watching_card));
    state.objects.insert(other_card, vanilla_hand_card(other_card, p0));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(other_card);
    for i in 0..6 {
        let id = ObjectId(100 + i);
        state.objects.insert(id, vanilla_hand_card(id, p0));
        state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(id);
    }

    let mut engine = Engine::new(state);
    let status = pass_through_end_step(&mut engine, p0, p1);
    assert!(matches!(status, StepStatus::AwaitingDiscard { player, hand_size: 8, max_hand_size: 7 } if player == p0));

    // Discard other_card, not the card carrying the Discarded trigger.
    engine.submit_discard(vec![other_card]).expect("valid discard");
    let status = drain(&mut engine);

    assert!(engine.state().stack.is_empty(), "the Discarded trigger must not fire for a different card");
    assert!(
        matches!(status, StepStatus::AwaitingAction { player } if player == p1),
        "no trigger pending - the turn should advance straight to p1's Upkeep, got {status:?}"
    );
    assert_eq!(engine.state().player(p0).unwrap().life, 20, "no GainLife - the trigger never fired");
}
