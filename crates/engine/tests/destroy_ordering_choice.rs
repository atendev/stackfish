//! CR 616.1: scripted `Destroy`, `Sacrifice` and `Exile` ops pause on `AwaitingReplacementChoice` (`ReplacementResumeTarget::ScriptedZoneMove`) when a target's proposed event matches two or more replacement effects.
//! The per-target loop carries the rest of the op's target queue, so the remaining independent targets still resolve once `submit_replacement_choice` answers.
//! Exile has no end-to-end test: `EventFilter` has only `DamageDealt` and `WouldDie`, and a scripted Exile's `to: Exile` matches neither.
// TODO: a Lua builder for replacement effects and more `EventFilter` shapes, so an Exile ordering choice can be tested

use std::collections::HashMap;

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::replacement::ReplacementCandidate;
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, two_player_state, zones};

fn creature_definition(definition_id: u64, name: &str, toughness: i32) -> CardDefinition {
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

fn shield_definition(definition_id: u64, name: &str) -> CardDefinition {
    CardDefinition {
        definition_id,
        name: name.to_string(),
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
        script_ref: ScriptReference::ByKey("test_shield".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn regenerate_shield(source: ObjectId, consumption: ConsumptionPolicy) -> ReplacementEffect {
    ReplacementEffect {
        source,
        spec: ReplacementSpec::Primitive {
            filter: EventFilter::WouldDie {
                affected: EffectFilter {
                    card_types: Some(CardTypeSet::CREATURE),
                    controller: None,
                    self_only: true,
                    subtypes: None,
                    excluded_subtypes: None,
                    supertypes: None,
                    excluded_supertypes: None,
                },
            },
            payload: ReplacementPayload::Regenerate,
        },
        consumption,
    }
}

// `controller: Some(You)` keeps the effect to its controller's creatures, since a multi-target test also has an opponent's creature on the battlefield.
fn exile_instead_effect(
    source: ObjectId,
    self_only: bool,
    consumption: ConsumptionPolicy,
) -> ReplacementEffect {
    ReplacementEffect {
        source,
        spec: ReplacementSpec::Primitive {
            filter: EventFilter::WouldDie {
                affected: EffectFilter {
                    card_types: Some(CardTypeSet::CREATURE),
                    controller: Some(FilterController::You),
                    self_only,
                    subtypes: None,
                    excluded_subtypes: None,
                    supertypes: None,
                    excluded_supertypes: None,
                },
            },
            payload: ReplacementPayload::ReplaceZoneDestination(ZoneKind::Exile),
        },
        consumption,
    }
}

fn destroy_spell_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 20,
        name: "Test Destroy".to_string(),
        mana_cost: ManaCost {
            symbols: Vec::new(),
        },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::INSTANT,
        subtypes: Vec::new(),
        oracle_text: "Destroy target creature.".to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_destroy".to_string()),
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
                controller: None,
                zone: None,
            },
            count: 1,
            up_to: false,
            at_least: false,
            exclude_source: false,
        })),
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: Some(AbilityEffect::Destroy),
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn slay_two_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 21,
        name: "Test Slay Two".to_string(),
        mana_cost: ManaCost {
            symbols: Vec::new(),
        },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::INSTANT,
        subtypes: Vec::new(),
        oracle_text: "Destroy two target creatures.".to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_slay_two".to_string()),
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
                controller: None,
                zone: None,
            },
            count: 2,
            up_to: false,
            at_least: false,
            exclude_source: false,
        })),
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: Some(AbilityEffect::Destroy),
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn sacrifice_spell_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 22,
        name: "Test Sacrifice".to_string(),
        mana_cost: ManaCost {
            symbols: Vec::new(),
        },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::INSTANT,
        subtypes: Vec::new(),
        oracle_text: "Sacrifice target creature.".to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_sacrifice".to_string()),
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
                controller: None,
                zone: None,
            },
            count: 1,
            up_to: false,
            at_least: false,
            exclude_source: false,
        })),
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: Some(AbilityEffect::Sacrifice),
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn base_state() -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        step: Step::PreCombatMain,
        last_active_turn_number: HashMap::from([(p0, 1), (p1, 1)]),
        ..two_player_state(zones(
            p0,
            p1,
            &[
                ZoneKind::Hand,
                ZoneKind::Battlefield,
                ZoneKind::Graveyard,
                ZoneKind::Exile,
            ],
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

fn put_in_hand(state: &mut GameState, object: GameObject) {
    let id = object.object_id;
    let owner = object.owner;
    state
        .zone_mut(ZoneKind::Hand, Some(owner))
        .unwrap()
        .push_bottom(id);
    state.objects.insert(id, object);
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

fn pass_both_and_drain(engine: &mut Engine, p0: PlayerId, p1: PlayerId) -> StepStatus {
    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::PassPriority,
        })
        .unwrap();
    drain(engine);
    engine
        .submit_action(PlayerAction {
            player: p1,
            kind: PlayerActionKind::PassPriority,
        })
        .unwrap();
    drain(engine)
}

#[test]
fn destroy_spell_with_two_matching_effects_pauses_instead_of_silently_no_oping() {
    // The cast must pause for the ordering choice and not drain through to AwaitingAction with the bear untouched.
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let bear = ObjectId(0);
    let shield_obj = ObjectId(1);
    let destroy_spell = ObjectId(2);
    let mut state = base_state();
    put_on_battlefield(
        &mut state,
        card_object(
            bear,
            creature_definition(1, "Bear", 2),
            p0,
            ZoneKind::Battlefield,
        ),
    );
    put_on_battlefield(
        &mut state,
        card_object(
            shield_obj,
            shield_definition(2, "Test Shield"),
            p0,
            ZoneKind::Battlefield,
        ),
    );
    put_in_hand(
        &mut state,
        card_object(
            destroy_spell,
            destroy_spell_definition(),
            p0,
            ZoneKind::Hand,
        ),
    );
    state
        .replacement_effects
        .push(regenerate_shield(bear, ConsumptionPolicy::OneShot));
    state.replacement_effects.push(exile_instead_effect(
        shield_obj,
        false,
        ConsumptionPolicy::OneShot,
    ));

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, destroy_spell, vec![Target::Object(bear)]);
    pay_free_and_drain(&mut engine);
    let status = pass_both_and_drain(&mut engine, p0, p1);

    let StepStatus::AwaitingReplacementChoice { player, candidates } = status else {
        panic!(
            "2 matching replacement effects on the scripted Destroy op must pause for a CR 616.1 choice, got {status:?}"
        );
    };
    assert_eq!(player, p0);
    assert_eq!(candidates.len(), 2);
    // Nothing applied yet: the bear is on the battlefield undamaged and both effects are still registered.
    assert_eq!(
        engine.state().object(bear).unwrap().zone,
        ZoneKind::Battlefield
    );
    assert_eq!(engine.state().replacement_effects.len(), 2);
}

