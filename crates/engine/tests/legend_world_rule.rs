//! CR 704.5j legend rule and CR 704.5k world rule. The legend rule needs a player choice (`NeedsLegendRuleChoice`, `submit_legend_rule_choice`) and resolves the losers through `sba::resolve_deaths`. The world rule is deterministic.

use std::collections::HashMap;

use stackfish_engine::engine::{Engine, LegendRuleChoiceError, StepStatus};
use stackfish_engine::sba::{SbaOutcome, check_state_based_actions};
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{card_object, two_player_state as base_two_player_state, zones};

fn base_state() -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    // Exile is a shared zone (CR 400.10, owner: None), which `zones` already handles.
    GameState {
        step: Step::PreCombatMain,
        last_active_turn_number: HashMap::from([(p0, 1), (p1, 1)]),
        ..base_two_player_state(zones(
            p0,
            p1,
            &[ZoneKind::Battlefield, ZoneKind::Graveyard, ZoneKind::Exile],
        ))
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

fn permanent_definition(
    name: &str,
    supertypes: SupertypeSet,
    card_types: CardTypeSet,
) -> CardDefinition {
    CardDefinition {
        definition_id: 1,
        name: name.to_string(),
        mana_cost: ManaCost {
            symbols: Vec::new(),
        },
        color_indicator: None,
        supertypes,
        card_types,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: if card_types.contains(CardTypeSet::CREATURE) {
            Some(PTValue::Fixed(2))
        } else {
            None
        },
        toughness: if card_types.contains(CardTypeSet::CREATURE) {
            Some(PTValue::Fixed(2))
        } else {
            None
        },
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_legend_world".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn legendary_creature(id: ObjectId, owner: PlayerId, name: &str, timestamp: u64) -> GameObject {
    let definition = permanent_definition(name, SupertypeSet::LEGENDARY, CardTypeSet::CREATURE);
    GameObject {
        timestamp: SequenceNo(timestamp),
        ..card_object(id, definition, owner, ZoneKind::Battlefield)
    }
}

fn world_permanent(id: ObjectId, owner: PlayerId, timestamp: u64) -> GameObject {
    let definition = permanent_definition(
        "Test World Permanent",
        SupertypeSet::WORLD,
        CardTypeSet::ENCHANTMENT,
    );
    GameObject {
        timestamp: SequenceNo(timestamp),
        ..card_object(id, definition, owner, ZoneKind::Battlefield)
    }
}

fn would_die_exile_effect(source: ObjectId) -> ReplacementEffect {
    ReplacementEffect {
        source,
        spec: ReplacementSpec::Primitive {
            filter: EventFilter::WouldDie {
                affected: EffectFilter {
                    card_types: None,
                    controller: None,
                    self_only: true,
                    subtypes: None,
                    excluded_subtypes: None,
                    supertypes: None,
                    excluded_supertypes: None,
                },
            },
            payload: ReplacementPayload::ReplaceZoneDestination(ZoneKind::Exile),
        },
        consumption: ConsumptionPolicy::Repeatable,
    }
}

// CR 704.5j
#[test]
fn two_same_name_legendaries_pause_for_a_legend_rule_choice() {
    let p0 = PlayerId(0);
    let a = ObjectId(0);
    let b = ObjectId(1);
    let mut state = base_state();
    put_on_battlefield(&mut state, legendary_creature(a, p0, "Test Legend", 0));
    put_on_battlefield(&mut state, legendary_creature(b, p0, "Test Legend", 1));

    let outcome = check_state_based_actions(&mut state, &ScriptRuntime::new());
    let SbaOutcome::NeedsLegendRuleChoice {
        player,
        candidates,
        resolved_so_far,
    } = outcome
    else {
        panic!("two same-name legendaries under one controller must pause, not guess which to keep")
    };
    assert_eq!(player, p0);
    assert_eq!(candidates, vec![a, b]);
    assert!(resolved_so_far.is_empty());
    // Nothing applied yet.
    assert!(state.object(a).is_some());
    assert!(state.object(b).is_some());
}

#[test]
fn three_same_name_legendaries_offer_all_three_as_candidates() {
    let p0 = PlayerId(0);
    let a = ObjectId(0);
    let b = ObjectId(1);
    let c = ObjectId(2);
    let mut state = base_state();
    put_on_battlefield(&mut state, legendary_creature(a, p0, "Test Legend", 0));
    put_on_battlefield(&mut state, legendary_creature(b, p0, "Test Legend", 1));
    put_on_battlefield(&mut state, legendary_creature(c, p0, "Test Legend", 2));

    let outcome = check_state_based_actions(&mut state, &ScriptRuntime::new());
    let SbaOutcome::NeedsLegendRuleChoice { candidates, .. } = outcome else {
        panic!("three same-name legendaries must still pause with all three offered")
    };
    assert_eq!(
        candidates,
        vec![a, b, c],
        "CR 704.5j: chooses one, the REST (not just one other) die"
    );
}

#[test]
fn same_name_different_controllers_are_not_grouped() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let mut state = base_state();
    put_on_battlefield(
        &mut state,
        legendary_creature(ObjectId(0), p0, "Test Legend", 0),
    );
    put_on_battlefield(
        &mut state,
        legendary_creature(ObjectId(1), p1, "Test Legend", 1),
    );

    let outcome = check_state_based_actions(&mut state, &ScriptRuntime::new());
    // Legend rule is per controller (CR 704.5j), so different controllers never violate it against each other.
    assert!(matches!(outcome, SbaOutcome::Progressed(events, _) if events.is_empty()));
}

#[test]
fn legend_rule_choice_rejects_invalid_candidate() {
    let p0 = PlayerId(0);
    let a = ObjectId(0);
    let b = ObjectId(1);
    let mut state = base_state();
    put_on_battlefield(&mut state, legendary_creature(a, p0, "Test Legend", 0));
    put_on_battlefield(&mut state, legendary_creature(b, p0, "Test Legend", 1));

    let mut engine = Engine::new(state);
    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::PassPriority,
        })
        .unwrap();
    let status = drain(&mut engine);
    assert!(
        matches!(status, StepStatus::AwaitingLegendRuleChoice { .. }),
        "expected a pause, got {status:?}"
    );

    let err = engine.submit_legend_rule_choice(ObjectId(99)).unwrap_err();
    assert!(matches!(err, LegendRuleChoiceError::InvalidCandidate));
    // Still awaiting the same choice - nothing was consumed by the rejected attempt.
    assert!(matches!(
        engine.step().status,
        StepStatus::AwaitingLegendRuleChoice { .. }
    ));
}

