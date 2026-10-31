//! A Lua runtime error mid-resolution logs `GameEventKind::ScriptError` wherever the call site has `&mut Engine`, keeping the conservative fallback (condition false, intervening-if does not hold).
//! `layers.rs` script call sites are `&GameState`-only characteristic queries, so they do not log, since threading error reporting through `effective_characteristics` is a much larger change.
// TODO: script errors raised from `layers.rs` are not logged

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::triggers::collect_triggers;
use stackfish_engine::*;
use stackfish_testutil::{
    card_object, drain, hand_battlefield_graveyard_zones, player_state,
    two_player_state as base_two_player_state,
};

const ALWAYS_ERRORS: &str = "function(...) error('boom') end";

// Errors only once controller's life is 10 or below, so the trigger-time check queues and the error is forced at the resolution-time recheck.
const ERRORS_BELOW_TEN_LIFE: &str = r#"
function(source, controller, state)
    if state.life[controller] <= 10 then
        error("boom")
    end
    return true
end
"#;

fn creature_definition(name: &str) -> CardDefinition {
    CardDefinition {
        definition_id: 1,
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

fn drain_spell_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 2,
        name: "Test Drain".to_string(),
        mana_cost: ManaCost {
            symbols: Vec::new(),
        },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::INSTANT,
        subtypes: Vec::new(),
        oracle_text: "You lose 15 life.".to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_drain".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: Some(AbilityEffect::GainLife(-15)),
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn two_player_state(p0_life: i32) -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        phase: Phase::PreGame,
        players: vec![
            PlayerState {
                life: p0_life,
                ..player_state(p0)
            },
            player_state(p1),
        ],
        ..base_two_player_state(hand_battlefield_graveyard_zones(p0, p1))
    }
}

