//! CR 511.2, 500.5a, 723.1: `EffectDuration::UntilEndOfCombat` and `UntilYourNextTurn` expire at a real drain point. `UntilEndOfTurn` is in ending_phase.rs.
//! `WhileSourceOnBattlefield` and `UntilSourceLeaves` already work, because a zone change mints a fresh `ObjectId` and `applicable_effects_in_layer` treats an unresolvable source as inapplicable.

use std::collections::HashMap;

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::*;
use stackfish_testutil::{card_object, two_player_state as base_two_player_state, zones};

fn base_zones(p0: PlayerId, p1: PlayerId) -> Vec<Zone> {
    zones(
        p0,
        p1,
        &[
            ZoneKind::Library,
            ZoneKind::Hand,
            ZoneKind::Graveyard,
            ZoneKind::Command,
            ZoneKind::Battlefield,
            ZoneKind::Exile,
        ],
    )
}

fn state_at_end_step(active: PlayerId) -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        active_player: active,
        turn_number: 2,
        phase: Phase::Ending,
        step: Step::End,
        priority_player: active,
        last_active_turn_number: HashMap::from([(p0, 1), (p1, 1)]),
        next_object_id: 100,
        ..base_two_player_state(base_zones(p0, p1))
    }
}

// CR 511.1: priority opens at End of Combat even though the step has no turn-based actions.
fn state_at_end_of_combat() -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        phase: Phase::Combat,
        step: Step::EndOfCombat,
        last_active_turn_number: HashMap::from([(p0, 1)]),
        next_object_id: 100,
        ..base_two_player_state(base_zones(p0, p1))
    }
}

fn creature_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 1,
        name: "Test Permanent".to_string(),
        mana_cost: ManaCost {
            symbols: Vec::new(),
        },
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
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn permanent(id: ObjectId, owner: PlayerId, controller: PlayerId) -> GameObject {
    GameObject {
        controller,
        controlled_since_turn: 1,
        ..card_object(id, creature_definition(), owner, ZoneKind::Battlefield)
    }
}

fn put_on_battlefield(state: &mut GameState, object: GameObject) {
    let id = object.object_id;
    state
        .zone_mut(ZoneKind::Battlefield, None)
        .unwrap()
        .push_bottom(id);
    state.objects.insert(id, object);
}

fn inert_effect(source: ObjectId, duration: EffectDuration) -> ContinuousEffect {
    ContinuousEffect {
        source,
        layer: Layer::PowerToughnessModifying,
        affected: AffectedFilter::Primitive(EffectFilter {
            controller: None,
            card_types: None,
            self_only: false,
            subtypes: None,
            excluded_subtypes: None,
            supertypes: None,
            excluded_supertypes: None,
        }),
        effect: EffectPayload::Primitive(EffectPrimitive::ModifyPT {
            power: 0,
            toughness: 0,
        }),
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
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::PassPriority,
        })
        .expect("PassPriority should be accepted");
}

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

// CR 511.2/500.5a
#[test]
fn until_end_of_combat_effect_survives_until_only_the_first_player_has_passed() {
    let mut state = state_at_end_of_combat();
    state
        .continuous_effects
        .push(inert_effect(ObjectId(1), EffectDuration::UntilEndOfCombat));
    let mut engine = Engine::new(state);

    let s = step_until_pause(&mut engine);
    assert!(matches!(s, StepStatus::AwaitingAction { player } if player == PlayerId(0)));
    pass(&mut engine, PlayerId(0));

    // CR 500.5a: expires at the end of the combat phase, and this is still mid-step, so it is present.
    assert_eq!(engine.state().step, Step::EndOfCombat);
    assert_eq!(engine.state().continuous_effects.len(), 1);
}

#[test]
fn until_end_of_combat_effect_drains_when_the_combat_phase_actually_ends() {
    let mut state = state_at_end_of_combat();
    state
        .continuous_effects
        .push(inert_effect(ObjectId(1), EffectDuration::UntilEndOfCombat));
    state
        .continuous_effects
        .push(inert_effect(ObjectId(2), EffectDuration::Permanent));
    state
        .continuous_effects
        .push(inert_effect(ObjectId(3), EffectDuration::UntilEndOfTurn));
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
    assert!(
        engine
            .state()
            .continuous_effects
            .iter()
            .all(|e| !matches!(e.duration, EffectDuration::UntilEndOfCombat))
    );
}

// CR 723.1
#[test]
fn until_your_next_turn_effect_survives_cleanup_into_the_opponents_turn() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let mut state = state_at_end_step(p0);
    put_on_battlefield(&mut state, permanent(ObjectId(1), p0, p0));
    state
        .continuous_effects
        .push(inert_effect(ObjectId(1), EffectDuration::UntilYourNextTurn));
    let mut engine = Engine::new(state);

    pass_both_players(&mut engine, p0, p1);

    // Cleanup rotated active_player to p1, who is not the effect's controller (p0), so "your next turn" has not arrived.
    assert_eq!(engine.state().active_player, p1);
    assert_eq!(engine.state().continuous_effects.len(), 1);
}

#[test]
fn until_your_next_turn_effect_drains_when_the_turn_cycles_back_to_its_controller() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    // The turn about to end is the opponent's, so cleanup's rotation lands on p0: the beginning of p0's next turn.
    let mut state = state_at_end_step(p1);
    put_on_battlefield(&mut state, permanent(ObjectId(1), p0, p0));
    state
        .continuous_effects
        .push(inert_effect(ObjectId(1), EffectDuration::UntilYourNextTurn));
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
    // Source ObjectId(1) is placed nowhere, like a spell-sourced effect after the spell resolves.
    state
        .continuous_effects
        .push(inert_effect(ObjectId(1), EffectDuration::UntilYourNextTurn));
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
    state
        .continuous_effects
        .push(inert_effect(ObjectId(1), EffectDuration::UntilYourNextTurn));
    let mut engine = Engine::new(state);

    pass_both_players(&mut engine, p0, PlayerId(1));

    assert_eq!(engine.state().continuous_effects.len(), 1);
}
