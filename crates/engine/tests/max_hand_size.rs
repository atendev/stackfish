//! CR 402.2: max hand size consults live `HandSizeModifier`s and not a hardcoded 7. It is not a CR 613 layer, as with `CostModifier`, and every test builds the modifier by struct literal.

use std::collections::HashMap;

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::*;
use stackfish_testutil::{card_object, zones, two_player_state as base_two_player_state};

fn vanilla_card(id: ObjectId, owner: PlayerId) -> GameObject {
    let definition = CardDefinition {
        definition_id: id.0,
        name: "Test Card".to_string(),
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
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    };
    GameObject { controlled_since_turn: 1, ..card_object(id, definition, owner, ZoneKind::Hand) }
}

fn state_at_end_step() -> GameState {
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

fn add_cards_to_hand(state: &mut GameState, player: PlayerId, count: usize) {
    for i in 0..count {
        let id = ObjectId(100 + i as u64);
        state.objects.insert(id, vanilla_card(id, player));
        state.zone_mut(ZoneKind::Hand, Some(player)).unwrap().push_bottom(id);
    }
}

fn step_until_pause(engine: &mut Engine) -> StepStatus {
    loop {
        let r = engine.step();
        if !matches!(r.status, StepStatus::Progressed) {
            return r.status;
        }
    }
}

fn pass(engine: &mut Engine, player: PlayerId) {
    engine.submit_action(PlayerAction { player, kind: PlayerActionKind::PassPriority }).expect("PassPriority should be accepted");
}

fn pass_through_end_step(engine: &mut Engine, p0: PlayerId, p1: PlayerId) -> StepStatus {
    let s = step_until_pause(engine);
    assert!(matches!(s, StepStatus::AwaitingAction { player } if player == p0), "expected p0 end-step priority, got {s:?}");
    pass(engine, p0);
    let s = step_until_pause(engine);
    assert!(matches!(s, StepStatus::AwaitingAction { player } if player == p1), "expected p1 end-step priority, got {s:?}");
    pass(engine, p1);
    step_until_pause(engine)
}

#[test]
fn no_maximum_hand_size_modifier_suppresses_the_cleanup_discard() {
    let mut state = state_at_end_step();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let source = ObjectId(0);
    add_cards_to_hand(&mut state, p0, 20);
    state.hand_size_modifiers.push(HandSizeModifier {
        source,
        controller: p0,
        affected: HandSizeAffected::Filter(FilterController::You),
        effect: HandSizeEffect::NoMaximum,
        timestamp: SequenceNo(0),
    });

    let mut engine = Engine::new(state);
    let status = pass_through_end_step(&mut engine, p0, p1);

    assert!(
        matches!(status, StepStatus::AwaitingAction { player } if player == p1),
        "no maximum hand size - cleanup should skip straight past any discard pause, got {status:?}"
    );
    assert_eq!(engine.state().phase, Phase::Beginning);
    assert_eq!(engine.state().step, Step::Upkeep);
}

#[test]
fn set_to_modifier_changes_the_discard_threshold() {
    let mut state = state_at_end_step();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let source = ObjectId(0);
    add_cards_to_hand(&mut state, p0, 12);
    state.hand_size_modifiers.push(HandSizeModifier {
        source,
        controller: p0,
        affected: HandSizeAffected::Filter(FilterController::You),
        effect: HandSizeEffect::SetTo(10),
        timestamp: SequenceNo(0),
    });

    let mut engine = Engine::new(state);
    let status = pass_through_end_step(&mut engine, p0, p1);

    assert!(
        matches!(status, StepStatus::AwaitingDiscard { player, hand_size: 12, max_hand_size: 10 } if player == p0),
        "expected AwaitingDiscard against the modified threshold of 10, got {status:?}"
    );
}

#[test]
fn two_conflicting_modifiers_resolve_by_latest_timestamp() {
    let mut state = state_at_end_step();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let source = ObjectId(0);
    add_cards_to_hand(&mut state, p0, 12);
    // SetTo(10) applies first and NoMaximum later, and the later timestamp wins outright.
    state.hand_size_modifiers.push(HandSizeModifier {
        source,
        controller: p0,
        affected: HandSizeAffected::Filter(FilterController::You),
        effect: HandSizeEffect::SetTo(10),
        timestamp: SequenceNo(0),
    });
    state.hand_size_modifiers.push(HandSizeModifier {
        source,
        controller: p0,
        affected: HandSizeAffected::Filter(FilterController::You),
        effect: HandSizeEffect::NoMaximum,
        timestamp: SequenceNo(1),
    });

    let mut engine = Engine::new(state);
    let status = pass_through_end_step(&mut engine, p0, p1);

    assert!(
        matches!(status, StepStatus::AwaitingAction { player } if player == p1),
        "the later NoMaximum modifier should win over the earlier SetTo(10), got {status:?}"
    );
}

#[test]
fn opponent_scoped_modifier_does_not_affect_its_own_controller() {
    let mut state = state_at_end_step();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let source = ObjectId(0);
    add_cards_to_hand(&mut state, p0, 12);
    // "Your opponents have no maximum hand size": p0's own cleanup must be unaffected by its own Opponent-scoped effect.
    state.hand_size_modifiers.push(HandSizeModifier {
        source,
        controller: p0,
        affected: HandSizeAffected::Filter(FilterController::Opponent),
        effect: HandSizeEffect::NoMaximum,
        timestamp: SequenceNo(0),
    });

    let mut engine = Engine::new(state);
    let status = pass_through_end_step(&mut engine, p0, p1);

    assert!(
        matches!(status, StepStatus::AwaitingDiscard { player, hand_size: 12, max_hand_size: 7 } if player == p0),
        "an Opponent-scoped modifier must not affect its own controller's cleanup, got {status:?}"
    );
}

// Forces a real retire_object call on the modifier's source.
fn destroy_permanent_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 30,
        name: "Test Destroy Permanent".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::INSTANT,
        subtypes: Vec::new(),
        oracle_text: "Destroy target permanent.".to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_destroy_permanent".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: Some(TargetSpec::Primitive(TargetRequirement {
            filter: TargetFilter::Object { mana_value: None, excluded_card_types: None, excluded_subtypes: None, card_types: None, subtypes: None, supertypes: None, excluded_supertypes: None, controller: None, zone: None },
            count: 1,
            up_to: false, at_least: false,
            exclude_source: false,
        })),
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: Some(AbilityEffect::Destroy),
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

