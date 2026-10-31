//! CR 118.12, 118.12a: `AbilityEffect::UnlessPaid` is the shared "[do something] unless [a player does something else]" mechanism, with `UnlessPaidConsequence::{Counter, SacrificeSelf}` serving Mana Leak, Ward (CR 702.21a) and Echo (CR 702.30a). `Game.Ops.CounterUnlessPaid("{3}")` builds the `Counter` shape.
//! The multi-candidate `payer_queue` (CR 101.4 APNAP) has no producer longer than one element, so its tests are in-crate unit tests in `engine/tests.rs`.
// TODO: a Lua builder for `SacrificeSelf` (see `AbilityEffect::UnlessPaid`), so its tests push the `StackObject` by hand

use std::collections::HashMap;

use stackfish_engine::engine::{Engine, StepStatus, UnlessPaidError};
use stackfish_engine::replacement::ReplacementCandidate;
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, two_player_state as base_two_player_state, zones};

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

fn mana_leak_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 31,
        name: "Test Mana Leak".to_string(),
        mana_cost: ManaCost {
            symbols: Vec::new(),
        },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::INSTANT,
        subtypes: Vec::new(),
        oracle_text: "Counter target spell unless its controller pays {3}.".to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_mana_leak".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: Some(TargetSpec::Primitive(TargetRequirement {
            filter: TargetFilter::Object {
                mana_value: None,
                card_types: None,
                excluded_card_types: None,
                subtypes: None,
                excluded_subtypes: None,
                supertypes: None,
                excluded_supertypes: None,
                controller: None,
                zone: Some(ZoneKind::Stack),
            },
            count: 1,
            up_to: false,
            at_least: false,
            exclude_source: false,
        })),
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: Some(AbilityEffect::UnlessPaid {
            cost: CostSpec::mana_only(ManaCost::parse("{3}").expect("valid mana cost")),
            otherwise: UnlessPaidConsequence::Counter,
        }),
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn two_player_state() -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        step: Step::PreCombatMain,
        last_active_turn_number: HashMap::from([(p0, 1), (p1, 1)]),
        ..base_two_player_state(zones(
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

fn two_player_state_with_p1_colorless(colorless: u32) -> GameState {
    let mut state = two_player_state();
    state.players[1].mana_pool.colorless = colorless;
    state
}

fn push_creature_spell_on_stack(state: &mut GameState, stack_id: ObjectId, owner: PlayerId) {
    state.objects.insert(
        stack_id,
        card_object(
            stack_id,
            creature_definition(1, "Bear"),
            owner,
            ZoneKind::Stack,
        ),
    );
    state.stack.push(StackObject {
        stack_id,
        kind: StackEntryKind::Spell,
        source: stack_id,
        source_lki: None,
        owner,
        controller: owner,
        chosen_modes: Vec::new(),
        targets: Vec::new(),
        target_requirement: None,
        x_value: None,
        exile_on_leaving_stack: false,
        bought_back: false,
        escaped: false,
        costs_paid: Vec::new(),
        operation: ScriptHandle(0),
        effect: None,
        intervening_if: None,
        pending_mode_choice: None,
    });
}

fn push_echo_trigger_on_stack(
    state: &mut GameState,
    stack_id: ObjectId,
    source: ObjectId,
    controller: PlayerId,
    cost: &str,
) {
    state.stack.push(StackObject {
        stack_id,
        kind: StackEntryKind::TriggeredAbility,
        source,
        source_lki: None,
        owner: controller,
        controller,
        chosen_modes: Vec::new(),
        targets: Vec::new(),
        target_requirement: None,
        x_value: None,
        exile_on_leaving_stack: false,
        bought_back: false,
        escaped: false,
        costs_paid: Vec::new(),
        operation: ScriptHandle(0),
        effect: Some(AbilityEffect::UnlessPaid {
            cost: CostSpec::mana_only(ManaCost::parse(cost).expect("valid mana cost")),
            otherwise: UnlessPaidConsequence::SacrificeSelf,
        }),
        intervening_if: None,
        pending_mode_choice: None,
    });
}

fn put_on_battlefield(state: &mut GameState, object: GameObject) {
    let id = object.object_id;
    state
        .zone_mut(ZoneKind::Battlefield, None)
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

// CR 701.6a/118.12
#[test]
fn declining_to_pay_counters_the_target_the_same_way_plain_counter_does() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let target = ObjectId(0);
    push_creature_spell_on_stack(&mut state, target, p1);

    let card = ObjectId(2);
    state.objects.insert(
        card,
        card_object(card, mana_leak_definition(), p0, ZoneKind::Hand),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(target)]);
    pay_free_and_drain(&mut engine);
    assert_eq!(
        engine.state().stack.len(),
        2,
        "both the target and Mana Leak are on the stack"
    );

    // Mana Leak resolves first (LIFO), pausing for p1's decision.
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
    let (payer, cost) = match status {
        StepStatus::AwaitingUnlessPaidChoice {
            payer,
            cost,
            otherwise,
        } => {
            assert!(matches!(otherwise, UnlessPaidConsequence::Counter));
            (payer, cost)
        }
        other => panic!("expected AwaitingUnlessPaidChoice, got {other:?}"),
    };
    assert_eq!(
        payer, p1,
        "the targeted spell's own controller is asked, not Mana Leak's caster"
    );
    assert_eq!(cost.mana.as_ref().expect("mana-only cost").mana_value(), 3);

    engine
        .decline_unless_paid(p1)
        .expect("declining is always legal");
    let status = drain(&mut engine);

    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert!(
        engine.state().stack.is_empty(),
        "the countered spell never gets its own resolution turn"
    );
    assert!(
        engine.state().object(target).is_none(),
        "the original creature-spell id is retired"
    );
    assert_eq!(
        engine
            .state()
            .zone(ZoneKind::Graveyard, Some(p1))
            .unwrap()
            .contents
            .len(),
        1,
        "the countered spell's card landed in its owner's graveyard"
    );
    let countered = engine
        .state()
        .ledger
        .iter()
        .any(|e| matches!(e.kind, GameEventKind::Countered { stack_id } if stack_id == target));
    assert!(
        countered,
        "expected a Countered event for the target's own stack_id"
    );
}

#[test]
fn paying_the_cost_spares_the_target_and_it_resolves_normally() {
    let mut state = two_player_state_with_p1_colorless(3);
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let target = ObjectId(0);
    push_creature_spell_on_stack(&mut state, target, p1);

    let card = ObjectId(2);
    state.objects.insert(
        card,
        card_object(card, mana_leak_definition(), p0, ZoneKind::Hand),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(target)]);
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
    assert!(
        matches!(status, StepStatus::AwaitingUnlessPaidChoice { payer, .. } if payer == p1),
        "got {status:?}"
    );

    let units = vec![
        ManaUnit::Colorless,
        ManaUnit::Colorless,
        ManaUnit::Colorless,
    ];
    engine
        .submit_unless_paid_payment(
            p1,
            PaymentPlan {
                payments: vec![CostPayment::Mana {
                    units,
                    phyrexian_life: 0,
                }],
            },
        )
        .expect("3 generic covers {3} exactly");
    let status = drain(&mut engine);

    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert_eq!(
        engine.state().player(p1).unwrap().mana_pool.colorless,
        0,
        "the {{3}} was actually deducted"
    );
    assert!(
        engine.state().object(target).is_some(),
        "the target spell was never countered"
    );
    assert_eq!(
        engine.state().stack.len(),
        1,
        "only the (spared) target is left on the stack"
    );
    let countered = engine
        .state()
        .ledger
        .iter()
        .any(|e| matches!(e.kind, GameEventKind::Countered { .. }));
    assert!(!countered, "no Countered event should have been logged");

    // The spared target now gets its own normal resolution turn.
    let status = pass_both_and_drain(&mut engine, p0, p1);
    assert!(
        matches!(status, StepStatus::AwaitingAction { .. }),
        "got {status:?}"
    );
    assert!(engine.state().stack.is_empty());
    assert_eq!(
        engine
            .state()
            .zone(ZoneKind::Battlefield, None)
            .unwrap()
            .contents
            .len(),
        1,
        "the spared creature spell resolved onto the battlefield normally"
    );
}

