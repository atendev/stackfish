//! CR 115, 601.2c-e, 608.2b: fixed-count `TargetRequirement` validation on `CastSpell` and `ActivateAbility` (count, filter, dedup: CR 601.2c, 601.2e, 602.2b) and the CR 608.2b all-targets-illegal recheck at `ResolveStackTop`.

use stackfish_engine::commands::Rejected;
use stackfish_engine::engine::ActionError;
use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::*;
use stackfish_testutil::{
    card_object, drain, hand_battlefield_graveyard_zones, two_player_state as base_two_player_state,
};

fn creature_definition(definition_id: u64, name: &str) -> CardDefinition {
    CardDefinition {
        definition_id,
        name: name.to_string(),
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

fn artifact_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 10,
        name: "Test Artifact".to_string(),
        mana_cost: ManaCost {
            symbols: Vec::new(),
        },
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
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn bolt_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 20,
        name: "Test Bolt".to_string(),
        mana_cost: ManaCost {
            symbols: Vec::new(),
        },
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
            up_to: false,
            at_least: false,
            exclude_source: false,
        })),
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn pump_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 21,
        name: "Test Pump".to_string(),
        mana_cost: ManaCost {
            symbols: Vec::new(),
        },
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
            filter: TargetFilter::Object {
                mana_value: None,
                excluded_card_types: None,
                excluded_subtypes: None,
                card_types: Some(CardTypeSet::CREATURE),
                subtypes: None,
                supertypes: None,
                excluded_supertypes: None,
                controller: Some(FilterController::You),
                zone: None,
            },
            count: 1,
            up_to: false,
            at_least: false,
            exclude_source: false,
        })),
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn two_player_state() -> GameState {
    base_two_player_state(hand_battlefield_graveyard_zones(PlayerId(0), PlayerId(1)))
}

fn pay_free_and_drain(engine: &mut Engine) -> StepStatus {
    let status = drain(engine);
    assert!(
        matches!(status, StepStatus::AwaitingPayment { .. }),
        "expected a pause for the cast's (free) cost, got {status:?}"
    );
    engine
        .submit_payment(PaymentPlan {
            payments: Vec::new(),
        })
        .expect("empty plan pays a {0} cost");
    drain(engine)
}

fn cast_spell(engine: &mut Engine, player: PlayerId, card: ObjectId, targets: Vec<Target>) {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::CastSpell {
                card,
                modes: Vec::new(),
                targets,
                x_value: None,
                alternative_cost: None,
                pay_buyback: false,
            },
        })
        .expect("player holds priority, spell is in hand");
}

fn try_cast_spell(
    engine: &mut Engine,
    player: PlayerId,
    card: ObjectId,
    targets: Vec<Target>,
) -> Result<(), ActionError> {
    engine.submit_action(PlayerAction {
        player,
        kind: PlayerActionKind::CastSpell {
            card,
            modes: Vec::new(),
            targets,
            x_value: None,
            alternative_cost: None,
            pay_buyback: false,
        },
    })
}

#[test]
fn cast_spell_rejects_wrong_target_count() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let creature = ObjectId(0);
    state.objects.insert(
        creature,
        card_object(
            creature,
            creature_definition(1, "Bear"),
            p0,
            ZoneKind::Battlefield,
        ),
    );
    state
        .zone_mut(ZoneKind::Battlefield, None)
        .unwrap()
        .push_bottom(creature);
    let card = ObjectId(2);
    state.objects.insert(
        card,
        card_object(card, bolt_definition(), p0, ZoneKind::Hand),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);

    let mut engine = Engine::new(state);
    // bolt_definition requires exactly 1 target, so 0 or 2 is illegal (CR 601.2c).
    let err = try_cast_spell(&mut engine, p0, card, Vec::new()).unwrap_err();

    assert!(matches!(
        err,
        ActionError::Rejected(Rejected::InvalidTarget)
    ));
    assert_eq!(
        engine.state().object(card).unwrap().zone,
        ZoneKind::Hand,
        "cast never happened"
    );
    assert_eq!(engine.state().ledger.len(), 0);
}

