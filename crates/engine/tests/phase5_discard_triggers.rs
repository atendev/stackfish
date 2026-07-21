//! Phase 5: `DiscardsCard`/`EachDiscardsCard` trigger conditions (backlog
//! item 7's discard bullet). `Engine::submit_discard` (CR 514.1's cleanup
//! hand-size discard) used to commit its `ZoneChange` events directly to the
//! ledger and jump straight to `ProcessState::CleanupSbaCheck`, bypassing
//! `CollectTriggers` entirely - the same `AdvanceTurnStep`-shaped pipeline
//! bypass gap the `upkeep-triggers`/Untap fixes already closed elsewhere.
//! Now routes through `CollectTriggers`/`PutTriggersApnap` via a new
//! unconditional `PipelineExit::CleanupSbaCheck`, which still reaches
//! `ProcessState::CleanupSbaCheck` afterward regardless of whether anything
//! triggered (CR 514.3's discard-then-SBA-check sequencing).

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
    };
    GameObject {
        object_id: id,
        copiable_override: None,
        lineage: ObjectLineage { previous: None },
        definition,
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

/// A two-player `GameState` sitting at `(Ending, End)` in `PriorityWindow`
/// for `p0` (active player) - mirrors `phase5_ending_phase.rs::state_at_end_step`.
fn state_at_end_step() -> GameState {
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
        hand_size_modifiers: Vec::new(),
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

fn add_cards_to_hand(state: &mut GameState, player: PlayerId, count: usize) -> Vec<ObjectId> {
    let mut ids = Vec::with_capacity(count);
    for i in 0..count {
        let id = ObjectId(100 + i as u64);
        state.objects.insert(id, vanilla_card(id, player));
        state.zone_mut(ZoneKind::Hand, Some(player)).unwrap().push_bottom(id);
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
fn discards_card_trigger_fires_off_the_cleanup_hand_size_discard() {
    let mut state = state_at_end_step();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let watcher = ObjectId(0);
    state.objects.insert(watcher, permanent(watcher, creature_definition("Watcher"), p0));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(watcher);
    state.abilities.push(RegisteredAbility { id: AbilityId(0),
        source: watcher,
        kind: AbilityKind::Triggered,
        condition: Some(ConditionSpec::Primitive(ConditionPrimitive::DiscardsCard)),
        cost: None,
        target: None,
        operation: ScriptHandle(0),
        effect: Some(AbilityEffect::GainLife(1)),
        intervening_if: None,
    });
    // Exactly one card over the limit (excess = 1) - keeps this test to a
    // single trigger; the two-discards case (excess = 2, group.len() == 2)
    // is its own test below.
    let ids = add_cards_to_hand(&mut state, p0, 8);

    let mut engine = Engine::new(state);
    let status = pass_through_end_step(&mut engine, p0, p1);
    assert!(matches!(status, StepStatus::AwaitingDiscard { player, hand_size: 8, max_hand_size: 7 } if player == p0));

    engine.submit_discard(vec![ids[0]]).expect("valid discard");
    // The DiscardsCard trigger must already be collected and on the stack -
    // not silently dropped - by the time this settles.
    let status = step_until_pause(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0), "expected priority reopened by the trigger, got {status:?}");
    assert_eq!(engine.state().stack.len(), 1, "the DiscardsCard trigger was collected and pushed");

    pass(&mut engine, p0);
    step_until_pause(&mut engine);
    pass(&mut engine, p1);
    let status = step_until_pause(&mut engine);
    assert!(engine.state().stack.is_empty(), "the triggered ability resolved");
    assert_eq!(engine.state().player(p0).unwrap().life, 21, "GainLife(1) applied from the DiscardsCard trigger");

    // One more full pass round (empty stack) actually advances the turn.
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    pass(&mut engine, p0);
    step_until_pause(&mut engine);
    pass(&mut engine, p1);
    let status = step_until_pause(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p1), "expected p1 Upkeep, got {status:?}");
    assert_eq!(engine.state().phase, Phase::Beginning);
    assert_eq!(engine.state().step, Step::Upkeep);
}

#[test]
fn discarding_two_cards_at_once_fires_the_trigger_twice() {
    let mut state = state_at_end_step();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let watcher = ObjectId(0);
    state.objects.insert(watcher, permanent(watcher, creature_definition("Watcher"), p0));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(watcher);
    state.abilities.push(RegisteredAbility { id: AbilityId(1),
        source: watcher,
        kind: AbilityKind::Triggered,
        condition: Some(ConditionSpec::Primitive(ConditionPrimitive::DiscardsCard)),
        cost: None,
        target: None,
        operation: ScriptHandle(0),
        effect: Some(AbilityEffect::GainLife(1)),
        intervening_if: None,
    });
    let ids = add_cards_to_hand(&mut state, p0, 9);

    let mut engine = Engine::new(state);
    pass_through_end_step(&mut engine, p0, p1);
    engine.submit_discard(vec![ids[0], ids[1]]).expect("valid discard");
    let status = step_until_pause(&mut engine);

    // Two discarded cards, both matched by the same ability's stable id -
    // CR 603.3b pauses for p0 to choose their relative stack order rather
    // than pushing both outright, same "group size proves it" shape
    // phase5_trigger_once_per_turn.rs's own uncapped test uses.
    match status {
        StepStatus::AwaitingTriggerOrderChoice { player, group } => {
            assert_eq!(player, p0);
            assert_eq!(group.len(), 2, "both discarded cards triggered their own instance");
        }
        other => panic!("expected AwaitingTriggerOrderChoice for two same-controller triggers, got {other:?}"),
    }
}