fn pass_both(engine: &mut Engine, p0: PlayerId, p1: PlayerId) -> StepStatus {
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

fn cast_spell(engine: &mut Engine, player: PlayerId, card: ObjectId) {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::CastSpell {
                card,
                modes: Vec::new(),
                targets: Vec::new(),
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

fn script_error_events(engine: &Engine) -> Vec<String> {
    engine
        .state()
        .ledger
        .iter()
        .filter_map(|e| match &e.kind {
            GameEventKind::ScriptError { message } => Some(message.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn collect_triggers_condition_script_error_falls_back_to_no_match_and_is_reported() {
    let mut runtime = ScriptRuntime::new();
    let handle = runtime.load_function(ALWAYS_ERRORS).expect("script loads");
    let source = ObjectId(0);
    let mut state = two_player_state(20);
    state.objects.insert(
        source,
        card_object(
            source,
            creature_definition("Bear"),
            PlayerId(0),
            ZoneKind::Battlefield,
        ),
    );
    state
        .zone_mut(ZoneKind::Battlefield, None)
        .unwrap()
        .push_bottom(source);
    state.abilities.push(RegisteredAbility {
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
        id: AbilityId(0),
        source,
        kind: AbilityKind::Triggered,
        condition: Some(ConditionSpec::Script(handle)),
        cost: None,
        target: None,
        operation: ScriptHandle(0),
        effect: Some(AbilityEffect::GainLife(1)),
        intervening_if: None,
    });
    let event = GameEvent {
        event_id: EventId(0),
        sequence_no: SequenceNo(0),
        kind: GameEventKind::ZoneChange {
            object: source,
            from: ZoneKind::Hand,
            to: ZoneKind::Battlefield,
            last_known_info: None,
        },
        caused_by_command: None,
        parent_event: None,
    };

    let (pending, errors) = collect_triggers(&state, &event, &runtime, &[]);

    assert!(
        pending.is_empty(),
        "a script error must fall back to 'didn't match', not guess true"
    );
    assert_eq!(
        errors.len(),
        1,
        "the error is reported, not silently dropped"
    );
    assert!(
        errors[0].contains("condition"),
        "error message: {}",
        errors[0]
    );
}

#[test]
fn collect_triggers_intervening_if_script_error_falls_back_to_not_holding() {
    let mut runtime = ScriptRuntime::new();
    let handle = runtime.load_function(ALWAYS_ERRORS).expect("script loads");
    let source = ObjectId(0);
    let mut state = two_player_state(20);
    state.objects.insert(
        source,
        card_object(
            source,
            creature_definition("Bear"),
            PlayerId(0),
            ZoneKind::Battlefield,
        ),
    );
    state
        .zone_mut(ZoneKind::Battlefield, None)
        .unwrap()
        .push_bottom(source);
    state.abilities.push(RegisteredAbility {
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
        id: AbilityId(1),
        source,
        kind: AbilityKind::Triggered,
        condition: None,
        cost: None,
        target: None,
        operation: ScriptHandle(0),
        effect: Some(AbilityEffect::GainLife(1)),
        intervening_if: Some(InterveningIfSpec::Script(handle)),
    });
    let event = GameEvent {
        event_id: EventId(0),
        sequence_no: SequenceNo(0),
        kind: GameEventKind::ZoneChange {
            object: source,
            from: ZoneKind::Hand,
            to: ZoneKind::Battlefield,
            last_known_info: None,
        },
        caused_by_command: None,
        parent_event: None,
    };

    let (pending, errors) = collect_triggers(&state, &event, &runtime, &[]);

    assert!(
        pending.is_empty(),
        "condition matched (None), but the erroring intervening-if must still suppress it"
    );
    assert_eq!(errors.len(), 1);
    assert!(
        errors[0].contains("intervening-if"),
        "error message: {}",
        errors[0]
    );
}

#[test]
fn collect_triggers_departed_loop_intervening_if_script_error_falls_back() {
    let mut runtime = ScriptRuntime::new();
    let handle = runtime.load_function(ALWAYS_ERRORS).expect("script loads");
    let source = ObjectId(0);
    let state = two_player_state(20);
    let departed = vec![RegisteredAbility {
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
        id: AbilityId(2),
        source,
        kind: AbilityKind::Triggered,
        condition: Some(ConditionSpec::Primitive(ConditionPrimitive::Dies)),
        cost: None,
        target: None,
        operation: ScriptHandle(0),
        effect: Some(AbilityEffect::GainLife(1)),
        intervening_if: Some(InterveningIfSpec::Script(handle)),
    }];
    let mut lki = card_object(
        source,
        creature_definition("Bear"),
        PlayerId(0),
        ZoneKind::Battlefield,
    );
    lki.object_id = source;
    let event = GameEvent {
        event_id: EventId(0),
        sequence_no: SequenceNo(0),
        kind: GameEventKind::ZoneChange {
            object: ObjectId(999),
            from: ZoneKind::Battlefield,
            to: ZoneKind::Graveyard,
            last_known_info: Some(lki),
        },
        caused_by_command: None,
        parent_event: None,
    };

    let (pending, errors) = collect_triggers(&state, &event, &runtime, &departed);

    assert!(
        pending.is_empty(),
        "the Dies match must not fire past an erroring intervening-if"
    );
    assert_eq!(errors.len(), 1);
    assert!(
        errors[0].contains("departed"),
        "error message: {}",
        errors[0]
    );
}

#[test]
fn resolution_time_intervening_if_script_error_fizzles_the_trigger_and_is_logged() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let permanent = ObjectId(0);
    // Life 20 is above the error threshold (10), so the trigger-time check succeeds and the trigger queues.
    let mut state = two_player_state(20);
    state.objects.insert(
        permanent,
        card_object(
            permanent,
            creature_definition("Bear"),
            p0,
            ZoneKind::Battlefield,
        ),
    );
    state
        .zone_mut(ZoneKind::Battlefield, None)
        .unwrap()
        .push_bottom(permanent);
    let drain_spell = ObjectId(1);
    state.objects.insert(
        drain_spell,
        card_object(drain_spell, drain_spell_definition(), p0, ZoneKind::Hand),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(drain_spell);

    // ScriptHandle values are keys local to one ScriptRuntime, so mint them from the engine's own runtime (Engine::load_script).
    let mut engine = Engine::new(state);
    let handle = engine
        .load_script(ERRORS_BELOW_TEN_LIFE)
        .expect("script loads");
    engine.state_mut().abilities.push(RegisteredAbility {
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
        id: AbilityId(3),
        source: permanent,
        kind: AbilityKind::Triggered,
        condition: Some(ConditionSpec::Primitive(
            ConditionPrimitive::BeginningOfYourUpkeep,
        )),
        cost: None,
        target: None,
        operation: ScriptHandle(0),
        effect: Some(AbilityEffect::GainLife(1)),
        intervening_if: Some(InterveningIfSpec::Script(handle)),
    });

    let status = pass_both(&mut engine, p0, p1);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert_eq!(
        engine.state().stack.len(),
        1,
        "intervening-if held (no error) at trigger time - collected and pushed"
    );
    assert!(
        script_error_events(&engine).is_empty(),
        "no error yet - the trigger-time check never ran the erroring branch"
    );

    // p0 responds with Test Drain, dropping life to 5, below the error threshold, before the trigger resolves.
    cast_spell(&mut engine, p0, drain_spell);
    pay_free_and_drain(&mut engine);
    assert_eq!(engine.state().stack.len(), 2);

    let status = pass_both(&mut engine, p0, p1);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert_eq!(
        engine.state().player(p0).unwrap().life,
        5,
        "Test Drain resolved first"
    );
    assert_eq!(
        engine.state().stack.len(),
        1,
        "only the upkeep trigger remains"
    );

    // CR 608.2a's recheck hits the erroring branch (life 5 <= 10), falls back to "doesn't hold", fizzles, and logs a ScriptError.
    let status = pass_both(&mut engine, p0, p1);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert!(engine.state().stack.is_empty());
    assert_eq!(
        engine.state().player(p0).unwrap().life,
        5,
        "GainLife(1) did NOT apply - the erroring recheck fizzled it"
    );
    assert!(
        engine
            .state()
            .ledger
            .iter()
            .any(|e| matches!(e.kind, GameEventKind::Fizzled { .. })),
        "a Fizzled event was logged for the errored-out trigger"
    );
    // collect_triggers evaluates every ability's intervening-if on every event, so at life 5 more than one ScriptError is expected. The point is that at least one was logged.
    let errors = script_error_events(&engine);
    assert!(
        !errors.is_empty(),
        "at least one ScriptError event was logged"
    );
    assert!(
        errors.iter().all(|m| m.contains("intervening-if")),
        "error messages: {errors:?}"
    );
}
