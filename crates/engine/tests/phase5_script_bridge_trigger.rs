//! Phase 5 vertical slice: narrow script bridge. One concrete triggered
//! ability (CR 603) - real Lua condition + operation, both invoked for real
//! via `ScriptRuntime` - powering an ETB "gain 1 life" trigger end to end.
//!
//! Registration itself stays Rust-side this slice: `RegisteredAbility` is
//! pushed directly onto `state.abilities` (via `Engine::load_script` +
//! a struct literal), not via a card script self-registering through
//! `Game.RegisterTriggeredAbility` (still `todo!()` - needs the `Game.*` Lua
//! table bound plus registration-time mutability sorted out, both flagged in
//! script.rs/script_api.rs).
//!
//! Design lock-in this slice exercises: "Lua asks, Rust decides" - the
//! operation script never mutates state itself, it returns a plain table
//! (`{ gain_life = 1 }`) that `ScriptRuntime::invoke_operation` decodes and
//! applies. Only `GameEventKind::ZoneChange` is bridged into a Lua-visible
//! shape (see `script::event_to_lua`) - enough for an ETB-shaped condition,
//! nothing else.

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
// Unit-level: collect_triggers' reaction to condition true/false/None,
// against a real ScriptRuntime, without driving the whole Engine pipeline.
// ---------------------------------------------------------------------

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
        source: bear,
        kind: AbilityKind::Triggered,
        condition: Some(ConditionSpec::Script(condition)),
        cost: None,
        target: None,
        operation: ScriptHandle(0), // never invoked by collect_triggers itself
        effect: None,
    });

    let event = zone_change_event(bear, ZoneKind::Hand, ZoneKind::Battlefield);
    let pending = collect_triggers(&state, &event, &runtime);
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].source, bear);
    assert_eq!(pending[0].controller, p0);
    assert_eq!(pending[0].ability_index, 0);
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
        source: bear,
        kind: AbilityKind::Triggered,
        condition: Some(ConditionSpec::Script(condition)),
        cost: None,
        target: None,
        operation: ScriptHandle(0),
        effect: None,
    });

    // A different object entering the battlefield - the condition checks
    // `event.object == source`, so this shouldn't match.
    let event = zone_change_event(someone_else, ZoneKind::Hand, ZoneKind::Battlefield);
    let pending = collect_triggers(&state, &event, &runtime);
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
        source: bear,
        kind: AbilityKind::Triggered,
        condition: Some(ConditionSpec::Script(condition)),
        cost: None,
        target: None,
        operation: ScriptHandle(0),
        effect: None,
    });

    // The bear moving to the graveyard (not the battlefield) shouldn't
    // match an ETB condition.
    let event = zone_change_event(bear, ZoneKind::Battlefield, ZoneKind::Graveyard);
    let pending = collect_triggers(&state, &event, &runtime);
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
        source: bear,
        kind: AbilityKind::Triggered,
        // No condition at all - RegisteredAbility::condition documents this
        // as "no filter, always triggers".
        condition: None,
        cost: None,
        target: None,
        operation: ScriptHandle(0),
        effect: None,
    });

    // A totally unrelated event kind - matches anyway, since there's no
    // condition to filter it out.
    let event = GameEvent {
        event_id: EventId(0),
        sequence_no: SequenceNo(0),
        kind: GameEventKind::PriorityPassed { player: p0 },
        caused_by_command: None,
        parent_event: None,
    };
    let pending = collect_triggers(&state, &event, &runtime);
    assert_eq!(pending.len(), 1, "no condition means no filter - any event matches");
}

#[test]
fn engine_load_script_rejects_malformed_lua() {
    let mut engine = Engine::new(two_player_state());
    let result = engine.load_script("this is not valid lua {{{");
    assert!(result.is_err(), "a syntax error should fail to load, not panic");
}

// ---------------------------------------------------------------------
// End-to-end: real Engine pipeline, cast a creature with a real Lua ETB
// trigger, resolve it, watch the operation script's life gain land.
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
        source: bear,
        kind: AbilityKind::Triggered,
        condition: Some(ConditionSpec::Script(condition)),
        cost: None,
        target: None,
        operation,
        effect: None,
    });

    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::CastSpell { card: bear, modes: Vec::new(), targets: Vec::new(), x_value: None, alternative_cost: None },
        })
        .expect("p0 holds priority, bear is in hand, main phase, empty stack");
    pay_free_and_drain(&mut engine);

    // Spell is on the stack, not resolved yet - only a hand->stack
    // ZoneChange has happened so far, which doesn't match the ETB
    // condition (`to == "battlefield"`) - no ability should be pending.
    assert_eq!(engine.state().stack.len(), 1, "only the spell itself is on the stack so far");
    assert_eq!(engine.state().player(p0).unwrap().life, 20, "no life gain before the creature resolves");

    // Both players pass - CR 117.4 resolves the top (the spell) since the
    // stack is non-empty. Resolution moves the creature to the battlefield,
    // which is exactly the ZoneChange the ETB condition matches - the
    // ability's own StackObject gets pushed in this same pipeline run.
    pass(&mut engine, p0);
    drain(&mut engine);
    pass(&mut engine, p1);
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));

    assert_eq!(engine.state().stack.len(), 1, "the ETB ability itself is now on the stack");
    assert_eq!(engine.state().player(p0).unwrap().life, 20, "ability hasn't resolved yet");

    // Both players pass again - resolves the triggered ability, invoking its
    // real Lua operation callback via the script bridge.
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
    // The ability is registered to `bear` specifically - casting a
    // different creature (even with the same definition) shouldn't fire it.
    engine.state_mut().abilities.push(RegisteredAbility {
        source: bear,
        kind: AbilityKind::Triggered,
        condition: Some(ConditionSpec::Script(condition)),
        cost: None,
        target: None,
        operation,
        effect: None,
    });

    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::CastSpell {
                card: other_creature,
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
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { .. }));

    assert!(engine.state().stack.is_empty(), "no ability should have triggered off a different object");
    assert_eq!(engine.state().player(p0).unwrap().life, 20, "no life gain - the bear itself never entered");
}