// CR 611.3b
// A static ability's continuous effect applies only while its source is on the battlefield.
#[test]
fn a_filter_scoped_hand_size_modifier_is_purged_once_its_source_leaves_the_battlefield() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let mut state = GameState {
        step: Step::PreCombatMain,
        last_active_turn_number: HashMap::from([(p0, 1), (p1, 1)]),
        ..base_two_player_state(stackfish_testutil::zones(p0, p1, &[ZoneKind::Hand, ZoneKind::Battlefield, ZoneKind::Graveyard]))
    };
    let source = ObjectId(0);
    let source_object = GameObject { controlled_since_turn: 1, ..card_object(source, vanilla_card(source, p0).definition, p0, ZoneKind::Battlefield) };
    state.objects.insert(source, source_object);
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(source);
    state.hand_size_modifiers.push(HandSizeModifier {
        source,
        controller: p0,
        affected: HandSizeAffected::Filter(FilterController::You),
        effect: HandSizeEffect::SetTo(3),
        timestamp: SequenceNo(0),
    });
    let removal = ObjectId(1);
    state.objects.insert(removal, card_object(removal, destroy_permanent_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(removal);

    assert_eq!(effective_max_hand_size(&state, &state.hand_size_modifiers, p0, 7), Some(3), "the modifier is live before its source is destroyed");

    let mut engine = Engine::new(state);
    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::CastSpell { card: removal, modes: Vec::new(), targets: vec![Target::Object(source)], x_value: None, alternative_cost: None, pay_buyback: false },
        })
        .expect("p0 holds priority, spell is in hand");
    let status = step_until_pause(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }));
    engine.submit_payment(PaymentPlan { payments: Vec::new() }).expect("empty plan pays a {0} cost");
    step_until_pause(&mut engine);
    pass(&mut engine, p0);
    step_until_pause(&mut engine);
    pass(&mut engine, p1);
    step_until_pause(&mut engine);

    assert!(engine.state().object(source).is_none(), "the source creature was destroyed");
    assert!(engine.state().hand_size_modifiers.is_empty(), "the modifier must be purged along with its source");
    assert_eq!(
        effective_max_hand_size(engine.state(), &engine.state().hand_size_modifiers, p0, 7),
        Some(7),
        "back to the base maximum hand size"
    );
}
