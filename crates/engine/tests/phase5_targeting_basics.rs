//! Phase 5 vertical slice: targeting system (CR 115 / 601.2c-e / 608.2b).
//! Covers a fixed-count primitive `TargetRequirement` on `CastSpell`'s spell
//! and `ActivateAbility`'s non-mana ability path - cast/activation-time
//! count/filter/dedup validation (601.2c/601.2e, 602.2b), and the CR 608.2b
//! all-targets-illegal recheck at `ResolveStackTop`. Same struct-literal
//! `GameState` construction pattern as `phase5_cast_spell.rs`.

use std::collections::{HashMap, VecDeque};

use stackfish_engine::commands::CommandError;
use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::*;

fn creature_definition(definition_id: u64, name: &str) -> CardDefinition {
    CardDefinition {
        definition_id,
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

fn artifact_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 10,
        name: "Test Artifact".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::ARTIFACT,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("artifact".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        effect: None,
        modes: Vec::new(),
    }
}

/// "Test Bolt deals 3 damage to any target" shape - CR 115.4's `AnyTarget`,
/// exactly one target.
fn bolt_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 20,
        name: "Test Bolt".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::INSTANT,
        subtypes: Vec::new(),
        oracle_text: "Test Bolt deals 3 damage to any target.".to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_bolt".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: Some(TargetSpec::Primitive(TargetRequirement {
            filter: TargetFilter::AnyTarget,
            count: 1,
            up_to: false, at_least: false,
            exclude_source: false,
        })),
        alternative_costs: Vec::new(),
        effect: None,
        modes: Vec::new(),
    }
}

/// "Test Pump: target creature you control gets +1/+1" shape - `Object`
/// filter with a `card_types`/`controller` restriction, exactly one target.
fn pump_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 21,
        name: "Test Pump".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::INSTANT,
        subtypes: Vec::new(),
        oracle_text: "Target creature you control gets +1/+1 until end of turn.".to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_pump".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: Some(TargetSpec::Primitive(TargetRequirement {
            filter: TargetFilter::Object { excluded_card_types: None, excluded_subtypes: None,
                card_types: Some(CardTypeSet::CREATURE),
                subtypes: None,
                supertypes: None,
                controller: Some(FilterController::You),
                zone: None,
            },
            count: 1,
            up_to: false, at_least: false,
            exclude_source: false,
        })),
        alternative_costs: Vec::new(),
        effect: None,
        modes: Vec::new(),
    }
}

fn card_object(id: ObjectId, definition: CardDefinition, owner: PlayerId, zone: ZoneKind) -> GameObject {
    GameObject {
        object_id: id,
        copiable_override: None,
        lineage: ObjectLineage { previous: None },
        definition,
        owner,
        controller: owner,
        zone,
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
        controlled_since_turn: 0,
        is_token: false,
        is_commander: false,
        visibility: VisibilityPolicy { public: true, visible_to: Vec::new() },
    }
}

/// Two players, hand/battlefield/graveyard zones present for both, stack
/// empty. Mirrors `phase5_cast_spell::two_player_state`.
fn two_player_state() -> GameState {
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
        turn_number: 1,
        phase: Phase::Main,
        step: Step::Untap,
        priority_player: p0,
        consecutive_passes: 0,
        lands_played_this_turn: HashMap::new(),
        combat: None,
        last_active_turn_number: HashMap::new(),
        objects: HashMap::new(),
        zones: vec![
            Zone { kind: ZoneKind::Hand, owner: Some(p0), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Hand, owner: Some(p1), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Battlefield, owner: None, contents: VecDeque::new() },
            Zone { kind: ZoneKind::Graveyard, owner: Some(p0), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Graveyard, owner: Some(p1), contents: VecDeque::new() },
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
        // Deliberately not 0/1 - those collide with the literal ObjectIds
        // the tests below assign to fixture cards, same reasoning as
        // phase5_cast_spell.rs's two_player_state.
        next_object_id: 1000,
        pending_step_skips: Vec::new(),
        once_per_turn_triggered: std::collections::HashSet::new(),
        commander_return_offered: std::collections::HashSet::new(),
    }
}

