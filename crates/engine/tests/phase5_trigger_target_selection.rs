//! Phase 5 vertical slice: CR 603.3d, a triggered ability that requires a
//! target now genuinely pauses for its controller to choose one before
//! going on the stack, instead of always being pushed with an empty
//! `targets: Vec::new()` and fizzling at the CR 608.2b recheck.
//! `triggers_pipeline.rs`'s own final push loop
//! (`resolve_put_triggers_apnap_state`) is resumable now
//! (`Engine::push_ordered_triggers`), pausing on
//! `ProcessState`/`StepStatus::AwaitingTriggerTargets` the moment a
//! `TargetSpec::Primitive`-shaped trigger has real candidates to choose
//! from, and skipping the push entirely (CR 603.3d's own "the ability is
//! simply removed from the stack") when it doesn't.
//!
//! Narrowed to `TargetSpec::Primitive` - every real triggered ability this
//! engine has ever modeled (Wall of Omens' plain `GainLife`, Vesuvan
//! Doppelganger's own granted upkeep ability) is this shape;
//! `TargetSpec::Multiple` on a trigger stays a named gap, same
//! empty-targets fallback as before this slice.

use stackfish_engine::engine::{Engine, StepStatus, TriggerTargetsError};
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, two_player_state as base_two_player_state, zones};

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
        mode_choice: ModeChoice::Exactly(1),
    }
}

/// Mirrors `phase5_upkeep_triggers::two_player_state` - starts at
/// `Phase::PreGame`, so one `pass_both` lands exactly on Upkeep.
fn two_player_state() -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        phase: Phase::PreGame,
        ..base_two_player_state(zones(p0, p1, &[ZoneKind::Hand, ZoneKind::Library, ZoneKind::Battlefield, ZoneKind::Graveyard]))
    }
}

fn pass_both(engine: &mut Engine, p0: PlayerId, p1: PlayerId) -> StepStatus {
    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine);
    engine.submit_action(PlayerAction { player: p1, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine)
}

/// CR 603.10's own worked example shape: "when this dies, destroy target
/// creature" - here on a plain upkeep trigger instead, so it's reachable
/// without also exercising the departed-source machinery.
fn destroy_target_creature_trigger(source: ObjectId, exclude_source: bool) -> RegisteredAbility {
    RegisteredAbility {
        id: AbilityId(0),
        source,
        kind: AbilityKind::Triggered,
        condition: Some(ConditionSpec::Primitive(ConditionPrimitive::BeginningOfYourUpkeep)),
        cost: None,
        target: Some(TargetSpec::Primitive(TargetRequirement {
            filter: TargetFilter::Object {
                card_types: Some(CardTypeSet::CREATURE),
                excluded_card_types: None,
                subtypes: None,
                excluded_subtypes: None,
                supertypes: None,
                excluded_supertypes: None,
                controller: None,
                zone: None,
            },
            count: 1,
            up_to: false,
            at_least: false,
            exclude_source,
        })),
        operation: ScriptHandle(0),
        effect: Some(AbilityEffect::Destroy),
        intervening_if: None,
    }
}

#[test]
fn triggered_ability_pauses_for_a_real_target_choice_and_resolves_against_it() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let reaper = ObjectId(0);
    let bear = ObjectId(1);
    let mut state = two_player_state();
    state.objects.insert(reaper, card_object(reaper, creature_definition("Reaper"), p0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(reaper);
    state.objects.insert(bear, card_object(bear, creature_definition("Bear"), p1, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(bear);
    state.abilities.push(destroy_target_creature_trigger(reaper, false));

    let mut engine = Engine::new(state);
    let status = pass_both(&mut engine, p0, p1);
    let StepStatus::AwaitingTriggerTargets { player, requirement, candidates } = status else {
        panic!("expected a CR 603.3d target-choice pause, got {status:?}")
    };
    assert_eq!(player, p0, "the trigger's own controller chooses");
    assert_eq!(requirement.count, 1);
    let mut sorted = candidates.clone();
    sorted.sort_by_key(|t| match t {
        Target::Object(id) => id.0,
        Target::Player(_) => u64::MAX,
    });
    assert_eq!(sorted, vec![Target::Object(reaper), Target::Object(bear)], "both creatures are legal candidates");
    assert_eq!(engine.state().stack.len(), 0, "not pushed yet - still waiting on the choice");

    engine.submit_trigger_targets(vec![Target::Object(bear)]).expect("Bear was an offered candidate");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0), "the pushed trigger opens a fresh priority window, got {status:?}");
    assert_eq!(engine.state().stack.len(), 1, "the trigger is now really on the stack, targeting Bear");

    // The stack object still needs both players to pass priority before it
    // resolves - same as any other triggered ability (phase5_upkeep_triggers).
    let status = pass_both(&mut engine, p0, p1);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0), "resolution finishes cleanly, got {status:?}");
    assert!(engine.state().object(bear).is_none() || engine.state().object(bear).unwrap().zone != ZoneKind::Battlefield, "Bear was destroyed by the real, chosen target");
    assert!(engine.state().object(reaper).is_some(), "Reaper itself is untouched");
}

