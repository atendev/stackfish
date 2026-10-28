//! CR 603: a real Lua condition and operation, invoked through `ScriptRuntime`, power an ETB "gain 1 life" trigger. The operation returns a plain table (`{ gain_life = 1 }`) that `invoke_operation` decodes, so Lua asks and Rust decides.
//! Registration is Rust-side, and only `GameEventKind::ZoneChange` is bridged into Lua (`script::event_to_lua`).

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::triggers::collect_triggers;
use stackfish_engine::*;
use stackfish_testutil::{
    card_object, drain, hand_battlefield_graveyard_zones, minimal_card_definition, two_player_state as base_two_player_state,
};

const CONDITION_ENTERS_BATTLEFIELD: &str = r#"
return function(source, controller, event)
    return event ~= nil
       and event.kind == "zone_change"
       and event.object == source
       and event.to == "battlefield"
end
"#;

const OPERATION_GAIN_LIFE_1: &str = r#"
return function(source, controller)
    return { gain_life = 1 }
end
"#;

fn bear_definition() -> CardDefinition {
    CardDefinition {
        power: Some(PTValue::Fixed(2)),
        toughness: Some(PTValue::Fixed(2)),
        oracle_text: "When this creature enters the battlefield, you gain 1 life.".to_string(),
        script_ref: ScriptReference::Inline(String::new()),
        ..minimal_card_definition(1, "Test Bear", "test_bear", ManaCost { symbols: Vec::new() }, CardTypeSet::CREATURE)
    }
}

fn two_player_state() -> GameState {
    base_two_player_state(hand_battlefield_graveyard_zones(PlayerId(0), PlayerId(1)))
}

fn zone_change_event(object: ObjectId, from: ZoneKind, to: ZoneKind) -> GameEvent {
    GameEvent {
        event_id: EventId(0),
        sequence_no: SequenceNo(0),
        kind: GameEventKind::ZoneChange { object, from, to, last_known_info: None },
        caused_by_command: None,
        parent_event: None,
    }
}

#[test]
fn collect_triggers_matches_a_real_condition_script() {
    let p0 = PlayerId(0);
    let bear = ObjectId(0);
    let mut runtime = ScriptRuntime::new();
    let condition = runtime.load_function(CONDITION_ENTERS_BATTLEFIELD).expect("condition script loads");

    let mut state = two_player_state();
    state.objects.insert(bear, card_object(bear, bear_definition(), p0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(bear);
    state.abilities.push(RegisteredAbility {
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
        id: AbilityId(0),
        source: bear,
        kind: AbilityKind::Triggered,
        condition: Some(ConditionSpec::Script(condition)),
        cost: None,
        target: None,
        operation: ScriptHandle(0), // never invoked by collect_triggers itself
        effect: None,
        intervening_if: None,
    });

    let event = zone_change_event(bear, ZoneKind::Hand, ZoneKind::Battlefield);
    let (pending, _) = collect_triggers(&state, &event, &runtime, &[]);
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].source, bear);
    assert_eq!(pending[0].controller, p0);
    assert_eq!(pending[0].ability, AbilityId(0));
}