#[test]
fn cast_spell_rejects_unrequested_target() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let creature = ObjectId(0);
    state.objects.insert(
        creature,
        card_object(
            creature,
            creature_definition(1, "Bear"),
            p0,
            ZoneKind::Battlefield,
        ),
    );
    state
        .zone_mut(ZoneKind::Battlefield, None)
        .unwrap()
        .push_bottom(creature);
    // creature_definition has target_requirement: None, so supplying a target is illegal.
    let card = ObjectId(2);
    state.objects.insert(
        card,
        card_object(card, creature_definition(2, "Bear2"), p0, ZoneKind::Hand),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);

    let mut engine = Engine::new(state);
    let err = try_cast_spell(&mut engine, p0, card, vec![Target::Object(creature)]).unwrap_err();

    assert!(matches!(
        err,
        ActionError::Rejected(Rejected::InvalidTarget)
    ));
}

#[test]
fn cast_spell_rejects_target_failing_filter() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    // pump_definition requires "target creature you control", so an opponent's creature fails FilterController::You.
    let opponents_creature = ObjectId(0);
    state.objects.insert(
        opponents_creature,
        card_object(
            opponents_creature,
            creature_definition(1, "Bear"),
            p1,
            ZoneKind::Battlefield,
        ),
    );
    state
        .zone_mut(ZoneKind::Battlefield, None)
        .unwrap()
        .push_bottom(opponents_creature);
    let card = ObjectId(2);
    state.objects.insert(
        card,
        card_object(card, pump_definition(), p0, ZoneKind::Hand),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);

    let mut engine = Engine::new(state);
    let err = try_cast_spell(
        &mut engine,
        p0,
        card,
        vec![Target::Object(opponents_creature)],
    )
    .unwrap_err();

    assert!(matches!(
        err,
        ActionError::Rejected(Rejected::InvalidTarget)
    ));
}

#[test]
fn cast_spell_rejects_duplicate_targets() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let creature = ObjectId(0);
    state.objects.insert(
        creature,
        card_object(
            creature,
            creature_definition(1, "Bear"),
            p0,
            ZoneKind::Battlefield,
        ),
    );
    state
        .zone_mut(ZoneKind::Battlefield, None)
        .unwrap()
        .push_bottom(creature);
    // A hypothetical 2-target requirement choosing the same object twice, from bolt_definition with count bumped to 2.
    let mut two_target_bolt = bolt_definition();
    two_target_bolt.target_requirement = Some(TargetSpec::Primitive(TargetRequirement {
        filter: TargetFilter::AnyTarget,
        count: 2,
        up_to: false,
        at_least: false,
        exclude_source: false,
    }));
    let card = ObjectId(2);
    state
        .objects
        .insert(card, card_object(card, two_target_bolt, p0, ZoneKind::Hand));
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);

    let mut engine = Engine::new(state);
    // CR 115.3: the same target can't be chosen twice for one instance of
    // the word "target".
    let err = try_cast_spell(
        &mut engine,
        p0,
        card,
        vec![Target::Object(creature), Target::Object(creature)],
    )
    .unwrap_err();

    assert!(matches!(
        err,
        ActionError::Rejected(Rejected::InvalidTarget)
    ));
}

#[test]
fn cast_spell_accepts_legal_target_and_bakes_it_onto_stack_object() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let opponent = ObjectId(0);
    state.objects.insert(
        opponent,
        card_object(
            opponent,
            creature_definition(1, "Bear"),
            p1,
            ZoneKind::Battlefield,
        ),
    );
    state
        .zone_mut(ZoneKind::Battlefield, None)
        .unwrap()
        .push_bottom(opponent);
    let card = ObjectId(2);
    state.objects.insert(
        card,
        card_object(card, bolt_definition(), p0, ZoneKind::Hand),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(opponent)]);
    let status = pay_free_and_drain(&mut engine);

    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert_eq!(engine.state().stack.len(), 1);
    let stack_object = &engine.state().stack[0];
    assert_eq!(stack_object.targets, vec![Target::Object(opponent)]);
    assert!(matches!(
        stack_object.target_requirement,
        Some(TargetSpec::Primitive(_))
    ));
}