#[test]
fn an_insufficient_payment_plan_is_rejected_and_the_pause_stays_open() {
    let mut state = two_player_state_with_p1_colorless(2);
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let target = ObjectId(0);
    push_creature_spell_on_stack(&mut state, target, p1);

    let card = ObjectId(2);
    state.objects.insert(
        card,
        card_object(card, mana_leak_definition(), p0, ZoneKind::Hand),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(target)]);
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
    drain(&mut engine);

    let units = vec![ManaUnit::Colorless, ManaUnit::Colorless];
    let err = engine
        .submit_unless_paid_payment(
            p1,
            PaymentPlan {
                payments: vec![CostPayment::Mana {
                    units,
                    phyrexian_life: 0,
                }],
            },
        )
        .unwrap_err();
    assert!(
        matches!(err, UnlessPaidError::InvalidPlan),
        "2 generic doesn't cover {{3}}"
    );
    assert_eq!(
        engine.state().player(p1).unwrap().mana_pool.colorless,
        2,
        "no partial deduction on a rejected plan"
    );
    assert!(
        engine.state().object(target).is_some(),
        "the target is still on the stack - nothing was decided yet"
    );
}

#[test]
fn game_ops_counter_unless_paid_decodes_to_the_unless_paid_counter_shape() {
    let runtime = ScriptRuntime::new();
    let source = r#"
        local s = {}
        function s.operation()
            return Game.Ops.CounterUnlessPaid("{3}")
        end
        return s
    "#;
    let (effect, _handle) = runtime
        .build_operation(source)
        .expect("script should load")
        .expect("declares an operation");
    match effect {
        Some(AbilityEffect::UnlessPaid {
            cost,
            otherwise: UnlessPaidConsequence::Counter,
        }) => {
            assert_eq!(cost.mana.expect("mana-only cost").mana_value(), 3);
        }
        other => panic!("expected UnlessPaid{{ otherwise: Counter }}, got {other:?}"),
    }
}