#[test]
fn destroy_spell_choosing_the_regeneration_shield_saves_the_creature() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let bear = ObjectId(0);
    let shield_obj = ObjectId(1);
    let destroy_spell = ObjectId(2);
    let mut state = base_state();
    put_on_battlefield(
        &mut state,
        card_object(
            bear,
            creature_definition(1, "Bear", 2),
            p0,
            ZoneKind::Battlefield,
        ),
    );
    put_on_battlefield(
        &mut state,
        card_object(
            shield_obj,
            shield_definition(2, "Test Shield"),
            p0,
            ZoneKind::Battlefield,
        ),
    );
    put_in_hand(
        &mut state,
        card_object(
            destroy_spell,
            destroy_spell_definition(),
            p0,
            ZoneKind::Hand,
        ),
    );
    state
        .replacement_effects
        .push(regenerate_shield(bear, ConsumptionPolicy::OneShot));
    state.replacement_effects.push(exile_instead_effect(
        shield_obj,
        false,
        ConsumptionPolicy::OneShot,
    ));

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, destroy_spell, vec![Target::Object(bear)]);
    pay_free_and_drain(&mut engine);
    let status = pass_both_and_drain(&mut engine, p0, p1);
    let StepStatus::AwaitingReplacementChoice { candidates, .. } = status else {
        panic!("expected the CR 616.1 pause, got {status:?}");
    };

    assert!(candidates.contains(&ReplacementCandidate::Registered(bear)));
    engine
        .submit_replacement_choice(ReplacementCandidate::Registered(bear))
        .expect("bear is the regeneration shield's own source, a real candidate");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));

    let survivor = engine
        .state()
        .object(bear)
        .expect("regeneration kept the same ObjectId, still on the battlefield");
    assert_eq!(survivor.zone, ZoneKind::Battlefield);
    assert_eq!(survivor.damage_marked, 0);
    assert!(
        survivor.tapped,
        "CR 701.19: the controller taps a regenerated permanent"
    );
    assert!(
        engine
            .state()
            .zone(ZoneKind::Exile, None)
            .unwrap()
            .contents
            .is_empty(),
        "the exile-instead effect must not also have applied - only the chosen one wins"
    );
    assert_eq!(
        engine.state().replacement_effects.len(),
        1,
        "the OneShot shield was consumed, the loser was never even evaluated"
    );
    assert_eq!(engine.state().replacement_effects[0].source, shield_obj);
}