// CR 115.4
// AnyTarget accepts a creature but rejects a noncreature artifact.
#[test]
fn any_target_filter_rejects_noncreature_artifact() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let artifact = ObjectId(0);
    state.objects.insert(
        artifact,
        card_object(artifact, artifact_definition(), p0, ZoneKind::Battlefield),
    );
    state
        .zone_mut(ZoneKind::Battlefield, None)
        .unwrap()
        .push_bottom(artifact);
    let card = ObjectId(2);
    state.objects.insert(
        card,
        card_object(card, bolt_definition(), p0, ZoneKind::Hand),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);

    let mut engine = Engine::new(state);
    let err = try_cast_spell(&mut engine, p0, card, vec![Target::Object(artifact)]).unwrap_err();

    assert!(matches!(
        err,
        ActionError::Rejected(Rejected::InvalidTarget)
    ));
}

// CR 608.2b
// The target leaves the battlefield before the spell resolves, so it fizzles: no effect, removed from the stack, and a spell goes to its owner's graveyard without invoking a card script.
#[test]
fn resolution_fizzles_when_only_target_becomes_illegal() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let target_creature = ObjectId(0);
    state.objects.insert(
        target_creature,
        card_object(
            target_creature,
            creature_definition(1, "Bear"),
            p1,
            ZoneKind::Battlefield,
        ),
    );
    state
        .zone_mut(ZoneKind::Battlefield, None)
        .unwrap()
        .push_bottom(target_creature);
    let card = ObjectId(2);
    state.objects.insert(
        card,
        card_object(card, bolt_definition(), p0, ZoneKind::Hand),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(target_creature)]);
    pay_free_and_drain(&mut engine);
    assert_eq!(engine.state().stack.len(), 1);

    // The target leaves the battlefield by some other effect, and ResolveStackTop rechecks legality against state.objects.
    engine
        .state_mut()
        .zone_mut(ZoneKind::Battlefield, None)
        .unwrap()
        .remove(target_creature);
    engine.state_mut().objects.remove(&target_creature);

    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::PassPriority,
        })
        .unwrap();
    drain(&mut engine);
    engine
        .submit_action(PlayerAction {
            player: p1,
            kind: PlayerActionKind::PassPriority,
        })
        .unwrap();
    let status = drain(&mut engine);

    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert!(
        engine.state().stack.is_empty(),
        "fizzled spell still leaves the stack"
    );

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
    assert!(
        !resolved_event,
        "a fizzled spell/ability never emits StackObjectResolved"
    );

    // CR 608.2b: a fizzled spell goes to its owner's graveyard. The point is that no battlefield permanent and no effect ran.
    let in_graveyard = engine
        .state()
        .zones
        .iter()
        .find(|z| z.kind == ZoneKind::Graveyard && z.owner == Some(p0))
        .unwrap();
    assert_eq!(
        in_graveyard.contents.len(),
        1,
        "fizzled spell landed in its owner's graveyard"
    );
}

// With the target still legal at resolution, the spell resolves normally (StackObjectResolved, no Fizzled event).
#[test]
fn resolution_succeeds_when_target_remains_legal() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let target_creature = ObjectId(0);
    state.objects.insert(
        target_creature,
        card_object(
            target_creature,
            creature_definition(1, "Bear"),
            p1,
            ZoneKind::Battlefield,
        ),
    );
    state
        .zone_mut(ZoneKind::Battlefield, None)
        .unwrap()
        .push_bottom(target_creature);
    let card = ObjectId(2);
    state.objects.insert(
        card,
        card_object(card, bolt_definition(), p0, ZoneKind::Hand),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(target_creature)]);
    pay_free_and_drain(&mut engine);

    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::PassPriority,
        })
        .unwrap();
    drain(&mut engine);
    engine
        .submit_action(PlayerAction {
            player: p1,
            kind: PlayerActionKind::PassPriority,
        })
        .unwrap();
    let status = drain(&mut engine);

    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert!(engine.state().stack.is_empty());
    let resolved = engine
        .state()
        .ledger
        .iter()
        .any(|e| matches!(e.kind, GameEventKind::StackObjectResolved { .. }));
    assert!(resolved);
    let fizzled = engine
        .state()
        .ledger
        .iter()
        .any(|e| matches!(e.kind, GameEventKind::Fizzled { .. }));
    assert!(!fizzled);
}
