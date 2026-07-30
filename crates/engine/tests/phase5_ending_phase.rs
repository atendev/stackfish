use std::collections::HashMap;

use stackfish_engine::engine::{DiscardError, Engine, StepStatus};
use stackfish_engine::*;
use stackfish_testutil::{card_object, zones, two_player_state as base_two_player_state};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn vanilla_card(id: ObjectId, owner: PlayerId) -> (ObjectId, GameObject) {
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
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    };
    let obj = GameObject { controlled_since_turn: 1, ..card_object(id, definition, owner, ZoneKind::Hand) };
    (id, obj)
}

/// Minimal 2-player `GameState` sitting at `(Ending, End)` in `PriorityWindow`
/// for `p0`. `p0` is active player.
fn state_at_end_step() -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        turn_number: 2,
        phase: Phase::Ending,
        step: Step::End,
        last_active_turn_number: HashMap::from([(p0, 2), (p1, 1)]),
        next_object_id: 100,
        ..base_two_player_state(zones(p0, p1, &[ZoneKind::Library, ZoneKind::Hand, ZoneKind::Graveyard, ZoneKind::Command, ZoneKind::Battlefield, ZoneKind::Exile]))
    }
}

/// Adds `count` vanilla cards to `player`'s hand. Returns their `ObjectId`s.
fn add_cards_to_hand(state: &mut GameState, player: PlayerId, count: usize) -> Vec<ObjectId> {
    let mut ids = Vec::with_capacity(count);
    for _ in 0..count {
        let id = ObjectId(state.next_object_id);
        state.next_object_id += 1;
        let (oid, obj) = vanilla_card(id, player);
        state.objects.insert(oid, obj);
        state
            .zones
            .iter_mut()
            .find(|z| z.kind == ZoneKind::Hand && z.owner == Some(player))
            .expect("hand zone")
            .contents
            .push_back(id);
        ids.push(id);
    }
    ids
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
    engine
        .submit_action(PlayerAction { player, kind: PlayerActionKind::PassPriority })
        .expect("PassPriority should be accepted");
}

/// Drives through End step priority for both players, then returns whatever
/// cleanup (or AdvanceTurnStep) produces.
fn pass_through_end_step(engine: &mut Engine) -> StepStatus {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);

    let s = step_until_pause(engine);
    assert!(
        matches!(s, StepStatus::AwaitingAction { player } if player == p0),
        "expected p0 end-step priority, got {s:?}"
    );
    pass(engine, p0);

    let s = step_until_pause(engine);
    assert!(
        matches!(s, StepStatus::AwaitingAction { player } if player == p1),
        "expected p1 end-step priority, got {s:?}"
    );
    pass(engine, p1);

    step_until_pause(engine)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn cleanup_no_discard_needed_skips_priority_and_advances_turn() {
    let mut state = state_at_end_step();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    add_cards_to_hand(&mut state, p0, 7);
    let mut engine = Engine::new(state);

    let status = pass_through_end_step(&mut engine);

    // Turn rotates: p1 is now active at Upkeep.
    assert!(
        matches!(status, StepStatus::AwaitingAction { player } if player == p1),
        "expected p1 Upkeep priority, got {status:?}"
    );
    assert_eq!(engine.state().phase, Phase::Beginning);
    assert_eq!(engine.state().step, Step::Upkeep);
    assert_eq!(engine.state().active_player, p1);
}

#[test]
fn cleanup_zero_cards_in_hand_skips_priority_and_advances_turn() {
    let state = state_at_end_step();
    let p1 = PlayerId(1);
    let mut engine = Engine::new(state);

    let status = pass_through_end_step(&mut engine);

    assert!(
        matches!(status, StepStatus::AwaitingAction { player } if player == p1),
        "expected p1 Upkeep priority, got {status:?}"
    );
    assert_eq!(engine.state().phase, Phase::Beginning);
    assert_eq!(engine.state().step, Step::Upkeep);
}

#[test]
fn cleanup_excess_hand_pauses_for_discard() {
    let mut state = state_at_end_step();
    let p0 = PlayerId(0);
    add_cards_to_hand(&mut state, p0, 9);
    let mut engine = Engine::new(state);

    let status = pass_through_end_step(&mut engine);

    assert!(
        matches!(
            status,
            StepStatus::AwaitingDiscard { player, hand_size: 9, max_hand_size: 7 }
            if player == p0
        ),
        "expected AwaitingDiscard, got {status:?}"
    );
}

#[test]
fn submit_discard_wrong_count_rejected() {
    let mut state = state_at_end_step();
    let p0 = PlayerId(0);
    let ids = add_cards_to_hand(&mut state, p0, 9);
    let mut engine = Engine::new(state);

    pass_through_end_step(&mut engine);

    // Excess = 2, but submit only 1.
    let err = engine.submit_discard(vec![ids[0]]).unwrap_err();
    assert!(
        matches!(err, DiscardError::WrongCount { expected: 2, got: 1 }),
        "expected WrongCount, got {err:?}"
    );
    // Engine still waiting.
    let s = engine.step();
    assert!(matches!(s.status, StepStatus::AwaitingDiscard { .. }));
}