// CR 702.30a
// TODO: `SacrificeSelf` has no Lua builder, so the triggered ability is pushed onto the stack directly and not collected through an Echo keyword
#[test]
fn declining_the_echo_cost_sacrifices_the_source_permanent() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let bear = ObjectId(0);
    put_on_battlefield(
        &mut state,
        card_object(
            bear,
            creature_definition(1, "Bear"),
            p1,
            ZoneKind::Battlefield,
        ),
    );
    let echo_trigger = ObjectId(1);
    push_echo_trigger_on_stack(&mut state, echo_trigger, bear, p1, "{2}");

    let mut engine = Engine::new(state);
    let status = pass_both_and_drain(&mut engine, p0, p1);
    let (payer, cost) = match status {
        StepStatus::AwaitingUnlessPaidChoice {
            payer,
            cost,
            otherwise,
        } => {
            assert!(matches!(otherwise, UnlessPaidConsequence::SacrificeSelf));
            (payer, cost)
        }
        other => panic!("expected AwaitingUnlessPaidChoice, got {other:?}"),
    };
    assert_eq!(
        payer, p1,
        "Echo asks the controller, not necessarily whoever else is playing"
    );
    assert_eq!(cost.mana.expect("mana-only cost").mana_value(), 2);

    engine
        .decline_unless_paid(p1)
        .expect("declining is always legal");
    let status = drain(&mut engine);

    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert!(
        engine.state().object(bear).is_none(),
        "the bear's original id retired - it left the battlefield"
    );
    assert_eq!(
        engine
            .state()
            .zone(ZoneKind::Graveyard, Some(p1))
            .unwrap()
            .contents
            .len(),
        1,
        "sacrificed to its owner's graveyard"
    );
    let resolved =
        engine.state().ledger.iter().any(|e| matches!(e.kind, GameEventKind::StackObjectResolved { stack_id } if stack_id == echo_trigger));
    assert!(
        resolved,
        "the Echo trigger's own resolution must still finish"
    );
}