fn drain(engine: &mut Engine) -> StepStatus {
    loop {
        let result = engine.step();
        match result.status {
            StepStatus::Progressed => continue,
            other => return other,
        }
    }
}

/// Every fixture card in this file has an empty `mana_cost` - pays it via
/// the explicit empty `PaymentPlan` CR 118.5a still requires, then drains
/// the rest of that pipeline run.
fn pay_free_and_drain(engine: &mut Engine) -> StepStatus {
    let status = drain(engine);
    assert!(
        matches!(status, StepStatus::AwaitingPayment { .. }),
        "expected a pause for the cast's (free) cost, got {status:?}"
    );
    engine.submit_payment(PaymentPlan { payments: Vec::new() }).expect("empty plan pays a {0} cost");
    drain(engine)
}

fn cast_spell(engine: &mut Engine, player: PlayerId, card: ObjectId, targets: Vec<Target>) {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::CastSpell { card, modes: Vec::new(), targets, x_value: None, alternative_cost: None },
        })
        .expect("player holds priority, spell is in hand");
}

#[test]
fn cast_spell_rejects_wrong_target_count() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let creature = ObjectId(0);
    state.objects.insert(creature, card_object(creature, creature_definition(1, "Bear"), p0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(creature);
    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, bolt_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    // bolt_definition requires exactly 1 target - submitting 0 or 2 is
    // illegal (CR 601.2c: the count is fixed by the spell's own text).
    cast_spell(&mut engine, p0, card, Vec::new());
    let status = drain(&mut engine);

    assert!(matches!(status, StepStatus::Error(CommandError::InvalidTarget)));
    assert_eq!(engine.state().object(card).unwrap().zone, ZoneKind::Hand, "cast never happened");
    assert_eq!(engine.state().ledger.len(), 0);
}

#[test]
fn cast_spell_rejects_unrequested_target() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let creature = ObjectId(0);
    state.objects.insert(creature, card_object(creature, creature_definition(1, "Bear"), p0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(creature);
    // creature_definition's own card, cast as a spell straight from hand,
    // has target_requirement: None - supplying a target anyway is illegal.
    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, creature_definition(2, "Bear2"), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(creature)]);
    let status = drain(&mut engine);

    assert!(matches!(status, StepStatus::Error(CommandError::InvalidTarget)));
}

#[test]
fn cast_spell_rejects_target_failing_filter() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    // pump_definition requires "target creature you control" - a creature
    // controlled by the opponent fails the FilterController::You check.
    let opponents_creature = ObjectId(0);
    state.objects.insert(
        opponents_creature,
        card_object(opponents_creature, creature_definition(1, "Bear"), p1, ZoneKind::Battlefield),
    );
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(opponents_creature);
    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, pump_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(opponents_creature)]);
    let status = drain(&mut engine);

    assert!(matches!(status, StepStatus::Error(CommandError::InvalidTarget)));
}

#[test]
fn cast_spell_rejects_duplicate_targets() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let creature = ObjectId(0);
    state.objects.insert(creature, card_object(creature, creature_definition(1, "Bear"), p0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(creature);
    // A hypothetical 2-target requirement choosing the same object twice -
    // reuse bolt_definition's shape but bump count to 2 inline.
    let mut two_target_bolt = bolt_definition();
    two_target_bolt.target_requirement =
        Some(TargetSpec::Primitive(TargetRequirement { filter: TargetFilter::AnyTarget, count: 2, up_to: false, at_least: false, exclude_source: false }));
    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, two_target_bolt, p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    // CR 115.3: the same target can't be chosen twice for one instance of
    // the word "target".
    cast_spell(&mut engine, p0, card, vec![Target::Object(creature), Target::Object(creature)]);
    let status = drain(&mut engine);

    assert!(matches!(status, StepStatus::Error(CommandError::InvalidTarget)));
}

#[test]
fn cast_spell_accepts_legal_target_and_bakes_it_onto_stack_object() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let opponent = ObjectId(0);
    state.objects.insert(opponent, card_object(opponent, creature_definition(1, "Bear"), p1, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(opponent);
    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, bolt_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(opponent)]);
    let status = pay_free_and_drain(&mut engine);

    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert_eq!(engine.state().stack.len(), 1);
    let stack_object = &engine.state().stack[0];
    assert_eq!(stack_object.targets, vec![Target::Object(opponent)]);
    assert!(matches!(stack_object.target_requirement, Some(TargetSpec::Primitive(_))));
}