#[test]
fn submit_legend_rule_choice_rejects_when_not_awaiting() {
    let mut engine = Engine::new(base_state());
    let err = engine.submit_legend_rule_choice(ObjectId(0)).unwrap_err();
    assert!(matches!(err, LegendRuleChoiceError::NotAwaitingChoice));
}

fn drain(engine: &mut Engine) -> StepStatus {
    loop {
        match engine.step().status {
            StepStatus::Progressed => continue,
            other => return other,
        }
    }
}

#[test]
fn choosing_one_destroys_the_others_through_the_real_pipeline() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let kept = ObjectId(0);
    let loser = ObjectId(1);
    let mut state = base_state();
    put_on_battlefield(&mut state, legendary_creature(kept, p0, "Test Legend", 0));
    put_on_battlefield(&mut state, legendary_creature(loser, p0, "Test Legend", 1));

    let mut engine = Engine::new(state);
    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::PassPriority,
        })
        .unwrap();
    let status = drain(&mut engine);
    let StepStatus::AwaitingLegendRuleChoice { player, candidates } = status else {
        panic!("expected the SBA pass to pause on the legend-rule choice, got {status:?}")
    };
    assert_eq!(player, p0);
    assert_eq!(candidates, vec![kept, loser]);

    engine
        .submit_legend_rule_choice(kept)
        .expect("a real candidate must be accepted");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p1));

    assert!(
        engine.state().object(kept).is_some(),
        "the kept legendary stays on the battlefield untouched"
    );
    assert_eq!(
        engine.state().object(kept).unwrap().zone,
        ZoneKind::Battlefield
    );
    assert!(
        engine.state().object(loser).is_none(),
        "the loser's old id is retired"
    );
    let gy = engine.state().zone(ZoneKind::Graveyard, Some(p0)).unwrap();
    assert_eq!(
        gy.contents.len(),
        1,
        "the loser really moved to the graveyard, through the real pipeline"
    );
}