#[test]
fn paying_the_echo_cost_spares_the_permanent() {
    let mut state = two_player_state_with_p1_colorless(2);
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let bear = ObjectId(0);
    put_on_battlefield(
        &mut state,
        card_object(
            bear,
            creature_definition(1, "Bear"),
            p1,
            ZoneKind::Battlefield,
        ),
    );
    let echo_trigger = ObjectId(1);
    push_echo_trigger_on_stack(&mut state, echo_trigger, bear, p1, "{2}");

    let mut engine = Engine::new(state);
    let status = pass_both_and_drain(&mut engine, p0, p1);
    assert!(
        matches!(status, StepStatus::AwaitingUnlessPaidChoice { payer, .. } if payer == p1),
        "got {status:?}"
    );

    let units = vec![ManaUnit::Colorless, ManaUnit::Colorless];
    engine
        .submit_unless_paid_payment(
            p1,
            PaymentPlan {
                payments: vec![CostPayment::Mana {
                    units,
                    phyrexian_life: 0,
                }],
            },
        )
        .expect("2 generic covers {2} exactly");
    let status = drain(&mut engine);

    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert_eq!(
        engine.state().player(p1).unwrap().mana_pool.colorless,
        0,
        "the {{2}} was actually deducted"
    );
    assert!(
        engine.state().object(bear).is_some(),
        "the bear was never sacrificed"
    );
    assert_eq!(
        engine.state().object(bear).unwrap().zone,
        ZoneKind::Battlefield
    );
}

#[test]
fn declining_the_echo_cost_through_a_simultaneous_replacement_ordering_choice_still_resolves_correctly()
 {
    // Two independent "exile instead" effects both match the bear's ZoneChange, forcing a CR 616.1 ordering pause mid-decline. decline_unless_paid's SacrificeSelf branch must thread events_so_far through the nested pause and still finish with StackObjectResolved for the outer Echo ability.
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let bear = ObjectId(0);
    let shield_obj = ObjectId(1);
    put_on_battlefield(
        &mut state,
        card_object(
            bear,
            creature_definition(1, "Bear"),
            p1,
            ZoneKind::Battlefield,
        ),
    );
    put_on_battlefield(
        &mut state,
        card_object(
            shield_obj,
            shield_definition(2, "Test Shield"),
            p1,
            ZoneKind::Battlefield,
        ),
    );
    state
        .replacement_effects
        .push(exile_instead_effect(bear, true, ConsumptionPolicy::OneShot));
    state.replacement_effects.push(exile_instead_effect(
        shield_obj,
        false,
        ConsumptionPolicy::OneShot,
    ));
    let echo_trigger = ObjectId(2);
    push_echo_trigger_on_stack(&mut state, echo_trigger, bear, p1, "{2}");

    let mut engine = Engine::new(state);
    let status = pass_both_and_drain(&mut engine, p0, p1);
    assert!(
        matches!(status, StepStatus::AwaitingUnlessPaidChoice { .. }),
        "got {status:?}"
    );

    engine
        .decline_unless_paid(p1)
        .expect("declining is always legal");
    let status = drain(&mut engine);
    let StepStatus::AwaitingReplacementChoice { candidates, .. } = status else {
        panic!(
            "2 matching replacement effects on the sacrificed bear must pause for a CR 616.1 choice, got {status:?}"
        );
    };
    assert_eq!(candidates.len(), 2);

    engine
        .submit_replacement_choice(ReplacementCandidate::Registered(bear))
        .expect("bear (self_only) is a real candidate");
    let status = drain(&mut engine);

    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert!(
        engine.state().object(bear).is_none(),
        "the bear's original id retired"
    );
    assert!(
        !engine
            .state()
            .zone(ZoneKind::Exile, None)
            .unwrap()
            .contents
            .is_empty(),
        "the chosen exile-instead effect won - the bear should be in Exile, not the graveyard"
    );
    let resolved =
        engine.state().ledger.iter().any(|e| matches!(e.kind, GameEventKind::StackObjectResolved { stack_id } if stack_id == echo_trigger));
    assert!(
        resolved,
        "the outer Echo ability's own resolution must still finish after the nested CR 616.1 pause clears"
    );
}
