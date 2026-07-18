//! Phase 5 vertical slice: real expiration for `EffectDuration::
//! UntilEndOfCombat` (CR 511.2/500.5a) and `EffectDuration::UntilYourNextTurn`
//! (CR 723.1-style "until your next turn"). Before this slice, both variants
//! existed on the enum but nothing ever drained them - only `UntilEndOfTurn`
//! had a real drain point (Cleanup, see `phase5_ending_phase.rs`'s own
//! coverage of that).
//!
//! `WhileSourceOnBattlefield`/`UntilSourceLeaves` are deliberately not
//! covered here - they already work today as a side effect of every zone
//! change minting a fresh `ObjectId` (`layers::applicable_effects_in_layer`'s
//! `state.object(e.source)` check already treats an unresolvable source as
//! inapplicable), so they were never part of this gap.

use std::collections::{HashMap, VecDeque};

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::*;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn empty_zone(kind: ZoneKind, owner: Option<PlayerId>) -> Zone {
    Zone { kind, owner, contents: VecDeque::new() }
}

fn player_state(id: PlayerId) -> PlayerState {
    PlayerState {
        player_id: id,
        life: 20,
        mana_pool: ManaPool::default(),
        counters: CounterSet::new(),
        emblems: Vec::new(),
        commander_damage_taken: Vec::new(),
        eliminated: false,
        drew_from_empty_library: false,
        mulligans_taken: 0,
        hand_kept: false,
        commander_casts: HashMap::new(),
    }
}

fn base_zones(p0: PlayerId, p1: PlayerId) -> Vec<Zone> {
    vec![
        empty_zone(ZoneKind::Library, Some(p0)),
        empty_zone(ZoneKind::Hand, Some(p0)),
        empty_zone(ZoneKind::Graveyard, Some(p0)),
        empty_zone(ZoneKind::Command, Some(p0)),
        empty_zone(ZoneKind::Library, Some(p1)),
        empty_zone(ZoneKind::Hand, Some(p1)),
        empty_zone(ZoneKind::Graveyard, Some(p1)),
        empty_zone(ZoneKind::Command, Some(p1)),
        empty_zone(ZoneKind::Battlefield, None),
        empty_zone(ZoneKind::Exile, None),
    ]
}

/// A minimal 2-player state sitting at `(Ending, End)`, active player
/// `active`, priority with `active` first - same shape
/// `phase5_ending_phase.rs::state_at_end_step` uses, parametrized on which
/// player is active so "your next turn" tests can start from either side.
fn state_at_end_step(active: PlayerId) -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        players: vec![player_state(p0), player_state(p1)],
        turn_order: vec![p0, p1],
        active_player: active,
        turn_number: 2,
        phase: Phase::Ending,
        step: Step::End,
        priority_player: active,
        consecutive_passes: 0,
        lands_played_this_turn: HashMap::new(),
        combat: None,
        last_active_turn_number: HashMap::from([(p0, 1), (p1, 1)]),
        objects: HashMap::new(),
        zones: base_zones(p0, p1),
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
        next_object_id: 100,
        pending_step_skips: Vec::new(),
        once_per_turn_triggered: std::collections::HashSet::new(),
        commander_return_offered: std::collections::HashSet::new(),
    }
}

/// A minimal 2-player state sitting at `(Combat, EndOfCombat)` with no
/// attackers declared (`combat: None`) - CR 511.1 still opens a priority
/// window here even though the step "has no turn-based actions."
fn state_at_end_of_combat() -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        players: vec![player_state(p0), player_state(p1)],
        turn_order: vec![p0, p1],
        active_player: p0,
        turn_number: 1,
        phase: Phase::Combat,
        step: Step::EndOfCombat,
        priority_player: p0,
        consecutive_passes: 0,
        lands_played_this_turn: HashMap::new(),
        combat: None,
        last_active_turn_number: HashMap::from([(p0, 1)]),
        objects: HashMap::new(),
        zones: base_zones(p0, p1),
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
        next_object_id: 100,
        pending_step_skips: Vec::new(),
        once_per_turn_triggered: std::collections::HashSet::new(),
        commander_return_offered: std::collections::HashSet::new(),
    }
}

fn creature_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 1,
        name: "Test Permanent".to_string(),
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
        script_ref: ScriptReference::ByKey("test_permanent".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        effect: None,
        modes: Vec::new(),
    }
}

