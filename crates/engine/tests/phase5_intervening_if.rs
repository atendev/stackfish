//! Phase 5: CR 603.4's intervening-if clause ("When/Whenever/At X, if Y, Z"),
//! backlog item 7's next sub-slice. Checked twice, independently: once when
//! the trigger event fires (folded into `triggers::collect_triggers`,
//! alongside but separate from the ability's own trigger-event `condition`),
//! and again the instant it would resolve (`Engine`'s `ResolveStackTop`, CR
//! 608.2a) - if false by then, the ability is removed from the stack and
//! does nothing.
//!
//! `InterveningIfPrimitive::ControllerAtOrBelowLife(n)` is the one concrete
//! shape this slice ships - real, common, and cleanly testable (nothing else
//! needs constructing to flip it, just change life). The resolution-recheck
//! test below is the actual proof this is two independent checks, not one
//! cached result: a response spell changes life *between* the trigger being
//! placed and it resolving.

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

/// "Test Heal: You gain 10 life." - the response spell used to invalidate
/// the intervening-if between trigger-time and resolution.
fn heal_spell_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 2,
        name: "Test Heal".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::INSTANT,
        subtypes: Vec::new(),
        oracle_text: "You gain 10 life.".to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_heal".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        effect: Some(AbilityEffect::GainLife(10)),
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

fn interveningif_upkeep_ability(source: ObjectId, threshold: i32) -> RegisteredAbility {
    RegisteredAbility {
        source,
        kind: AbilityKind::Triggered,
        condition: Some(ConditionSpec::Primitive(ConditionPrimitive::BeginningOfYourUpkeep)),
        cost: None,
        target: None,
        operation: ScriptHandle(0),
        effect: Some(AbilityEffect::GainLife(1)),
        intervening_if: Some(InterveningIfSpec::Primitive(InterveningIfPrimitive::ControllerAtOrBelowLife(threshold))),
    }
}

/// Two-player state starting at `Phase::PreGame`/`Step::Untap`, p0 active,
/// with `p0_life` as p0's starting life - mirrors
/// `phase5_upkeep_triggers::two_player_state`, plus a Hand/Graveyard zone
/// for the response spell.
fn two_player_state(p0_life: i32) -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        players: vec![
            PlayerState {
                player_id: p0,
                life: p0_life,
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
        turn_number: 1,
        phase: Phase::PreGame,
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
        let result = engine.step();
        match result.status {
            StepStatus::Progressed => continue,
            other => return other,
        }
    }
}

fn pass_both(engine: &mut Engine, p0: PlayerId, p1: PlayerId) -> StepStatus {
    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine);
    engine.submit_action(PlayerAction { player: p1, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine)
}

fn cast_spell(engine: &mut Engine, player: PlayerId, card: ObjectId) {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::CastSpell { card, modes: Vec::new(), targets: Vec::new(), x_value: None, alternative_cost: None },
        })
        .expect("player holds priority, spell is in hand");
}

fn pay_free_and_drain(engine: &mut Engine) -> StepStatus {
    let status = drain(engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }), "expected a pause for the cast's (free) cost, got {status:?}");
    engine.submit_payment(PaymentPlan { payments: Vec::new() }).expect("empty plan pays a {0} cost");
    drain(engine)
}

#[test]
fn intervening_if_false_at_trigger_time_never_triggers_at_all() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let permanent = ObjectId(0);
    // Life 10 > threshold 5 - the intervening-if is already false when
    // upkeep begins.
    let mut state = two_player_state(10);
    state.objects.insert(permanent, card_object(permanent, creature_definition("Bear"), p0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(permanent);
    state.abilities.push(interveningif_upkeep_ability(permanent, 5));

    let mut engine = Engine::new(state);
    let status = pass_both(&mut engine, p0, p1);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert_eq!(engine.state().step, Step::Upkeep);
    assert!(engine.state().stack.is_empty(), "intervening-if false at trigger time - never collected at all");
    assert_eq!(engine.state().player(p0).unwrap().life, 10, "no GainLife applied");
}

#[test]
fn intervening_if_true_throughout_resolves_normally() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let permanent = ObjectId(0);
    // Life 5 == threshold 5 - holds ("at or below").
    let mut state = two_player_state(5);
    state.objects.insert(permanent, card_object(permanent, creature_definition("Bear"), p0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(permanent);
    state.abilities.push(interveningif_upkeep_ability(permanent, 5));

    let mut engine = Engine::new(state);
    let status = pass_both(&mut engine, p0, p1);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert_eq!(engine.state().stack.len(), 1, "intervening-if true at trigger time - collected and pushed");

    let status = pass_both(&mut engine, p0, p1);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert!(engine.state().stack.is_empty(), "resolved (still true at resolution too)");
    assert_eq!(engine.state().player(p0).unwrap().life, 6, "GainLife(1) applied");
}

#[test]
fn a_response_spell_invalidating_the_condition_fizzles_the_trigger_at_resolution() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let permanent = ObjectId(0);
    // Life 3 <= threshold 5 - true at trigger time.
    let mut state = two_player_state(3);
    state.objects.insert(permanent, card_object(permanent, creature_definition("Bear"), p0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(permanent);
    state.abilities.push(interveningif_upkeep_ability(permanent, 5));
    let heal = ObjectId(1);
    state.objects.insert(heal, card_object(heal, heal_spell_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(heal);

    let mut engine = Engine::new(state);
    let status = pass_both(&mut engine, p0, p1);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert_eq!(engine.state().stack.len(), 1, "intervening-if true at trigger time - collected and pushed");

    // p0 holds priority with the upkeep trigger on the stack - responds by
    // casting Test Heal, which goes on top (CR 601.2i/608.1: LIFO).
    cast_spell(&mut engine, p0, heal);
    pay_free_and_drain(&mut engine);
    assert_eq!(engine.state().stack.len(), 2, "the heal spell sits on top of the still-pending upkeep trigger");

    // Both pass: the heal spell (top of stack) resolves first, raising life
    // to 13 - now above the threshold, before the trigger underneath it
    // ever gets a chance to resolve.
    let status = pass_both(&mut engine, p0, p1);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert_eq!(engine.state().player(p0).unwrap().life, 13, "Test Heal resolved first");
    assert_eq!(engine.state().stack.len(), 1, "only the upkeep trigger remains");

    // Now the upkeep trigger resolves - CR 608.2a's recheck sees life 13 >
    // threshold 5, so it's removed from the stack and does nothing.
    let status = pass_both(&mut engine, p0, p1);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert!(engine.state().stack.is_empty());
    assert_eq!(
        engine.state().player(p0).unwrap().life,
        13,
        "GainLife(1) did NOT apply - the intervening-if was false at resolution"
    );
    assert!(
        engine.state().ledger.iter().any(|e| matches!(e.kind, GameEventKind::Fizzled { .. })),
        "a Fizzled event was logged for the invalidated trigger"
    );
}