#[test]
fn destroy_spell_choosing_the_other_effect_first_lets_it_win_instead() {
    // Same fixture, opposite choice: the order determines the outcome, and the regeneration shield does not always win.
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let bear = ObjectId(0);
    let shield_obj = ObjectId(1);
    let destroy_spell = ObjectId(2);
    let mut state = base_state();
    put_on_battlefield(
        &mut state,
        card_object(
            bear,
            creature_definition(1, "Bear", 2),
            p0,
            ZoneKind::Battlefield,
        ),
    );
    put_on_battlefield(
        &mut state,
        card_object(
            shield_obj,
            shield_definition(2, "Test Shield"),
            p0,
            ZoneKind::Battlefield,
        ),
    );
    put_in_hand(
        &mut state,
        card_object(
            destroy_spell,
            destroy_spell_definition(),
            p0,
            ZoneKind::Hand,
        ),
    );
    state
        .replacement_effects
        .push(regenerate_shield(bear, ConsumptionPolicy::OneShot));
    state.replacement_effects.push(exile_instead_effect(
        shield_obj,
        false,
        ConsumptionPolicy::OneShot,
    ));

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, destroy_spell, vec![Target::Object(bear)]);
    pay_free_and_drain(&mut engine);
    let status = pass_both_and_drain(&mut engine, p0, p1);
    assert!(matches!(
        status,
        StepStatus::AwaitingReplacementChoice { .. }
    ));

    engine
        .submit_replacement_choice(ReplacementCandidate::Registered(shield_obj))
        .expect("shield_obj is the exile-instead effect's own source, a real candidate");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));

    assert!(
        engine.state().object(bear).is_none(),
        "the bear's original ObjectId retired - it left the battlefield via CR 400.7"
    );
    assert!(
        !engine
            .state()
            .zone(ZoneKind::Exile, None)
            .unwrap()
            .contents
            .is_empty(),
        "the exile-instead effect won this time - the bear should be in Exile, not the graveyard"
    );
    // The exile effect was consumed. The shield no longer matches WouldDie once the destination is Exile, and `retire_object` purges it because its self_only id retired (CR 400.7).
    assert_eq!(
        engine.state().replacement_effects.len(),
        0,
        "both effects are gone - one consumed by applying, the other purged because its pinned source \
         (bear's old, now-retired ObjectId) left the game"
    );
}

