//! Phase 5 vertical slice: CR 402.2 max hand size actually consults live
//! `HandSizeModifier`s instead of a bare hardcoded `7`. Before this slice,
//! `Engine::begin_cleanup_step` never looked at anything - a "you have no
//! maximum hand size" or "your maximum hand size is N" effect had no way to
//! change cleanup's discard behavior at all.
//!
//! `HandSizeModifier` is deliberately not a `ContinuousEffect`/CR 613 layer
//! - same reasoning `cost.rs`'s `CostModifier` already established for cost
//! modification: hand size isn't a characteristic query, and `Layer` is
//! meant to be exactly the CR 613.1 layer list. No `Game.CreateHandSizeModifier`
//! Lua builder yet - same "direct Rust struct-literal fixtures only"
//! bootstrap stance `CostModifier` itself took before any script builder
//! existed for it, so every test here constructs `HandSizeModifier` directly.

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
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    };
    GameObject { controlled_since_turn: 1, ..card_object(id, definition, owner, ZoneKind::Hand) }
}

/// A two-player `GameState` sitting at `(Ending, End)` in `PriorityWindow`
/// for `p0` (active player) - mirrors `phase5_ending_phase.rs::state_at_end_step`.
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
        affected: FilterController::You,
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
        affected: FilterController::You,
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
    // SetTo(10) applies first (earlier timestamp), then NoMaximum starts
    // applying later - the later one wins outright, same "latest timestamp"
    // convention this engine's layer effects already use for a conflicting
    // flat override.
    state.hand_size_modifiers.push(HandSizeModifier {
        source,
        controller: p0,
        affected: FilterController::You,
        effect: HandSizeEffect::SetTo(10),
        timestamp: SequenceNo(0),
    });
    state.hand_size_modifiers.push(HandSizeModifier {
        source,
        controller: p0,
        affected: FilterController::You,
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
    // "Your opponents have no maximum hand size" - controlled by p0, but
    // p0's own cleanup (the active player here) must be unaffected by its
    // own Opponent-scoped effect.
    state.hand_size_modifiers.push(HandSizeModifier {
        source,
        controller: p0,
        affected: FilterController::Opponent,
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
