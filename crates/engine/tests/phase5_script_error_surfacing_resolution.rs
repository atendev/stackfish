//! A Lua error in `run_card_script`, `invoke_operation`, `resume_operation` and `resume_operation_bool` after the initial cast-time call is logged as `GameEventKind::ScriptError` with the fallback unchanged, as `invoke_condition` and `invoke_intervening_if` do in phase5_script_error_surfacing.rs.
//! `record_script_error` and `resolve_misc_or_script_effect` skip logging for "no real script" shapes (`ByKey`, an empty `Inline`, the `ScriptHandle(0)` placeholder), since logging them broke many `ledger.len()` assertions.
//! One representative test per code shape, because every `run_card_script` call site funnels through the same `record_script_error` call.
// TODO: `cast.rs` unpacks `(operation, effect)` so any non-`Script` effect shape (including `ForEachPlayer`) discards its handle down to the `ScriptHandle(0)` placeholder, which keeps the placeholder ambiguity open

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::*;
use stackfish_testutil::{
    card_object, drain, hand_battlefield_graveyard_zones, minimal_card_definition, two_player_state as base_two_player_state,
};

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

fn two_player_state() -> GameState {
    base_two_player_state(hand_battlefield_graveyard_zones(PlayerId(0), PlayerId(1)))
}

const ALWAYS_ERRORS_CARD_SCRIPT: &str = r#"
local s = {}
function s.register_abilities(c)
    error("boom")
end
return s
"#;

fn erroring_land_definition() -> CardDefinition {
    CardDefinition {
        supertypes: SupertypeSet::BASIC,
        script_ref: ScriptReference::Inline(ALWAYS_ERRORS_CARD_SCRIPT.to_string()),
        ..minimal_card_definition(1, "Test Erroring Land", "unused", ManaCost { symbols: Vec::new() }, CardTypeSet::LAND)
    }
}

fn vanilla_land_definition() -> CardDefinition {
    CardDefinition {
        supertypes: SupertypeSet::BASIC,
        ..minimal_card_definition(2, "Forest", "forest", ManaCost { symbols: Vec::new() }, CardTypeSet::LAND)
    }
}

#[test]
fn play_land_with_a_real_erroring_script_logs_a_script_error() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    state.objects.insert(card, card_object(card, erroring_land_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PlayLand { card } }).expect("p0 holds priority, land is in hand");
    let status = drain(&mut engine);

    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0), "got {status:?}");
    assert!(
        engine.state().zone(ZoneKind::Battlefield, None).unwrap().contents.len() == 1,
        "the land still entered the battlefield - the fallback behavior is unchanged"
    );
    let errors = script_error_events(&engine);
    assert_eq!(errors.len(), 1, "the real script error is now logged, not silently dropped");
    assert!(errors[0].contains("boom"), "error message: {}", errors[0]);
}

#[test]
fn play_land_with_a_bykey_placeholder_script_logs_no_script_error() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    state.objects.insert(card, card_object(card, vanilla_land_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PlayLand { card } }).expect("p0 holds priority, land is in hand");
    let status = drain(&mut engine);

    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0), "got {status:?}");
    assert!(
        script_error_events(&engine).is_empty(),
        "ByKey is the ordinary 'no real script' shape testutil's own minimal_card_definition defaults to - \
         must not be reported as a script failure"
    );
}

fn vanilla_creature_definition() -> CardDefinition {
    CardDefinition {
        power: Some(PTValue::Fixed(2)),
        toughness: Some(PTValue::Fixed(2)),
        ..minimal_card_definition(3, "Grizzly Bears", "grizzly_bears", ManaCost { symbols: Vec::new() }, CardTypeSet::CREATURE)
    }
}

fn cast_spell(engine: &mut Engine, player: PlayerId, card: ObjectId) {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::CastSpell { card, modes: Vec::new(), targets: Vec::new(), x_value: None, alternative_cost: None, pay_buyback: false },
        })
        .expect("player holds priority, spell is in hand");
}

fn pay_free_and_drain(engine: &mut Engine) -> StepStatus {
    let status = drain(engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }), "expected a pause for the cast's (free) cost, got {status:?}");
    engine.submit_payment(PaymentPlan { payments: Vec::new() }).expect("empty plan pays a {0} cost");
    drain(engine)
}