#[test]
fn submit_discard_duplicate_card_rejected() {
    let mut state = state_at_end_step();
    let p0 = PlayerId(0);
    let ids = add_cards_to_hand(&mut state, p0, 9);
    let mut engine = Engine::new(state);

    pass_through_end_step(&mut engine);

    let err = engine.submit_discard(vec![ids[0], ids[0]]).unwrap_err();
    assert!(matches!(err, DiscardError::DuplicateCard), "expected DuplicateCard, got {err:?}");
}

#[test]
fn submit_discard_card_not_in_hand_rejected() {
    let mut state = state_at_end_step();
    let p0 = PlayerId(0);
    add_cards_to_hand(&mut state, p0, 9);
    let mut engine = Engine::new(state);

    pass_through_end_step(&mut engine);

    let err = engine.submit_discard(vec![ObjectId(999), ObjectId(998)]).unwrap_err();
    assert!(matches!(err, DiscardError::CardNotInHand), "expected CardNotInHand, got {err:?}");
}

#[test]
fn submit_discard_valid_moves_cards_to_graveyard_and_advances_turn() {
    let mut state = state_at_end_step();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let ids = add_cards_to_hand(&mut state, p0, 9);
    let mut engine = Engine::new(state);

    pass_through_end_step(&mut engine);

    // Discard 2 (excess = 9 - 7).
    engine.submit_discard(vec![ids[0], ids[1]]).expect("valid discard");

    let status = step_until_pause(&mut engine);

    assert!(
        matches!(status, StepStatus::AwaitingAction { player } if player == p1),
        "expected p1 Upkeep, got {status:?}"
    );
    assert_eq!(engine.state().phase, Phase::Beginning);
    assert_eq!(engine.state().step, Step::Upkeep);

    let hand_size = engine
        .state()
        .zones
        .iter()
        .find(|z| z.kind == ZoneKind::Hand && z.owner == Some(p0))
        .map(|z| z.contents.len())
        .unwrap_or(0);
    assert_eq!(hand_size, 7);

    let gy_size = engine
        .state()
        .zones
        .iter()
        .find(|z| z.kind == ZoneKind::Graveyard && z.owner == Some(p0))
        .map(|z| z.contents.len())
        .unwrap_or(0);
    assert_eq!(gy_size, 2);
}

#[test]
fn until_end_of_turn_continuous_effects_drained_at_cleanup() {
    let mut state = state_at_end_step();

    state.continuous_effects.push(ContinuousEffect {
        source: ObjectId(1),
        layer: Layer::PowerToughnessModifying,
        affected: AffectedFilter::Primitive(EffectFilter {  controller: None, card_types: None, self_only: false, subtypes: None, excluded_subtypes: None, }),
        effect: EffectPayload::Primitive(EffectPrimitive::ModifyPT { power: 0, toughness: 0 }),
        timestamp: SequenceNo(0),
        duration: EffectDuration::UntilEndOfTurn,
        condition: None,
    });
    state.continuous_effects.push(ContinuousEffect {
        source: ObjectId(2),
        layer: Layer::PowerToughnessModifying,
        affected: AffectedFilter::Primitive(EffectFilter {  controller: None, card_types: None, self_only: false, subtypes: None, excluded_subtypes: None, }),
        effect: EffectPayload::Primitive(EffectPrimitive::ModifyPT { power: 0, toughness: 0 }),
        timestamp: SequenceNo(1),
        duration: EffectDuration::Permanent,
        condition: None,
    });

    let mut engine = Engine::new(state);
    assert_eq!(engine.state().continuous_effects.len(), 2);

    pass_through_end_step(&mut engine);
    // cleanup already ran; effects drained. step_until_pause is idempotent here.

    assert_eq!(
        engine.state().continuous_effects.len(),
        1,
        "UntilEndOfTurn effect should be gone"
    );
    assert!(
        matches!(engine.state().continuous_effects[0].duration, EffectDuration::Permanent),
        "surviving effect should be Permanent"
    );
}

#[test]
fn permanent_effects_survive_cleanup() {
    let mut state = state_at_end_step();

    state.continuous_effects.push(ContinuousEffect {
        source: ObjectId(1),
        layer: Layer::ColorChanging,
        affected: AffectedFilter::Primitive(EffectFilter {  controller: None, card_types: None, self_only: false, subtypes: None, excluded_subtypes: None, }),
        effect: EffectPayload::Primitive(EffectPrimitive::ModifyPT { power: 0, toughness: 0 }),
        timestamp: SequenceNo(0),
        duration: EffectDuration::Permanent,
        condition: None,
    });

    let mut engine = Engine::new(state);
    pass_through_end_step(&mut engine);

    assert_eq!(engine.state().continuous_effects.len(), 1);
}