#[test]
fn destroy_two_target_spell_pauses_on_one_target_then_still_destroys_the_other_afterward() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let bear = ObjectId(0);
    let shield_obj = ObjectId(1);
    let plain_bear = ObjectId(2);
    let slay_two = ObjectId(3);
    let mut state = base_state();
    put_on_battlefield(
        &mut state,
        card_object(
            bear,
            creature_definition(1, "Bear", 2),
            p0,
            ZoneKind::Battlefield,
        ),
    );
    put_on_battlefield(
        &mut state,
        card_object(
            shield_obj,
            shield_definition(2, "Test Shield"),
            p0,
            ZoneKind::Battlefield,
        ),
    );
    // No replacement effects target this creature, independent of the first target's ordering choice.
    put_on_battlefield(
        &mut state,
        card_object(
            plain_bear,
            creature_definition(3, "Plain Bear", 2),
            p1,
            ZoneKind::Battlefield,
        ),
    );
    put_in_hand(
        &mut state,
        card_object(slay_two, slay_two_definition(), p0, ZoneKind::Hand),
    );
    state
        .replacement_effects
        .push(regenerate_shield(bear, ConsumptionPolicy::OneShot));
    state.replacement_effects.push(exile_instead_effect(
        shield_obj,
        false,
        ConsumptionPolicy::OneShot,
    ));

    let mut engine = Engine::new(state);
    cast_spell(
        &mut engine,
        p0,
        slay_two,
        vec![Target::Object(bear), Target::Object(plain_bear)],
    );
    pay_free_and_drain(&mut engine);
    let status = pass_both_and_drain(&mut engine, p0, p1);
    let StepStatus::AwaitingReplacementChoice { candidates, .. } = status else {
        panic!("expected bear's 2 matching effects to pause, got {status:?}");
    };
    assert_eq!(candidates.len(), 2);
    // plain_bear hasn't been touched by the paused target's own choice yet.
    assert_eq!(
        engine.state().object(plain_bear).unwrap().zone,
        ZoneKind::Battlefield
    );

    engine
        .submit_replacement_choice(ReplacementCandidate::Registered(bear))
        .expect("a real candidate");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));

    let survivor = engine.state().object(bear).expect("saved by regeneration");
    assert_eq!(survivor.zone, ZoneKind::Battlefield);
    assert!(
        engine.state().object(plain_bear).is_none(),
        "the second, independent target must still be destroyed after the first target's own choice resolves - \
         this is the core of the fix: the remaining queue keeps draining, it doesn't stop at the first pause"
    );
    let plain_bear_grave = engine.state().zone(ZoneKind::Graveyard, Some(p1)).unwrap();
    assert!(
        plain_bear_grave
            .contents
            .iter()
            .any(|id| engine.state().object(*id).is_some()),
        "plain_bear landed in its owner's graveyard"
    );
}

#[test]
fn sacrifice_spell_with_two_matching_effects_pauses_and_only_the_chosen_one_is_consumed() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let bear = ObjectId(0);
    let shield_obj = ObjectId(1);
    let sac_spell = ObjectId(2);
    let mut state = base_state();
    put_on_battlefield(
        &mut state,
        card_object(
            bear,
            creature_definition(1, "Bear", 2),
            p0,
            ZoneKind::Battlefield,
        ),
    );
    put_on_battlefield(
        &mut state,
        card_object(
            shield_obj,
            shield_definition(2, "Test Shield"),
            p0,
            ZoneKind::Battlefield,
        ),
    );
    put_in_hand(
        &mut state,
        card_object(sac_spell, sacrifice_spell_definition(), p0, ZoneKind::Hand),
    );
    // self_only:true (the bear itself) and self_only:false (a separate permanent) are two sources matching the same proposed ZoneChange.
    state
        .replacement_effects
        .push(exile_instead_effect(bear, true, ConsumptionPolicy::OneShot));
    state.replacement_effects.push(exile_instead_effect(
        shield_obj,
        false,
        ConsumptionPolicy::OneShot,
    ));

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, sac_spell, vec![Target::Object(bear)]);
    pay_free_and_drain(&mut engine);
    let status = pass_both_and_drain(&mut engine, p0, p1);
    let StepStatus::AwaitingReplacementChoice { candidates, .. } = status else {
        panic!("2 matching effects on the scripted Sacrifice op must pause, got {status:?}");
    };
    assert_eq!(candidates.len(), 2);

    engine
        .submit_replacement_choice(ReplacementCandidate::Registered(bear))
        .expect("bear (self_only) is a real candidate");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));

    assert!(
        engine.state().object(bear).is_none(),
        "the bear's original ObjectId retired"
    );
    assert!(
        !engine
            .state()
            .zone(ZoneKind::Exile, None)
            .unwrap()
            .contents
            .is_empty(),
        "sacrificed and exiled instead of hitting the graveyard"
    );
    assert_eq!(
        engine.state().replacement_effects.len(),
        1,
        "the chosen effect (OneShot) consumed itself; the other was never evaluated since the rewritten \
         event's own `to` was no longer Graveyard on recheck"
    );
    assert_eq!(
        engine.state().replacement_effects[0].source,
        shield_obj,
        "the un-chosen effect is the one left sitting unconsumed"
    );
}
