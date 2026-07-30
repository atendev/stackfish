//! Phase 5: backlog item 7a's cleanup half - `CleanupSbaCheck` now routes
//! through the real `CollectTriggers`/`PutTriggersApnap` stages
//! (`Engine::finish_cleanup_sba_check`/`PipelineExit::CleanupCheck`) instead
//! of deciding priority-vs-advance from the SBA pass's own event count alone.
//!
//! Regression scenario: CR 514.2's "until end of turn" effect expiry runs
//! before cleanup's SBA check (in the same turn-based action as marked-
//! damage removal - a lethal-*damage* scenario can't survive to cleanup's
//! own SBA pass, since damage clears in that same step). A permanent -X/-X
//! effect combined with a temporary (until-end-of-turn) +X/+X pump can,
//! though: the creature has 0-or-less effective toughness (CR 704.5f) the
//! instant the pump wears off, so its "when this dies" trigger fires from
//! *inside* `CleanupSbaCheck` itself - previously silently dropped (no
//! `CollectTriggers` ever ran for this path), now placed onto the stack and
//! correctly reopening priority per CR 514.3 ("if a state-based action is
//! performed... a priority window is opened").

use std::collections::HashMap;

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::*;
use stackfish_testutil::{card_object, zones, two_player_state as base_two_player_state};

fn creature_definition(toughness: i32) -> CardDefinition {
    CardDefinition {
        definition_id: 1,
        name: "Test Creature".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::CREATURE,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: Some(PTValue::Fixed(2)),
        toughness: Some(PTValue::Fixed(toughness)),
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_creature".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn creature(id: ObjectId, owner: PlayerId, toughness: i32, damage_marked: u32) -> GameObject {
    GameObject {
        damage_marked,
        controlled_since_turn: 1,
        ..card_object(id, creature_definition(toughness), owner, ZoneKind::Battlefield)
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

/// A two-player `GameState` sitting at `(Ending, End)` in `PriorityWindow`
/// for `p0` (active player), same fixture shape `phase5_ending_phase.rs`'s
/// own `state_at_end_step` uses.
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
/// cleanup produces.
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
fn a_creature_that_dies_exactly_when_its_end_of_turn_pump_expires_fires_its_dies_trigger_at_cleanup() {
    let mut state = state_at_end_step();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let source = ObjectId(1);
    // Base toughness 1. A permanent -5/-5 drops it to -4 on its own; a
    // +0/+5 "until end of turn" pump keeps it at a healthy 1 all turn. No
    // marked damage involved at all - CR 514.2 wipes that in this same
    // cleanup step regardless, so a damage-based lethal scenario can't
    // survive to reach CleanupSbaCheck's own SBA pass.
    state.objects.insert(source, creature(source, p0, 1, 0));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(source);
    state.abilities.push(dies_ability(source));
    state.continuous_effects.push(ContinuousEffect {
        source,
        layer: Layer::PowerToughnessModifying,
        affected: AffectedFilter::Primitive(EffectFilter {  controller: None, card_types: None, self_only: false, subtypes: None, excluded_subtypes: None, }),
        effect: EffectPayload::Primitive(EffectPrimitive::ModifyPT { power: 0, toughness: -5 }),
        timestamp: SequenceNo(0),
        duration: EffectDuration::Permanent,
        condition: None,
    });
    state.continuous_effects.push(ContinuousEffect {
        source,
        layer: Layer::PowerToughnessModifying,
        affected: AffectedFilter::Primitive(EffectFilter {  controller: None, card_types: None, self_only: false, subtypes: None, excluded_subtypes: None, }),
        effect: EffectPayload::Primitive(EffectPrimitive::ModifyPT { power: 0, toughness: 5 }),
        timestamp: SequenceNo(1),
        duration: EffectDuration::UntilEndOfTurn,
        condition: None,
    });

    let mut engine = Engine::new(state);
    let status = pass_through_end_step(&mut engine, p0, p1);

    // The pump expired at cleanup (CR 514.2), the creature dropped to
    // 0-or-less effective toughness, CleanupSbaCheck's own SBA pass killed
    // it (CR 704.5f), and - the actual fix - the resulting Dies trigger was
    // collected and pushed onto the stack instead of silently dropped, so
    // priority reopens for p0 (the active player) rather than the turn
    // advancing straight to p1's Upkeep.
    assert!(
        matches!(status, StepStatus::AwaitingAction { player } if player == p0),
        "expected p0 priority reopened by the Dies trigger, got {status:?}"
    );
    assert_eq!(engine.state().phase, Phase::Ending, "still in cleanup, turn did not silently advance");
    assert_eq!(engine.state().step, Step::Cleanup);
    assert_eq!(engine.state().stack.len(), 1, "the Dies trigger was pushed onto the stack");
    assert!(
        engine.state().zone(ZoneKind::Battlefield, None).unwrap().contents.is_empty(),
        "the creature actually died"
    );
    assert_eq!(
        engine.state().zone(ZoneKind::Graveyard, Some(p0)).unwrap().contents.len(),
        1,
        "the dead creature landed in its owner's graveyard"
    );

    // Resolve the trigger for real: both pass, GainLife(1) applies.
    pass(&mut engine, p0);
    step_until_pause(&mut engine);
    pass(&mut engine, p1);
    let status = step_until_pause(&mut engine);

    assert_eq!(engine.state().player(p0).unwrap().life, 21, "GainLife(1) applied from the Dies trigger");
    assert!(engine.state().stack.is_empty());
    // CR 117.5: priority reopens for p0 once more (still the same Cleanup
    // step, empty stack now) before the turn is finally allowed to advance -
    // one more full pass round is needed.
    assert!(
        matches!(status, StepStatus::AwaitingAction { player } if player == p0),
        "expected priority to reopen once more for p0 with the stack now empty, got {status:?}"
    );
    pass(&mut engine, p0);
    step_until_pause(&mut engine);
    pass(&mut engine, p1);
    let status = step_until_pause(&mut engine);

    assert!(
        matches!(status, StepStatus::AwaitingAction { player } if player == p1),
        "turn now advances normally to p1's Upkeep, got {status:?}"
    );
    assert_eq!(engine.state().phase, Phase::Beginning);
    assert_eq!(engine.state().step, Step::Upkeep);
}

#[test]
fn cleanup_with_no_sba_and_no_trigger_still_advances_silently() {
    let mut state = state_at_end_step();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    // A perfectly healthy creature - untouched by cleanup's SBA pass, no
    // Dies trigger should ever fire, matching pre-existing behavior
    // (phase5_ending_phase.rs's own cleanup_no_discard_needed_* tests) - not
    // a regression introduced by routing CleanupSbaCheck through the real
    // pipeline now.
    let source = ObjectId(1);
    state.objects.insert(source, creature(source, p0, 3, 0));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(source);
    state.abilities.push(dies_ability(source));

    let mut engine = Engine::new(state);
    let status = pass_through_end_step(&mut engine, p0, p1);

    assert!(
        matches!(status, StepStatus::AwaitingAction { player } if player == p1),
        "expected p1 Upkeep priority (silent advance), got {status:?}"
    );
    assert_eq!(engine.state().phase, Phase::Beginning);
    assert_eq!(engine.state().step, Step::Upkeep);
    assert_eq!(engine.state().active_player, p1);
    assert!(engine.state().stack.is_empty());
}