fn permanent(id: ObjectId, owner: PlayerId, controller: PlayerId) -> GameObject {
    GameObject {
        object_id: id,
        copiable_override: None,
        lineage: ObjectLineage { previous: None },
        definition: creature_definition(),
        owner,
        controller,
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

fn put_on_battlefield(state: &mut GameState, object: GameObject) {
    let id = object.object_id;
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(id);
    state.objects.insert(id, object);
}

fn inert_effect(source: ObjectId, duration: EffectDuration) -> ContinuousEffect {
    ContinuousEffect {
        source,
        layer: Layer::PowerToughnessModifying,
        affected: AffectedFilter::Primitive(EffectFilter { controller: None, card_types: None, self_only: false }),
        effect: EffectPayload::Primitive(EffectPrimitive::ModifyPT { power: 0, toughness: 0 }),
        timestamp: SequenceNo(0),
        duration,
        condition: None,
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

/// Drives both players' priority passes for whatever step the engine is
/// currently paused at, returning whatever the resulting transition produces.
fn pass_both_players(engine: &mut Engine, first: PlayerId, second: PlayerId) -> StepStatus {
    let s = step_until_pause(engine);
    assert!(
        matches!(s, StepStatus::AwaitingAction { player } if player == first),
        "expected {first:?} priority, got {s:?}"
    );
    pass(engine, first);

    let s = step_until_pause(engine);
    assert!(
        matches!(s, StepStatus::AwaitingAction { player } if player == second),
        "expected {second:?} priority, got {s:?}"
    );
    pass(engine, second);

    step_until_pause(engine)
}

// ---------------------------------------------------------------------------
// CR 511.2/500.5a - "until end of combat"
// ---------------------------------------------------------------------------

#[test]
fn until_end_of_combat_effect_survives_until_only_the_first_player_has_passed() {
    let mut state = state_at_end_of_combat();
    state.continuous_effects.push(inert_effect(ObjectId(1), EffectDuration::UntilEndOfCombat));
    let mut engine = Engine::new(state);

    let s = step_until_pause(&mut engine);
    assert!(matches!(s, StepStatus::AwaitingAction { player } if player == PlayerId(0)));
    pass(&mut engine, PlayerId(0));

    // CR 500.5a: expires at the end of the combat phase, not the beginning
    // of the End of Combat step - still mid-step here, so still present.
    assert_eq!(engine.state().step, Step::EndOfCombat);
    assert_eq!(engine.state().continuous_effects.len(), 1);
}

#[test]
fn until_end_of_combat_effect_drains_when_the_combat_phase_actually_ends() {
    let mut state = state_at_end_of_combat();
    state.continuous_effects.push(inert_effect(ObjectId(1), EffectDuration::UntilEndOfCombat));
    state.continuous_effects.push(inert_effect(ObjectId(2), EffectDuration::Permanent));
    state.continuous_effects.push(inert_effect(ObjectId(3), EffectDuration::UntilEndOfTurn));
    let mut engine = Engine::new(state);

    let status = pass_both_players(&mut engine, PlayerId(0), PlayerId(1));

    assert_eq!(engine.state().phase, Phase::Main);
    assert_eq!(engine.state().step, Step::PostCombatMain);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == PlayerId(0)));
    assert_eq!(
        engine.state().continuous_effects.len(),
        2,
        "only the UntilEndOfCombat effect should have drained"
    );
    assert!(engine
        .state()
        .continuous_effects
        .iter()
        .all(|e| !matches!(e.duration, EffectDuration::UntilEndOfCombat)));
}

// ---------------------------------------------------------------------------
// CR 723.1-style "until your next turn"
// ---------------------------------------------------------------------------

#[test]
fn until_your_next_turn_effect_survives_cleanup_into_the_opponents_turn() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let mut state = state_at_end_step(p0);
    put_on_battlefield(&mut state, permanent(ObjectId(1), p0, p0));
    state.continuous_effects.push(inert_effect(ObjectId(1), EffectDuration::UntilYourNextTurn));
    let mut engine = Engine::new(state);

    pass_both_players(&mut engine, p0, p1);

    // Cleanup rotated active_player to p1 - not the effect's controller
    // (p0), so "your next turn" hasn't arrived yet.
    assert_eq!(engine.state().active_player, p1);
    assert_eq!(engine.state().continuous_effects.len(), 1);
}

#[test]
fn until_your_next_turn_effect_drains_when_the_turn_cycles_back_to_its_controller() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    // Active player is p1 here - the turn about to end belongs to the
    // opponent of the effect's controller (p0), so cleanup's rotation lands
    // squarely on p0: this is the beginning of p0's next turn.
    let mut state = state_at_end_step(p1);
    put_on_battlefield(&mut state, permanent(ObjectId(1), p0, p0));
    state.continuous_effects.push(inert_effect(ObjectId(1), EffectDuration::UntilYourNextTurn));
    let mut engine = Engine::new(state);

    pass_both_players(&mut engine, p1, p0);

    assert_eq!(engine.state().active_player, p0);
    assert_eq!(
        engine.state().continuous_effects.len(),
        0,
        "should drain at the beginning of the controller's next turn"
    );
}

#[test]
fn until_your_next_turn_effect_with_unresolvable_source_is_left_inert_rather_than_drained() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let mut state = state_at_end_step(p1);
    // Source ObjectId(1) never placed anywhere - same "already left the
    // stack/zone it was created from" shape a spell-sourced effect would hit
    // once that spell resolves (known limitation, not fixed by this slice).
    state.continuous_effects.push(inert_effect(ObjectId(1), EffectDuration::UntilYourNextTurn));
    let mut engine = Engine::new(state);

    pass_both_players(&mut engine, p1, p0);

    assert_eq!(engine.state().active_player, p0);
    assert_eq!(
        engine.state().continuous_effects.len(),
        1,
        "an unresolvable source can't be matched to a controller, so it's left inert"
    );
}

#[test]
fn until_your_next_turn_effect_is_untouched_by_the_end_of_combat_drain() {
    let p0 = PlayerId(0);
    let mut state = state_at_end_of_combat();
    put_on_battlefield(&mut state, permanent(ObjectId(1), p0, p0));
    state.continuous_effects.push(inert_effect(ObjectId(1), EffectDuration::UntilYourNextTurn));
    let mut engine = Engine::new(state);

    pass_both_players(&mut engine, p0, PlayerId(1));

    assert_eq!(engine.state().continuous_effects.len(), 1);
}