/// CR 115.4: `AnyTarget` accepts a creature but rejects a noncreature
/// artifact.
#[test]
fn any_target_filter_rejects_noncreature_artifact() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let artifact = ObjectId(0);
    state.objects.insert(artifact, card_object(artifact, artifact_definition(), p0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(artifact);
    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, bolt_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(artifact)]);
    let status = drain(&mut engine);

    assert!(matches!(status, StepStatus::Error(CommandError::InvalidTarget)));
}

/// CR 608.2b: the target creature is removed from the battlefield after the
/// spell is cast (simulating an opponent's instant-speed removal resolving
/// first) but before this spell resolves - by resolution time its only
/// target is illegal, so it fizzles: no effect runs, it's removed from the
/// stack, and since it's a spell it goes to its owner's graveyard (not
/// invoking any card script, unlike a normal permanent resolution).
#[test]
fn resolution_fizzles_when_only_target_becomes_illegal() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let target_creature = ObjectId(0);
    state.objects.insert(
        target_creature,
        card_object(target_creature, creature_definition(1, "Bear"), p1, ZoneKind::Battlefield),
    );
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(target_creature);
    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, bolt_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(target_creature)]);
    pay_free_and_drain(&mut engine);
    assert_eq!(engine.state().stack.len(), 1);

    // Simulate the target leaving the battlefield via some other effect
    // between casting and this spell resolving - the mechanism isn't this
    // test's concern, only that ResolveStackTop rechecks legality against
    // whatever state.objects says now.
    engine.state_mut().zone_mut(ZoneKind::Battlefield, None).unwrap().remove(target_creature);
    engine.state_mut().objects.remove(&target_creature);

    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(&mut engine);
    engine.submit_action(PlayerAction { player: p1, kind: PlayerActionKind::PassPriority }).unwrap();
    let status = drain(&mut engine);

    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert!(engine.state().stack.is_empty(), "fizzled spell still leaves the stack");

    let fizzled = engine
        .state()
        .ledger
        .iter()
        .any(|e| matches!(e.kind, GameEventKind::Fizzled { .. }));
    assert!(fizzled, "expected a Fizzled event");
    let resolved_event = engine
        .state()
        .ledger
        .iter()
        .any(|e| matches!(e.kind, GameEventKind::StackObjectResolved { .. }));
    assert!(!resolved_event, "a fizzled spell/ability never emits StackObjectResolved");

    // CR 608.2b: a spell that fizzles always goes to its owner's graveyard,
    // even though bolt_definition is an instant anyway (so this coincides
    // with normal 608.2m routing) - the important part is *no battlefield
    // permanent* and *no effect ran*.
    let in_graveyard = engine
        .state()
        .zones
        .iter()
        .find(|z| z.kind == ZoneKind::Graveyard && z.owner == Some(p0))
        .unwrap();
    assert_eq!(in_graveyard.contents.len(), 1, "fizzled spell landed in its owner's graveyard");
}

/// Same setup as the fizzle test, but the target is still legal at
/// resolution - proceeds exactly as before this slice (StackObjectResolved,
/// no Fizzled event).
#[test]
fn resolution_succeeds_when_target_remains_legal() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let target_creature = ObjectId(0);
    state.objects.insert(
        target_creature,
        card_object(target_creature, creature_definition(1, "Bear"), p1, ZoneKind::Battlefield),
    );
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(target_creature);
    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, bolt_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(target_creature)]);
    pay_free_and_drain(&mut engine);

    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(&mut engine);
    engine.submit_action(PlayerAction { player: p1, kind: PlayerActionKind::PassPriority }).unwrap();
    let status = drain(&mut engine);

    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert!(engine.state().stack.is_empty());
    let resolved = engine
        .state()
        .ledger
        .iter()
        .any(|e| matches!(e.kind, GameEventKind::StackObjectResolved { .. }));
    assert!(resolved);
    let fizzled = engine.state().ledger.iter().any(|e| matches!(e.kind, GameEventKind::Fizzled { .. }));
    assert!(!fizzled);
}
