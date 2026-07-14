//! Phase 5 vertical slice: `Game.Conditions.*`/`Game.Ops.*` primitives. A
//! condition/operation source string may now be a one-liner referencing one
//! of these Lua sugar tables (`return Game.Conditions.Enters`,
//! `return Game.Ops.GainLife(1)`) instead of a full hand-written function -
//! resolved once at load time into a Rust-native fast path (`ConditionSpec::
//! Primitive`/`RegisteredAbility::effect`) that never calls into Lua again at
//! trigger-check/resolve time. A full custom function remains fully
//! supported (`ConditionSpec::Script`/`operation` fallback), unchanged from
//! the prior slice.
//!
//! Registration itself is still Rust-side this slice (same as before) - a
//! `RegisteredAbility` is pushed directly via a struct literal, referencing
//! handles/specs obtained from `Engine::load_condition`/`Engine::load_operation`,
//! not via card-script self-registration through `Game.RegisterTriggeredAbility`
//! (still out of scope, see `script_api.rs`).

use std::collections::{HashMap, VecDeque};

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::triggers::collect_triggers;
use stackfish_engine::*;

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
        definition_id: 1,
        name: "Test Bear".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::CREATURE,
        subtypes: Vec::new(),
        oracle_text: "When this creature enters the battlefield, you gain 1 life.".to_string(),
        power: Some(PTValue::Fixed(2)),
        toughness: Some(PTValue::Fixed(2)),
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::Inline(String::new()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        effect: None,
    }
}

fn card_object(id: ObjectId, definition: CardDefinition, owner: PlayerId, zone: ZoneKind) -> GameObject {
    GameObject {
        object_id: id,
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
        visibility: VisibilityPolicy { public: true, visible_to: Vec::new() },
    }
}

/// Mirrors `phase5_script_bridge_trigger::two_player_state`.
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
        abilities: Vec::new(),
        ledger: Vec::new(),
        rng: RngState { seed: 1, draws_consumed: 0 },
        next_event_id: 0,
        next_sequence_no: 0,
        next_object_id: 1000,
        pending_step_skips: Vec::new(),
        once_per_turn_triggered: std::collections::HashSet::new(),
    }
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

// ---------------------------------------------------------------------
// load_condition: Game.Conditions.Enters behaves identically to the
// hand-written Lua function from the prior slice.
// ---------------------------------------------------------------------

#[test]
fn primitive_enters_condition_matches_the_bear_entering_the_battlefield() {
    let p0 = PlayerId(0);
    let bear = ObjectId(0);
    let mut runtime = ScriptRuntime::new();
    let condition = runtime.load_condition("return Game.Conditions.Enters").expect("primitive loads");
    assert!(matches!(condition, ConditionSpec::Primitive(ConditionPrimitive::EntersBattlefield)));

    let mut state = two_player_state();
    state.objects.insert(bear, card_object(bear, bear_definition(), p0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(bear);
    state.abilities.push(RegisteredAbility {
        source: bear,
        kind: AbilityKind::Triggered,
        condition: Some(condition),
        cost: None,
        target: None,
        operation: ScriptHandle(0), // never invoked by collect_triggers itself
        effect: None,
    });

    let event = zone_change_event(bear, ZoneKind::Hand, ZoneKind::Battlefield);
    let pending = collect_triggers(&state, &event, &runtime, &[]);
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].source, bear);
    assert_eq!(pending[0].controller, p0);
}