#[test]
fn casting_a_vanilla_creature_with_a_bykey_script_logs_no_script_error() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(0);
    state.objects.insert(card, card_object(card, vanilla_creature_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(&mut engine);
    engine.submit_action(PlayerAction { player: p1, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(&mut engine);

    assert_eq!(engine.state().zone(ZoneKind::Battlefield, None).unwrap().contents.len(), 1, "the creature resolved onto the battlefield");
    assert!(
        script_error_events(&engine).is_empty(),
        "a vanilla ByKey creature is this engine's own overwhelmingly common test-fixture shape - \
         casting one must not flood the ledger with a false 'script failure'"
    );
}

const CONDITION_ENTERS_BATTLEFIELD: &str = r#"
return function(source, controller, event)
    return event ~= nil
       and event.kind == "zone_change"
       and event.object == source
       and event.to == "battlefield"
end
"#;

const OPERATION_ALWAYS_ERRORS: &str = r#"
return function(source, controller)
    error("boom")
end
"#;

fn erroring_operation_creature_definition() -> CardDefinition {
    CardDefinition {
        power: Some(PTValue::Fixed(2)),
        toughness: Some(PTValue::Fixed(2)),
        script_ref: ScriptReference::Inline(String::new()),
        ..minimal_card_definition(4, "Test Erroring ETB Creature", "unused", ManaCost { symbols: Vec::new() }, CardTypeSet::CREATURE)
    }
}

#[test]
fn a_triggered_abilitys_erroring_operation_logs_a_script_error_and_still_resolves() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let caster = ObjectId(0);
    let mut state = two_player_state();
    state.objects.insert(caster, card_object(caster, erroring_operation_creature_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(caster);

    let mut engine = Engine::new(state);
    let condition = engine.load_script(CONDITION_ENTERS_BATTLEFIELD).expect("condition script loads");
    let operation = engine.load_script(OPERATION_ALWAYS_ERRORS).expect("operation script loads");
    engine.state_mut().abilities.push(RegisteredAbility {
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
        id: AbilityId(0),
        source: caster,
        kind: AbilityKind::Triggered,
        condition: Some(ConditionSpec::Script(condition)),
        cost: None,
        target: None,
        operation,
        effect: None,
        intervening_if: None,
    });

    cast_spell(&mut engine, p0, caster);
    pay_free_and_drain(&mut engine);
    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(&mut engine);
    engine.submit_action(PlayerAction { player: p1, kind: PlayerActionKind::PassPriority }).unwrap();
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0), "got {status:?}");
    assert_eq!(engine.state().stack.len(), 1, "the ETB ability itself is now on the stack");

    // The erroring operation resolves with no effect (the fallback) and the error is logged.
    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(&mut engine);
    engine.submit_action(PlayerAction { player: p1, kind: PlayerActionKind::PassPriority }).unwrap();
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { .. }), "got {status:?}");
    assert!(engine.state().stack.is_empty(), "the ability resolved with no effect - nothing left on the stack");

    let errors = script_error_events(&engine);
    assert_eq!(errors.len(), 1, "the real operation script error is now logged, not silently dropped");
    assert!(errors[0].contains("boom"), "error message: {}", errors[0]);
}

fn vanilla_instant_definition() -> CardDefinition {
    minimal_card_definition(5, "Test Vanilla Instant", "vanilla_instant", ManaCost { symbols: Vec::new() }, CardTypeSet::INSTANT)
}

#[test]
fn a_vanilla_instant_with_no_operation_logs_no_script_error() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(0);
    state.objects.insert(card, card_object(card, vanilla_instant_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(&mut engine);
    engine.submit_action(PlayerAction { player: p1, kind: PlayerActionKind::PassPriority }).unwrap();
    let status = drain(&mut engine);

    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0), "got {status:?}");
    assert!(engine.state().stack.is_empty(), "the vanilla spell resolved with no effect, same as before this slice");
    assert!(
        script_error_events(&engine).is_empty(),
        "ScriptHandle(0) is the reserved 'nothing bound' placeholder every vanilla spell/ability carries - \
         must not be reported as a script failure"
    );
}

fn pending_cast_caster_definition() -> CardDefinition {
    CardDefinition {
        power: Some(PTValue::Fixed(2)),
        toughness: Some(PTValue::Fixed(2)),
        script_ref: ScriptReference::Inline(String::new()),
        ..minimal_card_definition(6, "Test Pending-Cast Caster", "unused", ManaCost { symbols: Vec::new() }, CardTypeSet::CREATURE)
    }
}

const OPERATION_CASTS_A_NONEXISTENT_OBJECT: &str = r#"
return function(source, controller)
    return Game.Ops.CastSpell(9999, { free = true })
end
"#;

#[test]
fn a_script_requested_cast_that_targets_a_nonexistent_object_logs_a_script_error() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let caster = ObjectId(0);
    let mut state = two_player_state();
    state.objects.insert(caster, card_object(caster, pending_cast_caster_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(caster);

    let mut engine = Engine::new(state);
    let condition = engine.load_script(CONDITION_ENTERS_BATTLEFIELD).expect("condition script loads");
    let operation = engine.load_script(OPERATION_CASTS_A_NONEXISTENT_OBJECT).expect("operation script loads");
    engine.state_mut().abilities.push(RegisteredAbility {
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
        id: AbilityId(0),
        source: caster,
        kind: AbilityKind::Triggered,
        condition: Some(ConditionSpec::Script(condition)),
        cost: None,
        target: None,
        operation,
        effect: None,
        intervening_if: None,
    });

    cast_spell(&mut engine, p0, caster);
    pay_free_and_drain(&mut engine);
    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(&mut engine);
    engine.submit_action(PlayerAction { player: p1, kind: PlayerActionKind::PassPriority }).unwrap();
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0), "got {status:?}");
    assert_eq!(engine.state().stack.len(), 1, "the ETB ability itself is now on the stack");

    // The operation requests an illegal cast: no panic, no stuck pipeline, no payment pause, and the failure is logged.
    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(&mut engine);
    engine.submit_action(PlayerAction { player: p1, kind: PlayerActionKind::PassPriority }).unwrap();
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { .. }), "got {status:?}");
    assert!(engine.state().stack.is_empty(), "the ability resolved with no effect - nothing left on the stack");

    let errors = script_error_events(&engine);
    assert_eq!(errors.len(), 1, "the illegal script-requested cast is now logged, not silently dropped");
    assert!(errors[0].contains("script-requested cast failed"), "error message: {}", errors[0]);
}