/// CR 115.4/"another target": with `exclude_source` set and no other
/// creature on the battlefield, the only creature that would otherwise
/// qualify (the source itself) is excluded - zero real candidates.
#[test]
fn triggered_ability_with_zero_legal_targets_is_removed_from_the_stack_without_pausing() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let reaper = ObjectId(0);
    let mut state = two_player_state();
    state.objects.insert(reaper, card_object(reaper, creature_definition("Reaper"), p0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(reaper);
    state.abilities.push(destroy_target_creature_trigger(reaper, true));

    let mut engine = Engine::new(state);
    let status = pass_both(&mut engine, p0, p1);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0), "no pause - straight through to the next priority window, got {status:?}");
    assert_eq!(engine.state().step, Step::Upkeep);
    assert!(engine.state().stack.is_empty(), "CR 603.3d: no legal targets - the ability was removed from the stack, never pushed");
}

#[test]
fn submit_trigger_targets_rejects_when_not_awaiting() {
    let mut engine = Engine::new(two_player_state());
    let err = engine.submit_trigger_targets(Vec::new()).unwrap_err();
    assert!(matches!(err, TriggerTargetsError::NotAwaitingChoice));
}

#[test]
fn submit_trigger_targets_rejects_a_candidate_not_offered() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let reaper = ObjectId(0);
    let bear = ObjectId(1);
    let mut state = two_player_state();
    state.objects.insert(reaper, card_object(reaper, creature_definition("Reaper"), p0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(reaper);
    state.objects.insert(bear, card_object(bear, creature_definition("Bear"), p1, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(bear);
    state.abilities.push(destroy_target_creature_trigger(reaper, false));

    let mut engine = Engine::new(state);
    let status = pass_both(&mut engine, p0, p1);
    assert!(matches!(status, StepStatus::AwaitingTriggerTargets { .. }));

    let not_a_candidate = ObjectId(999);
    let err = engine.submit_trigger_targets(vec![Target::Object(not_a_candidate)]).unwrap_err();
    assert!(matches!(err, TriggerTargetsError::InvalidTargets));
    // Still paused - a rejected answer doesn't consume the choice.
    assert_eq!(engine.state().stack.len(), 0);
}

/// Closes the exact gap `phase5/vesuvan-linked-copy-ability`'s own report
/// named: "CR 603.3c [sic, really 603.3d] blocks this ability from ever
/// firing through real gameplay." Builds the granted ability by hand in the
/// exact shape `commands::grant_linked_copy_ability` produces (that
/// function itself is `pub(crate)`, not reachable from an integration
/// test), then lets it fire through a real `BeginningOfYourUpkeep`
/// collection - no manual re-registration-as-an-Activated-ability
/// workaround needed anymore.
#[test]
fn vesuvans_granted_upkeep_ability_now_really_fires_and_becomes_a_copy() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let doppel = ObjectId(0);
    let salamander = ObjectId(1);
    let mut state = two_player_state();
    state.objects.insert(doppel, card_object(doppel, creature_definition("Vesuvan Doppelganger"), p0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(doppel);
    let mut salamander_def = creature_definition("Salamander");
    salamander_def.power = Some(PTValue::Fixed(5));
    salamander_def.toughness = Some(PTValue::Fixed(4));
    state.objects.insert(salamander, card_object(salamander, salamander_def, p1, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(salamander);
    let exceptions = CopyExceptions { grant_recurring_copy_trigger: true, ..CopyExceptions::default() };
    state.abilities.push(RegisteredAbility {
        id: AbilityId(0),
        source: doppel,
        kind: AbilityKind::Triggered,
        condition: Some(ConditionSpec::Primitive(ConditionPrimitive::BeginningOfYourUpkeep)),
        cost: None,
        target: Some(TargetSpec::Primitive(TargetRequirement {
            filter: TargetFilter::Object {
                card_types: Some(CardTypeSet::CREATURE),
                excluded_card_types: None,
                subtypes: None,
                excluded_subtypes: None,
                supertypes: None,
                excluded_supertypes: None,
                controller: None,
                zone: None,
            },
            count: 1,
            up_to: false,
            at_least: false,
            exclude_source: false,
        })),
        operation: ScriptHandle(0),
        effect: Some(AbilityEffect::BecomeCopy(exceptions)),
        intervening_if: None,
    });

    let mut engine = Engine::new(state);
    let status = pass_both(&mut engine, p0, p1);
    let StepStatus::AwaitingTriggerTargets { player, candidates, .. } = status else {
        panic!("expected the real upkeep trigger to pause for a target, got {status:?}")
    };
    assert_eq!(player, p0);
    assert!(candidates.contains(&Target::Object(salamander)));

    engine.submit_trigger_targets(vec![Target::Object(salamander)]).expect("Salamander was offered");
    drain(&mut engine);
    let status = pass_both(&mut engine, p0, p1);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));

    let runtime = stackfish_engine::script::ScriptRuntime::new();
    let now = stackfish_engine::layers::effective_characteristics(engine.state(), &runtime, doppel);
    assert_eq!(now.name, "Salamander", "the real, natural upkeep trigger actually resolved the copy");
    assert_eq!(now.power, Some(5));
    // The re-granted linked ability is present again (Vesuvan "keeps
    // flipping indefinitely") under a fresh AbilityId - CR 707.9a.
    let linked: Vec<_> =
        engine.state().abilities.iter().filter(|a| a.source == doppel && a.kind == AbilityKind::Triggered).collect();
    assert_eq!(linked.len(), 1, "re-granted after the copy, not lost");
}