#[test]
fn would_die_replacement_applies_to_the_losing_legendary() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let kept = ObjectId(0);
    let loser = ObjectId(1);
    let mut state = base_state();
    put_on_battlefield(&mut state, legendary_creature(kept, p0, "Test Legend", 0));
    put_on_battlefield(&mut state, legendary_creature(loser, p0, "Test Legend", 1));
    // "If this would die, exile it instead" on the loser shows the losers go through the real replacement pipeline.
    state
        .replacement_effects
        .push(would_die_exile_effect(loser));

    let mut engine = Engine::new(state);
    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::PassPriority,
        })
        .unwrap();
    let status = drain(&mut engine);
    assert!(matches!(
        status,
        StepStatus::AwaitingLegendRuleChoice { .. }
    ));

    engine
        .submit_legend_rule_choice(kept)
        .expect("a real candidate must be accepted");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p1));

    assert!(
        engine
            .state()
            .zone(ZoneKind::Graveyard, Some(p0))
            .unwrap()
            .contents
            .is_empty(),
        "the loser must never have touched the graveyard"
    );
    let exile = engine.state().zone(ZoneKind::Exile, None).unwrap();
    assert_eq!(
        exile.contents.len(),
        1,
        "the loser was exiled instead, per the real replacement effect"
    );
}

// CR 704.5k
#[test]
fn world_rule_destroys_all_but_the_most_recently_arrived() {
    let p0 = PlayerId(0);
    let older = ObjectId(0);
    let newer = ObjectId(1);
    let mut state = base_state();
    // A higher timestamp arrived later and has had World for the shortest time, so CR 704.5k keeps it.
    put_on_battlefield(&mut state, world_permanent(older, p0, 0));
    put_on_battlefield(&mut state, world_permanent(newer, p0, 1));

    let outcome = check_state_based_actions(&mut state, &ScriptRuntime::new());
    let SbaOutcome::Progressed(events, _) = outcome else {
        panic!("world rule is deterministic - must never pause")
    };
    assert_eq!(events.len(), 1);

    assert!(
        state.object(newer).is_some(),
        "the most-recently-arrived World permanent survives"
    );
    assert!(state.object(older).is_none(), "the older one dies");
    assert_eq!(
        state
            .zone(ZoneKind::Graveyard, Some(p0))
            .unwrap()
            .contents
            .len(),
        1
    );
}

#[test]
fn world_rule_tie_destroys_all_of_them() {
    let p0 = PlayerId(0);
    let a = ObjectId(0);
    let b = ObjectId(1);
    let mut state = base_state();
    // An identical timestamp leaves no unique survivor, so all World permanents die (CR 704.5k).
    put_on_battlefield(&mut state, world_permanent(a, p0, 5));
    put_on_battlefield(&mut state, world_permanent(b, p0, 5));

    let outcome = check_state_based_actions(&mut state, &ScriptRuntime::new());
    let SbaOutcome::Progressed(events, _) = outcome else {
        panic!("world rule is deterministic - must never pause")
    };
    assert_eq!(events.len(), 2);
    assert!(state.object(a).is_none());
    assert!(state.object(b).is_none());
}

#[test]
fn a_single_world_permanent_is_untouched() {
    let p0 = PlayerId(0);
    let only = ObjectId(0);
    let mut state = base_state();
    put_on_battlefield(&mut state, world_permanent(only, p0, 0));

    let outcome = check_state_based_actions(&mut state, &ScriptRuntime::new());
    assert!(matches!(outcome, SbaOutcome::Progressed(events, _) if events.is_empty()));
    assert!(state.object(only).is_some());
}
