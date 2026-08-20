//! Phase 5 vertical slice: closes the "a script error here is silently
//! dropped" cluster left across `engine/action.rs`, `engine/choice_answers.rs`,
//! `engine/payment.rs`, `engine/resolve/tokens.rs`, `engine/resolve/
//! misc_effects.rs`, `engine/resolve/each_player.rs`, and `engine/
//! library_choices.rs` - every `run_card_script`/`invoke_operation`/
//! `resume_operation`/`resume_operation_bool` call after the initial cast-
//! time invocation used to do `let _ = ...`, discarding a real Lua error.
//! Sibling slice to the already-merged `phase5/script-error-surfacing`
//! (`phase5_script_error_surfacing.rs`, `invoke_condition`/
//! `invoke_intervening_if`) - same fallback-unchanged, log-a-
//! `GameEventKind::ScriptError` shape, applied to the rest of the script
//! bridge's call sites.
//!
//! Unlike `invoke_condition`/`invoke_intervening_if` (only ever called on a
//! `ScriptHandle` a script already registered), `run_card_script`/
//! `invoke_operation` are called unconditionally - including for a "vanilla,
//! no real script" card, which is the ordinary shape of most of this
//! engine's own test fixtures (`ScriptReference::ByKey`, an empty `Inline`
//! source, or the `ScriptHandle(0)` "nothing bound" placeholder every
//! permanent/instant/sorcery with no script-declared operation carries -
//! see `cast.rs`'s own `(ScriptHandle(0), other)` match arm). Logging
//! unconditionally there was found to break a real, wide swath of this
//! engine's own `ledger.len()`-asserting tests (`run_card_script` fires for
//! *every* permanent that resolves onto the battlefield via `payment.rs`'s
//! `run_script` flag) - so both `record_script_error`
//! (`engine/mod.rs`) and `resolve_misc_or_script_effect`'s own
//! `invoke_operation` arm deliberately skip logging for those "no real
//! script" shapes, narrowed to only the two genuinely-reachable "real
//! script threw" cases this cluster is actually about.
//!
//! Tried and reverted: reserving `ScriptHandle(0)` as an actually-
//! unallocatable sentinel (`ScriptRuntime::new`'s `next_handle` starting at
//! 1 instead of 0) to close the exact "a real script's first-ever handle
//! could coincide with the placeholder" ambiguity
//! `phase5_script_operation.rs`'s own
//! `operation_function_decodes_a_custom_function_to_a_real_handle` test
//! comment already named. Broke a real, unrelated, already-passing test
//! (`phase5_mass_choice_effects.rs`'s
//! `for_each_player_asks_in_apnap_order_and_sacrifices_simultaneously`) -
//! root-caused via direct instrumentation (not guessed): `Game.Ops.
//! ForEachPlayer(fn)`'s own wrapped-function handle is a real, separately
//! allocated `ScriptHandle`, but `cast.rs`'s own `(operation, effect) =
//! match effect { Some(Script(handle)) => (handle, None), other =>
//! (ScriptHandle(0), other) }` unpacking still discards it down to the same
//! placeholder `ScriptHandle(0)` for any non-`Script` effect shape
//! (`ForEachPlayer` included) - a real, pre-existing bug in that unpacking
//! this engine was *already* silently depending on: every test exercising
//! this shape happened to load its function as the very first handle in a
//! fresh registry, so the placeholder and the real handle coincided by
//! accident. Reserving 0 broke that accidental coincidence without fixing
//! the underlying discard - a separate, bigger gap, named here rather than
//! attempted. `resolve_misc_or_script_effect`'s own `invoke_operation` arm
//! still skips logging for `ScriptHandle(0)` (see its own doc comment for
//! the full, now-narrower reasoning) - the narrow ambiguity stays open,
//! unchanged from before this slice.
//!
//! One representative test per distinct code shape touched, not one per
//! call site: `action.rs`'s `battlefield_entries` loop and the other eight
//! `run_card_script` call sites (`choice_answers.rs`, `payment.rs`,
//! `resolve/tokens.rs`, `resolve/misc_effects.rs` x3, `library_choices.rs`)
//! all funnel through the exact same two-line `record_script_error` call -
//! proving it once via `PlayLand` proves the shared mechanism; a test per
//! site would just re-test the same three-line helper body.

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

// ---------------------------------------------------------------------
// `run_card_script` family (`record_script_error`, `engine/mod.rs`) -
// exercised through `PlayLand`'s `battlefield_entries` loop
// (`action.rs`), the one call site that never needs a cast/payment pause.
// ---------------------------------------------------------------------

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

// ---------------------------------------------------------------------
// Regression guard for the highest-blast-radius `run_card_script` call
// site: `payment.rs`'s `move_resolving_spell` runs for *every* permanent
// that resolves onto the battlefield via a normal cast.
// ---------------------------------------------------------------------

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

// ---------------------------------------------------------------------
// `invoke_operation` (`resolve_misc_or_script_effect`, `misc_effects.rs`) -
// a triggered ability whose real, genuinely-loaded operation handle
// throws.
// ---------------------------------------------------------------------

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

    // Resolving the ability invokes the erroring operation - no panic, the
    // ability still resolves with no effect (unchanged fallback), but the
    // real error is now logged.
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

// ---------------------------------------------------------------------
// Regression guard: a vanilla instant/sorcery with no script-declared
// operation carries the reserved `ScriptHandle(0)` placeholder - must not
// be reported as a script failure either.
// ---------------------------------------------------------------------

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

// ---------------------------------------------------------------------
// `resolve_pending_cast` (`misc_effects.rs`) - a script-requested cast
// that targets a nonexistent object. Mirrors
// `phase5_script_cast_spell.rs`'s own
// `casting_a_nonexistent_object_does_not_panic_and_the_triggering_ability_still_resolves`,
// adding this slice's own new ScriptError assertion.
// ---------------------------------------------------------------------

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

    // Resolving the ability invokes the operation, which requests an
    // illegal cast - no panic, no stuck pipeline, and no payment pause (the
    // cast never happened), same as before this slice - but the failure is
    // now logged.
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