#[test]
fn primitive_enters_condition_false_on_a_different_object() {
    let p0 = PlayerId(0);
    let bear = ObjectId(0);
    let someone_else = ObjectId(1);
    let mut runtime = ScriptRuntime::new();
    let condition = runtime.load_condition("return Game.Conditions.Enters").expect("primitive loads");

    let mut state = two_player_state();
    state.objects.insert(bear, card_object(bear, bear_definition(), p0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(bear);
    state.abilities.push(RegisteredAbility {
        source: bear,
        kind: AbilityKind::Triggered,
        condition: Some(condition),
        cost: None,
        target: None,
        operation: ScriptHandle(0),
        effect: None,
    });

    let event = zone_change_event(someone_else, ZoneKind::Hand, ZoneKind::Battlefield);
    let pending = collect_triggers(&state, &event, &runtime, &[]);
    assert!(pending.is_empty(), "the bear's ETB condition shouldn't fire for a different object");
}

#[test]
fn primitive_enters_condition_false_on_wrong_destination_zone() {
    let p0 = PlayerId(0);
    let bear = ObjectId(0);
    let mut runtime = ScriptRuntime::new();
    let condition = runtime.load_condition("return Game.Conditions.Enters").expect("primitive loads");

    let mut state = two_player_state();
    state.objects.insert(bear, card_object(bear, bear_definition(), p0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(bear);
    state.abilities.push(RegisteredAbility {
        source: bear,
        kind: AbilityKind::Triggered,
        condition: Some(condition),
        cost: None,
        target: None,
        operation: ScriptHandle(0),
        effect: None,
    });

    let event = zone_change_event(bear, ZoneKind::Battlefield, ZoneKind::Graveyard);
    let pending = collect_triggers(&state, &event, &runtime, &[]);
    assert!(pending.is_empty());
}

// ---------------------------------------------------------------------
// load_condition: Game.Conditions.Always matches unconditionally.
// ---------------------------------------------------------------------

#[test]
fn primitive_always_condition_matches_an_unrelated_event() {
    let p0 = PlayerId(0);
    let bear = ObjectId(0);
    let mut runtime = ScriptRuntime::new();
    let condition = runtime.load_condition("return Game.Conditions.Always").expect("primitive loads");
    assert!(matches!(condition, ConditionSpec::Primitive(ConditionPrimitive::Always)));

    let mut state = two_player_state();
    state.objects.insert(bear, card_object(bear, bear_definition(), p0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(bear);
    state.abilities.push(RegisteredAbility {
        source: bear,
        kind: AbilityKind::Triggered,
        condition: Some(condition),
        cost: None,
        target: None,
        operation: ScriptHandle(0),
        effect: None,
    });

    // A totally unrelated event kind - matches anyway, same as a `None`
    // condition, but via an explicit Game.Conditions.Always spelling.
    let event = GameEvent {
        event_id: EventId(0),
        sequence_no: SequenceNo(0),
        kind: GameEventKind::PriorityPassed { player: p0 },
        caused_by_command: None,
        parent_event: None,
    };
    let pending = collect_triggers(&state, &event, &runtime, &[]);
    assert_eq!(pending.len(), 1, "Game.Conditions.Always means no filter - any event matches");
}

// ---------------------------------------------------------------------
// load_condition: a nil-producing chunk (e.g. a typo'd Game.Conditions.*
// lookup) is a load error, not silently treated as unconditional.
// ---------------------------------------------------------------------

#[test]
fn condition_script_returning_nil_is_a_load_error() {
    let mut runtime = ScriptRuntime::new();
    // Game.Conditions.Typo doesn't exist - indexing a Lua table with an
    // unknown key yields nil, not a runtime error, so this must be rejected
    // by load_condition itself rather than silently becoming "always".
    let result = runtime.load_condition("return Game.Conditions.Typo");
    assert!(result.is_err(), "a nil-producing condition chunk must fail to load, not become unconditional");
}

#[test]
fn condition_script_returning_a_table_is_a_load_error() {
    let mut runtime = ScriptRuntime::new();
    let result = runtime.load_condition("return { not_a = \"condition\" }");
    assert!(result.is_err(), "an unrecognized return shape must fail to load");
}

// ---------------------------------------------------------------------
// load_operation: Game.Ops.GainLife(1) decodes to a real AbilityEffect
// primitive, and a nil-producing chunk is likewise a load error.
// ---------------------------------------------------------------------

#[test]
fn operation_script_gain_life_decodes_to_the_ability_effect_primitive() {
    let mut runtime = ScriptRuntime::new();
    let (effect, handle) = runtime.load_operation("return Game.Ops.GainLife(3)").expect("primitive loads");
    assert!(matches!(effect, Some(AbilityEffect::GainLife(3))));
    assert_eq!(handle, ScriptHandle(0), "the operation handle is an unused placeholder when effect is Some");
}

#[test]
fn operation_script_returning_nil_is_a_load_error() {
    let mut runtime = ScriptRuntime::new();
    let result = runtime.load_operation("return nil");
    assert!(result.is_err());
}

// ---------------------------------------------------------------------
// End-to-end: the effect fast path resolves without ever invoking the
// operation callback - proven with an operation handle that would error if
// invoked at all.
// ---------------------------------------------------------------------

fn drain(engine: &mut Engine) -> StepStatus {
    loop {
        match engine.step().status {
            StepStatus::Progressed => continue,
            other => return other,
        }
    }
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
fn gain_life_effect_resolves_without_invoking_the_operation_script() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let bear = ObjectId(0);
    let mut state = two_player_state();
    state.objects.insert(bear, card_object(bear, bear_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(bear);

    let mut engine = Engine::new(state);
    let condition =
        engine.load_condition("return Game.Conditions.Enters").expect("condition primitive loads");
    // A real Lua function that errors if it's ever actually called - the
    // operation field still has to point at *something* (RegisteredAbility::
    // operation isn't itself Option), but ResolveStackTop must never reach
    // it while `effect` is `Some`.
    let poison_pill_operation = engine
        .load_script("return function(source, controller) error('operation should never run when effect is Some') end")
        .expect("poison-pill script loads");

    engine.state_mut().abilities.push(RegisteredAbility {
        source: bear,
        kind: AbilityKind::Triggered,
        condition: Some(condition),
        cost: None,
        target: None,
        operation: poison_pill_operation,
        effect: Some(AbilityEffect::GainLife(1)),
    });

    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::CastSpell { card: bear, modes: Vec::new(), targets: Vec::new(), x_value: None, alternative_cost: None },
        })
        .expect("p0 holds priority, bear is in hand, main phase, empty stack");
    pay_free_and_drain(&mut engine);

    pass(&mut engine, p0);
    drain(&mut engine);
    pass(&mut engine, p1);
    drain(&mut engine);

    pass(&mut engine, p0);
    drain(&mut engine);
    pass(&mut engine, p1);
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));

    assert!(engine.state().stack.is_empty(), "both the spell and its triggered ability have resolved");
    assert_eq!(
        engine.state().player(p0).unwrap().life,
        21,
        "GainLife(1) applied via the fast path - if the poison-pill operation script had run instead, \
         step() would have propagated its Lua error rather than reaching here at all"
    );
}

// ---------------------------------------------------------------------
// Mixed case: one ability on the primitive path, one on the hand-written
// script path, both firing correctly in the same game.
// ---------------------------------------------------------------------

#[test]
fn primitive_and_script_abilities_both_fire_in_the_same_game() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let primitive_bear = ObjectId(0);
    let script_bear = ObjectId(1);
    let mut state = two_player_state();
    state.objects.insert(primitive_bear, card_object(primitive_bear, bear_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(primitive_bear);
    state.objects.insert(script_bear, card_object(script_bear, bear_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(script_bear);

    let mut engine = Engine::new(state);

    let (effect, operation) =
        engine.load_operation("return Game.Ops.GainLife(1)").expect("operation primitive loads");
    let condition = engine.load_condition("return Game.Conditions.Enters").expect("condition primitive loads");
    engine.state_mut().abilities.push(RegisteredAbility {
        source: primitive_bear,
        kind: AbilityKind::Triggered,
        condition: Some(condition),
        cost: None,
        target: None,
        operation,
        effect,
    });

    let script_condition = engine.load_script(CONDITION_ENTERS_BATTLEFIELD).expect("condition script loads");
    let script_operation = engine.load_script(OPERATION_GAIN_LIFE_1).expect("operation script loads");
    engine.state_mut().abilities.push(RegisteredAbility {
        source: script_bear,
        kind: AbilityKind::Triggered,
        condition: Some(ConditionSpec::Script(script_condition)),
        cost: None,
        target: None,
        operation: script_operation,
        effect: None,
    });

    // Cast and resolve the primitive-path bear first.
    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::CastSpell {
                card: primitive_bear,
                modes: Vec::new(),
                targets: Vec::new(),
                x_value: None,
                alternative_cost: None,
            },
        })
        .expect("p0 holds priority");
    pay_free_and_drain(&mut engine);
    pass(&mut engine, p0);
    drain(&mut engine);
    pass(&mut engine, p1);
    drain(&mut engine);
    pass(&mut engine, p0);
    drain(&mut engine);
    pass(&mut engine, p1);
    drain(&mut engine);
    assert_eq!(engine.state().player(p0).unwrap().life, 21, "primitive-path ability applied its gain_life");

    // Cast and resolve the script-path bear second.
    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::CastSpell {
                card: script_bear,
                modes: Vec::new(),
                targets: Vec::new(),
                x_value: None,
                alternative_cost: None,
            },
        })
        .expect("p0 holds priority");
    pay_free_and_drain(&mut engine);
    pass(&mut engine, p0);
    drain(&mut engine);
    pass(&mut engine, p1);
    drain(&mut engine);
    pass(&mut engine, p0);
    drain(&mut engine);
    pass(&mut engine, p1);
    drain(&mut engine);

    assert_eq!(
        engine.state().player(p0).unwrap().life,
        22,
        "script-path ability also applied its gain_life - the two paths don't interfere"
    );
    assert!(engine.state().stack.is_empty());
}
