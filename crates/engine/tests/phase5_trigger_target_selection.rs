//! Phase 5 vertical slice: CR 603.3d, a triggered ability that requires a
//! target (or targets - CR 601.2c's multi-word "destroy target creature and
//! target land" form works here too now) genuinely pauses for its
//! controller to choose before going on the stack, instead of always being
//! pushed with an empty `targets: Vec::new()` and fizzling at the CR 608.2b
//! recheck. `triggers_pipeline.rs`'s own final push loop
//! (`resolve_put_triggers_apnap_state`) is resumable now
//! (`Engine::push_ordered_triggers`), pausing on
//! `ProcessState`/`StepStatus::AwaitingTriggerTargets` the moment a
//! triggered ability with a real `TargetSpec` (single- or multi-word) has
//! every word's own minimum met by real candidates, and skipping the push
//! entirely (CR 603.3d's own "the ability is simply removed from the
//! stack") when even one word can't be - the same all-or-nothing stance CR
//! 601.2c/e already takes for a spell.
//!
//! `phase5/trigger-target-selection` (the previous slice) started narrowed
//! to `TargetSpec::Primitive`; `phase5/trigger-multi-target` (this one)
//! closed that gap by generalizing the pause to any `TargetSpec` and
//! reusing `commands::validate_targets`/`split_targets_by_requirement`
//! wholesale rather than re-deriving the per-word split logic.

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
        buyback_cost: None,
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
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
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

fn land_definition(name: &str) -> CardDefinition {
    CardDefinition {
        definition_id: 2,
        name: name.to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::LAND,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::Inline(String::new()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn creature_target_word() -> TargetRequirement {
    TargetRequirement {
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
        exclude_source: true,
    }
}

fn land_target_word() -> TargetRequirement {
    TargetRequirement {
        filter: TargetFilter::Object {
            card_types: Some(CardTypeSet::LAND),
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
    }
}

/// CR 601.2c's own worked example shape, on a trigger instead of a spell:
/// "destroy target creature and target land."
fn destroy_creature_and_land_trigger(source: ObjectId) -> RegisteredAbility {
    RegisteredAbility {
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
        id: AbilityId(0),
        source,
        kind: AbilityKind::Triggered,
        condition: Some(ConditionSpec::Primitive(ConditionPrimitive::BeginningOfYourUpkeep)),
        cost: None,
        target: Some(TargetSpec::Multiple(vec![creature_target_word(), land_target_word()])),
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
    let StepStatus::AwaitingTriggerTargets { player, spec, candidates } = status else {
        panic!("expected a CR 603.3d target-choice pause, got {status:?}")
    };
    assert_eq!(player, p0, "the trigger's own controller chooses");
    let TargetSpec::Primitive(requirement) = &spec else { panic!("expected a single-word spec, got {spec:?}") };
    assert_eq!(requirement.count, 1);
    assert_eq!(candidates.len(), 1, "one target word, one candidate list");
    let mut sorted = candidates[0].clone();
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
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
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
    assert!(candidates[0].contains(&Target::Object(salamander)));

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

/// CR 601.2c's multi-word form works on a trigger too now - "destroy target
/// creature and target land" pauses once, offering one candidate list per
/// word, and a single flat answer resolves both.
#[test]
fn triggered_ability_with_multiple_target_words_pauses_once_and_resolves_both() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let reaper = ObjectId(0);
    let bear = ObjectId(1);
    let swamp = ObjectId(2);
    let mut state = two_player_state();
    state.objects.insert(reaper, card_object(reaper, creature_definition("Reaper"), p0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(reaper);
    state.objects.insert(bear, card_object(bear, creature_definition("Bear"), p1, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(bear);
    state.objects.insert(swamp, card_object(swamp, land_definition("Swamp"), p1, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(swamp);
    state.abilities.push(destroy_creature_and_land_trigger(reaper));

    let mut engine = Engine::new(state);
    let status = pass_both(&mut engine, p0, p1);
    let StepStatus::AwaitingTriggerTargets { player, spec, candidates } = status else {
        panic!("expected a CR 603.3d target-choice pause, got {status:?}")
    };
    assert_eq!(player, p0);
    assert!(matches!(spec, TargetSpec::Multiple(ref reqs) if reqs.len() == 2));
    assert_eq!(candidates.len(), 2, "one candidate list per target word");
    assert_eq!(candidates[0], vec![Target::Object(bear)], "creature word - Reaper excludes itself, so only Bear qualifies");
    assert_eq!(candidates[1], vec![Target::Object(swamp)], "land word");

    engine.submit_trigger_targets(vec![Target::Object(bear), Target::Object(swamp)]).expect("both words satisfied");
    drain(&mut engine);
    let status = pass_both(&mut engine, p0, p1);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0), "resolution finishes cleanly, got {status:?}");
    assert!(engine.state().object(bear).is_none() || engine.state().object(bear).unwrap().zone != ZoneKind::Battlefield, "Bear was destroyed");
    assert!(engine.state().object(swamp).is_none() || engine.state().object(swamp).unwrap().zone != ZoneKind::Battlefield, "Swamp was destroyed");
}

/// CR 601.2c/e's all-or-nothing stance: one target word coming up with zero
/// legal candidates voids the whole trigger, not just that word - here the
/// land word has nothing to target, so the creature word never gets a
/// chance either.
#[test]
fn triggered_ability_with_multiple_target_words_and_one_unsatisfiable_is_removed_entirely() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let reaper = ObjectId(0);
    let bear = ObjectId(1);
    let mut state = two_player_state();
    state.objects.insert(reaper, card_object(reaper, creature_definition("Reaper"), p0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(reaper);
    state.objects.insert(bear, card_object(bear, creature_definition("Bear"), p1, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(bear);
    // No land anywhere - the land word's own minimum can never be met.
    state.abilities.push(destroy_creature_and_land_trigger(reaper));

    let mut engine = Engine::new(state);
    let status = pass_both(&mut engine, p0, p1);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0), "no pause - straight through, got {status:?}");
    assert!(engine.state().stack.is_empty(), "CR 603.3d: the land word has no legal target, so the whole ability is removed from the stack");
    assert!(engine.state().object(bear).is_some(), "Bear survives - the creature word never even got asked");
}