#[test]
fn collect_triggers_condition_false_on_a_different_object() {
    let p0 = PlayerId(0);
    let bear = ObjectId(0);
    let someone_else = ObjectId(1);
    let mut runtime = ScriptRuntime::new();
    let condition = runtime.load_function(CONDITION_ENTERS_BATTLEFIELD).expect("condition script loads");

    let mut state = two_player_state();
    state.objects.insert(bear, card_object(bear, bear_definition(), p0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(bear);
    state.abilities.push(RegisteredAbility {
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
        id: AbilityId(1),
        source: bear,
        kind: AbilityKind::Triggered,
        condition: Some(ConditionSpec::Script(condition)),
        cost: None,
        target: None,
        operation: ScriptHandle(0),
        effect: None,
        intervening_if: None,
    });

    // A different object entering the battlefield does not match, since the condition checks event.object == source.
    let event = zone_change_event(someone_else, ZoneKind::Hand, ZoneKind::Battlefield);
    let (pending, _) = collect_triggers(&state, &event, &runtime, &[]);
    assert!(pending.is_empty(), "the bear's ETB condition shouldn't fire for a different object");
}

#[test]
fn collect_triggers_condition_false_on_wrong_destination_zone() {
    let p0 = PlayerId(0);
    let bear = ObjectId(0);
    let mut runtime = ScriptRuntime::new();
    let condition = runtime.load_function(CONDITION_ENTERS_BATTLEFIELD).expect("condition script loads");

    let mut state = two_player_state();
    state.objects.insert(bear, card_object(bear, bear_definition(), p0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(bear);
    state.abilities.push(RegisteredAbility {
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
        id: AbilityId(2),
        source: bear,
        kind: AbilityKind::Triggered,
        condition: Some(ConditionSpec::Script(condition)),
        cost: None,
        target: None,
        operation: ScriptHandle(0),
        effect: None,
        intervening_if: None,
    });

    // The bear moving to the graveyard does not match an ETB condition.
    let event = zone_change_event(bear, ZoneKind::Battlefield, ZoneKind::Graveyard);
    let (pending, _) = collect_triggers(&state, &event, &runtime, &[]);
    assert!(pending.is_empty());
}

#[test]
fn collect_triggers_none_condition_matches_any_event() {
    let p0 = PlayerId(0);
    let bear = ObjectId(0);
    let runtime = ScriptRuntime::new();

    let mut state = two_player_state();
    state.objects.insert(bear, card_object(bear, bear_definition(), p0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(bear);
    state.abilities.push(RegisteredAbility {
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
        id: AbilityId(3),
        source: bear,
        kind: AbilityKind::Triggered,
        // No condition means no filter, so it always triggers.
        condition: None,
        cost: None,
        target: None,
        operation: ScriptHandle(0),
        effect: None,
        intervening_if: None,
    });

    // An unrelated event kind matches anyway, since there is no condition to filter it.
    let event = GameEvent {
        event_id: EventId(0),
        sequence_no: SequenceNo(0),
        kind: GameEventKind::PriorityPassed { player: p0 },
        caused_by_command: None,
        parent_event: None,
    };
    let (pending, _) = collect_triggers(&state, &event, &runtime, &[]);
    assert_eq!(pending.len(), 1, "no condition means no filter - any event matches");
}

#[test]
fn engine_load_script_rejects_malformed_lua() {
    let mut engine = Engine::new(two_player_state());
    let result = engine.load_script("this is not valid lua {{{");
    assert!(result.is_err(), "a syntax error should fail to load, not panic");
}

fn pay_free_and_drain(engine: &mut Engine) -> StepStatus {
    let status = drain(engine);
    assert!(
        matches!(status, StepStatus::AwaitingPayment { .. }),
        "expected a pause for the cast's (free) cost, got {status:?}"
    );
    engine.submit_payment(PaymentPlan { payments: Vec::new() }).expect("empty plan pays a {0} cost");
    drain(engine)
}

fn pass(engine: &mut Engine, player: PlayerId) {
    engine.submit_action(PlayerAction { player, kind: PlayerActionKind::PassPriority }).unwrap();
}

#[test]
fn cast_creature_with_etb_triggers_gain_life_end_to_end() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let bear = ObjectId(0);
    let mut state = two_player_state();
    state.objects.insert(bear, card_object(bear, bear_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(bear);

    let mut engine = Engine::new(state);
    let condition = engine.load_script(CONDITION_ENTERS_BATTLEFIELD).expect("condition script loads");
    let operation = engine.load_script(OPERATION_GAIN_LIFE_1).expect("operation script loads");
    engine.state_mut().abilities.push(RegisteredAbility {
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
        id: AbilityId(4),
        source: bear,
        kind: AbilityKind::Triggered,
        condition: Some(ConditionSpec::Script(condition)),
        cost: None,
        target: None,
        operation,
        effect: None,
        intervening_if: None,
    });

    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::CastSpell { card: bear, modes: Vec::new(), targets: Vec::new(), x_value: None, alternative_cost: None , pay_buyback: false},
        })
        .expect("p0 holds priority, bear is in hand, main phase, empty stack");
    pay_free_and_drain(&mut engine);

    // The spell is on the stack, and the hand to stack ZoneChange does not match the ETB condition, so no ability is pending.
    assert_eq!(engine.state().stack.len(), 1, "only the spell itself is on the stack so far");
    assert_eq!(engine.state().player(p0).unwrap().life, 20, "no life gain before the creature resolves");

    // Both players pass, CR 117.4 resolves the spell onto the battlefield, and the ETB match pushes the ability in the same run.
    pass(&mut engine, p0);
    drain(&mut engine);
    pass(&mut engine, p1);
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));

    assert_eq!(engine.state().stack.len(), 1, "the ETB ability itself is now on the stack");
    assert_eq!(engine.state().player(p0).unwrap().life, 20, "ability hasn't resolved yet");

    // Both players pass again, which resolves the ability and invokes its Lua callback.
    pass(&mut engine, p0);
    drain(&mut engine);
    pass(&mut engine, p1);
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));

    assert!(engine.state().stack.is_empty(), "both the spell and its triggered ability have resolved");
    assert_eq!(engine.state().player(p0).unwrap().life, 21, "the operation script's gain_life = 1 applied");

    let resolved_count = engine
        .state()
        .ledger
        .iter()
        .filter(|e| matches!(e.kind, GameEventKind::StackObjectResolved { .. }))
        .count();
    assert_eq!(resolved_count, 2, "the spell and the triggered ability each resolved once");
}

#[test]
fn cast_a_different_creature_does_not_trigger_the_bears_etb() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let bear = ObjectId(0);
    let other_creature = ObjectId(1);
    let mut state = two_player_state();
    state.objects.insert(bear, card_object(bear, bear_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(bear);
    state.objects.insert(other_creature, card_object(other_creature, bear_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(other_creature);

    let mut engine = Engine::new(state);
    let condition = engine.load_script(CONDITION_ENTERS_BATTLEFIELD).expect("condition script loads");
    let operation = engine.load_script(OPERATION_GAIN_LIFE_1).expect("operation script loads");
    // The ability is registered to bear, so another creature with the same definition does not fire it.
    engine.state_mut().abilities.push(RegisteredAbility {
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
        id: AbilityId(5),
        source: bear,
        kind: AbilityKind::Triggered,
        condition: Some(ConditionSpec::Script(condition)),
        cost: None,
        target: None,
        operation,
        effect: None,
        intervening_if: None,
    });

    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::CastSpell {
                card: other_creature,
                modes: Vec::new(),
                targets: Vec::new(),
                x_value: None,
                alternative_cost: None, pay_buyback: false,
            },
        })
        .expect("p0 holds priority");
    pay_free_and_drain(&mut engine);
    pass(&mut engine, p0);
    drain(&mut engine);
    pass(&mut engine, p1);
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { .. }));

    assert!(engine.state().stack.is_empty(), "no ability should have triggered off a different object");
    assert_eq!(engine.state().player(p0).unwrap().life, 20, "no life gain - the bear itself never entered");
}
