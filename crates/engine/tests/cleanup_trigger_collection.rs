//! CR 514.3: `CleanupSbaCheck` routes through `CollectTriggers` and `PutTriggersApnap`, so a trigger fired in the cleanup SBA pass reaches the stack and reopens priority.
//! A permanent -X/-X plus an until-end-of-turn pump reaches 0 toughness (CR 704.5f) when the pump expires (CR 514.2).

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
        buyback_cost: None,
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
    RegisteredAbility {
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
        id: AbilityId(0),
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
    // A permanent -5/-5 plus a +0/+5 pump leaves toughness 1 all turn, with no marked damage since CR 514.2 clears it in the same step.
    state.objects.insert(source, creature(source, p0, 1, 0));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(source);
    state.abilities.push(dies_ability(source));
    state.continuous_effects.push(ContinuousEffect {
        source,
        layer: Layer::PowerToughnessModifying,
        affected: AffectedFilter::Primitive(EffectFilter {  controller: None, card_types: None, self_only: false, subtypes: None, excluded_subtypes: None, supertypes: None, excluded_supertypes: None, }),
        effect: EffectPayload::Primitive(EffectPrimitive::ModifyPT { power: 0, toughness: -5 }),
        timestamp: SequenceNo(0),
        duration: EffectDuration::Permanent,
        condition: None,
    });
    state.continuous_effects.push(ContinuousEffect {
        source,
        layer: Layer::PowerToughnessModifying,
        affected: AffectedFilter::Primitive(EffectFilter {  controller: None, card_types: None, self_only: false, subtypes: None, excluded_subtypes: None, supertypes: None, excluded_supertypes: None, }),
        effect: EffectPayload::Primitive(EffectPrimitive::ModifyPT { power: 0, toughness: 5 }),
        timestamp: SequenceNo(1),
        duration: EffectDuration::UntilEndOfTurn,
        condition: None,
    });

    let mut engine = Engine::new(state);
    let status = pass_through_end_step(&mut engine, p0, p1);

    // The pump expired (CR 514.2), toughness hit 0 (CR 704.5f) and the Dies trigger was pushed, so priority reopens for p0 and the turn does not advance.
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
    // CR 117.5: priority reopens for p0 once more in the same Cleanup step, so one more pass round is needed.
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
    // A healthy creature fires no Dies trigger, as the cleanup_no_discard_needed tests in ending_phase.rs also show.
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
