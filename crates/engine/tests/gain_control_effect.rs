//! CR 108.4, 613.1: `AbilityEffect::GainControl` (`Game.Ops.GainControl(duration)`) is a layer 2 `ContinuousEffect` built per target, like `Pump`, for "gain control of target creature until <duration>".
//! `CardDefinition.effect` is set directly to take the Rust fast path in `ResolveStackTop`, as in pump_effect.rs.
// TODO: a static "you control enchanted creature" ability (Control Magic) needs attach and enchant support for control effects

use std::collections::HashMap;

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::layers::effective_controller;
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{
    card_object, drain, hand_battlefield_graveyard_zones, two_player_state as base_two_player_state,
};

fn creature_definition(
    definition_id: u64,
    name: &str,
    power: i32,
    toughness: i32,
) -> CardDefinition {
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
        power: Some(PTValue::Fixed(power)),
        toughness: Some(PTValue::Fixed(toughness)),
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey(name.to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn threaten_definition(duration: EffectDuration) -> CardDefinition {
    CardDefinition {
        definition_id: 20,
        name: "Test Threaten".to_string(),
        mana_cost: ManaCost {
            symbols: Vec::new(),
        },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::SORCERY,
        subtypes: Vec::new(),
        oracle_text: "Test Threaten: Gain control of target creature.".to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_threaten".to_string()),
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
        effect: Some(AbilityEffect::GainControl(duration)),
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
        ..base_two_player_state(hand_battlefield_graveyard_zones(p0, p1))
    }
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

// Passes priority until the active player rotates, that is, until Cleanup has run.
fn advance_to_next_turn(engine: &mut Engine) {
    let starting_turn = engine.state().turn_number;
    loop {
        match drain(engine) {
            StepStatus::AwaitingAction { player } => {
                engine
                    .submit_action(PlayerAction {
                        player,
                        kind: PlayerActionKind::PassPriority,
                    })
                    .unwrap();
            }
            other => panic!("unexpected pause while advancing turns: {other:?}"),
        }
        if engine.state().turn_number != starting_turn {
            break;
        }
    }
}

fn pass_to_declare_attackers(engine: &mut Engine) {
    for _ in 0..10 {
        if engine.state().step == Step::DeclareAttackers {
            return;
        }
        let active = engine.state().priority_player;
        engine
            .submit_action(PlayerAction {
                player: active,
                kind: PlayerActionKind::PassPriority,
            })
            .expect("priority window");
        drain(engine);
    }
    panic!(
        "never reached DeclareAttackers - state.step = {:?}",
        engine.state().step
    );
}

fn declare_attackers(
    engine: &mut Engine,
    player: PlayerId,
    attackers: Vec<(ObjectId, Target)>,
) -> StepStatus {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::DeclareAttackers { attackers },
        })
        .expect("player holds priority");
    drain(engine)
}

#[test]
fn threaten_spell_makes_the_caster_the_real_effective_controller() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let target = ObjectId(0);
    state.objects.insert(
        target,
        card_object(
            target,
            creature_definition(1, "Bear", 2, 2),
            p1,
            ZoneKind::Battlefield,
        ),
    );
    state
        .zone_mut(ZoneKind::Battlefield, None)
        .unwrap()
        .push_bottom(target);
    let card = ObjectId(2);
    state.objects.insert(
        card,
        card_object(
            card,
            threaten_definition(EffectDuration::UntilEndOfTurn),
            p0,
            ZoneKind::Hand,
        ),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(target)]);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    assert_eq!(
        effective_controller(engine.state(), &ScriptRuntime::new(), target),
        p0,
        "p0 (the caster) is now the real controller"
    );
    assert_eq!(engine.state().continuous_effects.len(), 1);
    assert!(matches!(
        engine.state().continuous_effects[0].duration,
        EffectDuration::UntilEndOfTurn
    ));

    // p0 can now declare p1's original creature as an attacker, which is real combat legality.
    pass_to_declare_attackers(&mut engine);
    let status = declare_attackers(&mut engine, p0, vec![(target, Target::Player(p1))]);
    assert!(
        matches!(status, StepStatus::AwaitingAction { .. }),
        "the new controller should legally declare it as an attacker, got {status:?}"
    );
}

#[test]
fn gain_control_effect_is_pinned_to_its_target_not_the_spells_own_source() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let target = ObjectId(0);
    let bystander = ObjectId(1);
    state.objects.insert(
        target,
        card_object(
            target,
            creature_definition(1, "Bear", 2, 2),
            p1,
            ZoneKind::Battlefield,
        ),
    );
    state
        .zone_mut(ZoneKind::Battlefield, None)
        .unwrap()
        .push_bottom(target);
    state.objects.insert(
        bystander,
        card_object(
            bystander,
            creature_definition(2, "Bystander", 1, 1),
            p1,
            ZoneKind::Battlefield,
        ),
    );
    state
        .zone_mut(ZoneKind::Battlefield, None)
        .unwrap()
        .push_bottom(bystander);
    let card = ObjectId(2);
    state.objects.insert(
        card,
        card_object(
            card,
            threaten_definition(EffectDuration::UntilEndOfTurn),
            p0,
            ZoneKind::Hand,
        ),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(target)]);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    assert_eq!(
        effective_controller(engine.state(), &ScriptRuntime::new(), bystander),
        p1,
        "the untargeted creature's controller is untouched"
    );
}

#[test]
fn until_end_of_turn_gain_control_reverts_by_the_start_of_the_next_turn() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let target = ObjectId(0);
    state.objects.insert(
        target,
        card_object(
            target,
            creature_definition(1, "Bear", 2, 2),
            p1,
            ZoneKind::Battlefield,
        ),
    );
    state
        .zone_mut(ZoneKind::Battlefield, None)
        .unwrap()
        .push_bottom(target);
    let card = ObjectId(2);
    state.objects.insert(
        card,
        card_object(
            card,
            threaten_definition(EffectDuration::UntilEndOfTurn),
            p0,
            ZoneKind::Hand,
        ),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(target)]);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);
    assert_eq!(
        effective_controller(engine.state(), &ScriptRuntime::new(), target),
        p0,
        "sanity: control was gained"
    );

    advance_to_next_turn(&mut engine);

    assert_eq!(
        engine.state().continuous_effects.len(),
        0,
        "UntilEndOfTurn drains at cleanup"
    );
    assert_eq!(
        effective_controller(engine.state(), &ScriptRuntime::new(), target),
        p1,
        "control reverts to the original owner once the effect is gone"
    );
}

#[test]
fn permanent_gain_control_survives_into_the_next_turn() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let target = ObjectId(0);
    state.objects.insert(
        target,
        card_object(
            target,
            creature_definition(1, "Bear", 2, 2),
            p1,
            ZoneKind::Battlefield,
        ),
    );
    state
        .zone_mut(ZoneKind::Battlefield, None)
        .unwrap()
        .push_bottom(target);
    let card = ObjectId(2);
    state.objects.insert(
        card,
        card_object(
            card,
            threaten_definition(EffectDuration::Permanent),
            p0,
            ZoneKind::Hand,
        ),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(target)]);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    advance_to_next_turn(&mut engine);

    assert_eq!(
        engine.state().continuous_effects.len(),
        1,
        "Permanent duration is unaffected by cleanup"
    );
    assert_eq!(
        effective_controller(engine.state(), &ScriptRuntime::new(), target),
        p0,
        "control is still held after the turn passes"
    );
}
