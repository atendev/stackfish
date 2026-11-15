//! CR 603.7: delayed triggered abilities made by a `Game.Ops.AtNextEndStep`
//! chain entry, from the row a resolution adds to the stack object it becomes.

use std::collections::HashMap;

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::sba::check_state_based_actions;
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::triggers::collect_triggers;
use stackfish_engine::*;
use stackfish_testutil::{
    card_object, drain, minimal_card_definition, two_player_state as base_two_player_state, zones,
};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const BEAR: ObjectId = ObjectId(1);
const WATCHER: ObjectId = ObjectId(2);
const SPELL: ObjectId = ObjectId(100);
const SECOND: ObjectId = ObjectId(101);
const THIRD: ObjectId = ObjectId(102);

fn decode(body: &str) -> Result<Option<AbilityEffect>, String> {
    let source =
        format!("local s = {{}}\nfunction s.operation()\n    return {body}\nend\nreturn s");
    ScriptRuntime::new()
        .build_operation(&source)
        .map(|decoded| decoded.expect("declares an operation").0)
        .map_err(|e| format!("{e:?}"))
}

fn op(body: &str) -> AbilityEffect {
    decode(body).expect("loads").expect("an operation")
}

fn creature(id: u64, name: &str) -> CardDefinition {
    CardDefinition {
        script_ref: ScriptReference::ByKey(name.to_string()),
        power: Some(PTValue::Fixed(2)),
        toughness: Some(PTValue::Fixed(2)),
        ..minimal_card_definition(
            id,
            name,
            "unused",
            ManaCost {
                symbols: Vec::new(),
            },
            CardTypeSet::CREATURE,
        )
    }
}

fn instant(
    id: u64,
    name: &str,
    effect: AbilityEffect,
    target: Option<TargetSpec>,
) -> CardDefinition {
    CardDefinition {
        effect: Some(effect),
        target_requirement: target,
        ..minimal_card_definition(
            id,
            name,
            "unused",
            ManaCost {
                symbols: Vec::new(),
            },
            CardTypeSet::INSTANT,
        )
    }
}

fn creature_in(zone: ZoneKind) -> Option<TargetSpec> {
    Some(TargetSpec::Primitive(TargetRequirement {
        filter: TargetFilter::Object {
            mana_value: None,
            excluded_card_types: None,
            excluded_subtypes: None,
            card_types: Some(CardTypeSet::CREATURE),
            subtypes: None,
            supertypes: None,
            excluded_supertypes: None,
            controller: None,
            zone: Some(zone),
        },
        count: 1,
        up_to: false,
        at_least: false,
        exclude_source: false,
    }))
}

/// Turn 2, `P0` active, with `P0` holding priority in `step`.
fn state_at(step: Step) -> GameState {
    let phase = if step == Step::End {
        Phase::Ending
    } else {
        Phase::Main
    };
    let mut state = GameState {
        turn_number: 2,
        phase,
        step,
        last_active_turn_number: HashMap::from([(P0, 2), (P1, 1)]),
        ..base_two_player_state(zones(
            P0,
            P1,
            &[
                ZoneKind::Hand,
                ZoneKind::Library,
                ZoneKind::Battlefield,
                ZoneKind::Graveyard,
                ZoneKind::Exile,
            ],
        ))
    };
    for i in 0..5 {
        for (owner, base) in [(P0, 200), (P1, 300)] {
            let id = base + i;
            place(
                &mut state,
                ObjectId(id),
                CardDefinition {
                    script_ref: ScriptReference::ByKey(format!("land {id}")),
                    ..minimal_card_definition(
                        id,
                        format!("Land {id}"),
                        "unused",
                        ManaCost {
                            symbols: Vec::new(),
                        },
                        CardTypeSet::LAND,
                    )
                },
                owner,
                ZoneKind::Library,
            );
        }
    }
    state
}

fn place(
    state: &mut GameState,
    id: ObjectId,
    def: CardDefinition,
    owner: PlayerId,
    zone: ZoneKind,
) {
    state.objects.insert(id, card_object(id, def, owner, zone));
    let scope = if matches!(zone, ZoneKind::Battlefield | ZoneKind::Exile) {
        None
    } else {
        Some(owner)
    };
    state.zone_mut(zone, scope).unwrap().push_bottom(id);
}

fn whip(id: u64) -> CardDefinition {
    instant(
        id,
        "Test Whip",
        op(
            r#"Game.Ops.Chain(Game.Ops.ReturnToBattlefield(), Game.Ops.AtNextEndStep(Game.Ops.Exile, "entered"))"#,
        ),
        creature_in(ZoneKind::Graveyard),
    )
}

/// A bear in `P0`'s graveyard and `whip` in `P0`'s hand.
fn whip_state(step: Step) -> GameState {
    let mut state = state_at(step);
    place(
        &mut state,
        BEAR,
        creature(1, "Bear"),
        P0,
        ZoneKind::Graveyard,
    );
    place(&mut state, SPELL, whip(99), P0, ZoneKind::Hand);
    state
}

fn cast(engine: &mut Engine, player: PlayerId, card: ObjectId, targets: Vec<Target>) {
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
                division: Vec::new(),
            },
        })
        .expect("the player holds priority and the targets are legal");
    let status = drain(engine);
    assert!(
        matches!(status, StepStatus::AwaitingPayment { .. }),
        "{status:?}"
    );
    engine
        .submit_payment(PaymentPlan {
            payments: Vec::new(),
        })
        .unwrap();
    drain(engine);
}

fn pass(engine: &mut Engine) -> StepStatus {
    let player = engine.state().priority_player;
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::PassPriority,
        })
        .expect("a priority window");
    drain(engine)
}

fn pass_both(engine: &mut Engine) -> StepStatus {
    pass(engine);
    pass(engine)
}

/// Passes with whoever holds priority until `turn`'s end step has begun.
fn pass_to_end_step(engine: &mut Engine, turn: u32) {
    for _ in 0..40 {
        let state = engine.state();
        if state.turn_number == turn && state.step == Step::End {
            return;
        }
        pass(engine);
    }
    panic!(
        "never reached turn {turn}'s end step: turn {} step {:?}",
        engine.state().turn_number,
        engine.state().step
    );
}

fn resolve_stack(engine: &mut Engine) {
    for _ in 0..10 {
        if engine.state().stack.is_empty() {
            return;
        }
        pass_both(engine);
    }
    panic!("the stack never emptied");
}

fn bear_zones(engine: &Engine) -> Vec<ZoneKind> {
    engine
        .state()
        .objects
        .values()
        .filter(|o| o.definition.name == "Bear")
        .map(|o| o.zone)
        .collect()
}

fn bear_on_battlefield(engine: &Engine) -> ObjectId {
    engine
        .state()
        .objects
        .values()
        .find(|o| o.definition.name == "Bear" && o.zone == ZoneKind::Battlefield)
        .map(|o| o.object_id)
        .expect("a bear on the battlefield")
}

fn delayed_on_stack(engine: &Engine) -> Vec<&StackObject> {
    engine
        .state()
        .stack
        .iter()
        .filter(|s| {
            !s.delayed_refers.is_empty()
                || s.ability_text
                    .as_deref()
                    .is_some_and(|t| t.starts_with("At the beginning of the next end step"))
        })
        .collect()
}

// CR 603.7, 603.7c
#[test]
fn a_returned_creature_is_exiled_at_the_next_end_step() {
    let mut engine = Engine::new(whip_state(Step::PostCombatMain));
    cast(&mut engine, P0, SPELL, vec![Target::Object(BEAR)]);
    pass_both(&mut engine);
    let returned = bear_on_battlefield(&engine);
    let rows = &engine.state().delayed_triggers;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].refers, vec![(returned, ZoneKind::Battlefield)]);

    pass_to_end_step(&mut engine, 2);
    assert_eq!(delayed_on_stack(&engine).len(), 1);
    assert!(
        engine.state().delayed_triggers.is_empty(),
        "the row leaves the list as it goes on the stack"
    );
    resolve_stack(&mut engine);
    assert_eq!(bear_zones(&engine), vec![ZoneKind::Exile]);
}

// CR 603.7d
#[test]
fn the_row_outlives_the_spell_that_made_it() {
    let mut engine = Engine::new(whip_state(Step::PostCombatMain));
    cast(&mut engine, P0, SPELL, vec![Target::Object(BEAR)]);
    let on_stack = engine.state().stack[0].source;
    pass_both(&mut engine);
    assert!(
        engine.state().object(on_stack).is_none(),
        "the spell left the stack"
    );
    let row = &engine.state().delayed_triggers[0];
    assert_eq!(row.source, on_stack);
    assert_eq!(
        row.source_lki.as_ref().map(|o| o.definition.name.as_str()),
        Some("Test Whip")
    );
    assert_eq!(row.controller, P0);
}

// CR 603.7d, 603.7e
#[test]
fn the_delayed_ability_is_controlled_by_whoever_controlled_the_spell() {
    let mut state = state_at(Step::PostCombatMain);
    place(
        &mut state,
        BEAR,
        creature(1, "Bear"),
        P1,
        ZoneKind::Graveyard,
    );
    place(&mut state, SPELL, whip(99), P1, ZoneKind::Hand);
    let mut engine = Engine::new(state);
    pass(&mut engine);
    cast(&mut engine, P1, SPELL, vec![Target::Object(BEAR)]);
    pass_both(&mut engine);
    assert_eq!(engine.state().delayed_triggers[0].controller, P1);

    pass_to_end_step(&mut engine, 2);
    let delayed = delayed_on_stack(&engine);
    assert_eq!(delayed.len(), 1);
    assert_eq!(delayed[0].controller, P1);
    resolve_stack(&mut engine);
    assert_eq!(bear_zones(&engine), vec![ZoneKind::Exile]);
}

// CR 513.2, 603.7a
#[test]
fn a_row_made_during_the_end_step_waits_for_the_next_turns_end_step() {
    let mut engine = Engine::new(whip_state(Step::End));
    cast(&mut engine, P0, SPELL, vec![Target::Object(BEAR)]);
    pass_both(&mut engine);
    assert!(engine.state().stack.is_empty());
    assert_eq!(engine.state().delayed_triggers.len(), 1);

    pass(&mut engine);
    pass(&mut engine);
    assert_ne!(engine.state().step, Step::End, "turn 2's end step is over");
    assert_eq!(
        engine.state().delayed_triggers.len(),
        1,
        "the end step the row was made in did not trigger it"
    );
    assert!(!engine.state().delayed_triggers[0].triggered);

    pass_to_end_step(&mut engine, 3);
    assert_eq!(delayed_on_stack(&engine).len(), 1);
    resolve_stack(&mut engine);
    assert_eq!(bear_zones(&engine), vec![ZoneKind::Exile]);
}

// CR 603.7a
#[test]
fn an_end_step_event_older_than_the_row_does_not_match_it() {
    let mut state = state_at(Step::End);
    state.delayed_triggers.push(DelayedTrigger {
        id: AbilityId(7),
        source: SPELL,
        source_lki: None,
        controller: P0,
        when: DelayedWhen::NextEndStep,
        effect: AbilityEffect::Exile,
        refers: Vec::new(),
        created_after: EventId(5),
        triggered: false,
    });
    let runtime = ScriptRuntime::new();
    let step_started = |id: u64| GameEvent {
        event_id: EventId(id),
        sequence_no: SequenceNo(id),
        kind: GameEventKind::StepStarted { step: Step::End },
        caused_by_command: None,
        parent_event: None,
    };
    let (older, _) = collect_triggers(&state, &step_started(4), &runtime, &[]);
    assert!(older.is_empty());
    let (later, _) = collect_triggers(&state, &step_started(5), &runtime, &[]);
    assert_eq!(later.len(), 1);
    assert_eq!(later[0].ability, AbilityId(7));
}

// CR 603.7b
#[test]
fn the_row_triggers_only_once() {
    let mut engine = Engine::new(whip_state(Step::PostCombatMain));
    cast(&mut engine, P0, SPELL, vec![Target::Object(BEAR)]);
    pass_both(&mut engine);
    pass_to_end_step(&mut engine, 2);
    resolve_stack(&mut engine);
    assert!(engine.state().delayed_triggers.is_empty());

    pass_to_end_step(&mut engine, 3);
    assert!(
        engine.state().stack.is_empty(),
        "the next end step finds no row"
    );
}

// CR 603.7c, 400.7
#[test]
fn a_creature_that_left_and_came_back_is_not_exiled() {
    let mut state = whip_state(Step::PostCombatMain);
    place(
        &mut state,
        SECOND,
        instant(
            98,
            "Test Kill",
            AbilityEffect::Destroy,
            creature_in(ZoneKind::Battlefield),
        ),
        P0,
        ZoneKind::Hand,
    );
    place(
        &mut state,
        THIRD,
        instant(
            97,
            "Test Raise",
            op("Game.Ops.ReturnToBattlefield()"),
            creature_in(ZoneKind::Graveyard),
        ),
        P0,
        ZoneKind::Hand,
    );
    let mut engine = Engine::new(state);
    cast(&mut engine, P0, SPELL, vec![Target::Object(BEAR)]);
    pass_both(&mut engine);
    let first = bear_on_battlefield(&engine);
    cast(&mut engine, P0, SECOND, vec![Target::Object(first)]);
    pass_both(&mut engine);
    let dead = engine
        .state()
        .objects
        .values()
        .find(|o| o.definition.name == "Bear")
        .map(|o| o.object_id)
        .unwrap();
    cast(&mut engine, P0, THIRD, vec![Target::Object(dead)]);
    pass_both(&mut engine);
    assert_ne!(bear_on_battlefield(&engine), first, "a new object");

    pass_to_end_step(&mut engine, 2);
    assert_eq!(delayed_on_stack(&engine).len(), 1);
    resolve_stack(&mut engine);
    assert_eq!(bear_zones(&engine), vec![ZoneKind::Battlefield]);
    let resolved = engine
        .state()
        .ledger
        .iter()
        .filter(|e| matches!(e.kind, GameEventKind::StackObjectResolved { .. }))
        .count();
    assert_eq!(resolved, 4, "the delayed ability resolved and did nothing");
}

// CR 603.7c, 400.7
#[test]
fn a_creature_that_leaves_while_the_ability_waits_is_skipped() {
    let mut state = whip_state(Step::PostCombatMain);
    place(
        &mut state,
        SECOND,
        instant(
            98,
            "Test Kill",
            AbilityEffect::Destroy,
            creature_in(ZoneKind::Battlefield),
        ),
        P0,
        ZoneKind::Hand,
    );
    let mut engine = Engine::new(state);
    cast(&mut engine, P0, SPELL, vec![Target::Object(BEAR)]);
    pass_both(&mut engine);
    let returned = bear_on_battlefield(&engine);

    pass_to_end_step(&mut engine, 2);
    assert_eq!(delayed_on_stack(&engine).len(), 1);
    cast(&mut engine, P0, SECOND, vec![Target::Object(returned)]);
    resolve_stack(&mut engine);
    assert_eq!(
        bear_zones(&engine),
        vec![ZoneKind::Graveyard],
        "the bear in the graveyard is a new object the ability does not exile"
    );
}

// CR 603.7c
#[test]
fn tokens_named_by_entered_are_sacrificed() {
    let mut state = state_at(Step::PostCombatMain);
    place(
        &mut state,
        SPELL,
        instant(
            99,
            "Test Clues",
            op(
                r#"Game.Ops.Chain(Game.Ops.CreateToken(Game.Tokens.Clue, 2), Game.Ops.AtNextEndStep(Game.Ops.Sacrifice, "entered"))"#,
            ),
            None,
        ),
        P0,
        ZoneKind::Hand,
    );
    let mut engine = Engine::new(state);
    cast(&mut engine, P0, SPELL, Vec::new());
    pass_both(&mut engine);
    let clues = |engine: &Engine| {
        engine
            .state()
            .objects
            .values()
            .filter(|o| o.zone == ZoneKind::Battlefield && o.definition.name.contains("Clue"))
            .count()
    };
    assert_eq!(clues(&engine), 2);
    assert_eq!(engine.state().delayed_triggers[0].refers.len(), 2);

    pass_to_end_step(&mut engine, 2);
    resolve_stack(&mut engine);
    assert_eq!(clues(&engine), 0);
}

// CR 603.7c
#[test]
fn a_card_a_replacement_sent_elsewhere_did_not_enter() {
    let mut state = whip_state(Step::PostCombatMain);
    place(
        &mut state,
        WATCHER,
        creature(2, "Watcher"),
        P1,
        ZoneKind::Battlefield,
    );
    state.replacement_effects.push(ReplacementEffect {
        source: WATCHER,
        spec: ReplacementSpec::Primitive {
            filter: EventFilter::WouldMoveZone {
                from: Some(ZoneKind::Graveyard),
                to: Some(ZoneKind::Battlefield),
                affected: EffectFilter {
                    controller: None,
                    card_types: None,
                    subtypes: None,
                    excluded_subtypes: None,
                    supertypes: None,
                    excluded_supertypes: None,
                    self_only: false,
                },
            },
            payload: ReplacementPayload::ReplaceZoneDestination(ZoneKind::Exile),
        },
        consumption: ConsumptionPolicy::Repeatable,
    });
    let mut engine = Engine::new(state);
    cast(&mut engine, P0, SPELL, vec![Target::Object(BEAR)]);
    pass_both(&mut engine);
    assert_eq!(bear_zones(&engine), vec![ZoneKind::Exile]);
    assert!(engine.state().delayed_triggers[0].refers.is_empty());
}

// CR 701.21a
#[test]
fn a_delayed_sacrifice_skips_a_creature_another_player_controls() {
    let mut state = state_at(Step::PostCombatMain);
    place(
        &mut state,
        BEAR,
        creature(1, "Bear"),
        P1,
        ZoneKind::Graveyard,
    );
    place(
        &mut state,
        SPELL,
        instant(
            99,
            "Test Lend",
            op(
                r#"Game.Ops.Chain(Game.Ops.ReturnToBattlefield({ control = "owner" }), Game.Ops.AtNextEndStep(Game.Ops.Sacrifice, "entered"))"#,
            ),
            creature_in(ZoneKind::Graveyard),
        ),
        P0,
        ZoneKind::Hand,
    );
    let mut engine = Engine::new(state);
    cast(&mut engine, P0, SPELL, vec![Target::Object(BEAR)]);
    pass_both(&mut engine);
    let returned = bear_on_battlefield(&engine);
    assert_eq!(engine.state().object(returned).unwrap().controller, P1);

    pass_to_end_step(&mut engine, 2);
    assert_eq!(delayed_on_stack(&engine).len(), 1);
    resolve_stack(&mut engine);
    assert_eq!(bear_zones(&engine), vec![ZoneKind::Battlefield]);
}

fn end_step_watcher(state: &mut GameState, controller: PlayerId) {
    place(
        state,
        WATCHER,
        creature(2, "Watcher"),
        controller,
        ZoneKind::Battlefield,
    );
    state.abilities.push(RegisteredAbility {
        text: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
        id: AbilityId(50),
        source: WATCHER,
        kind: AbilityKind::Triggered,
        condition: Some(ConditionSpec::Primitive(
            ConditionPrimitive::BeginningOfEachEndStep,
        )),
        cost: None,
        target: None,
        operation: ScriptHandle(0),
        effect: Some(AbilityEffect::GainLife(1)),
        intervening_if: None,
    });
}

// CR 603.3b, 603.7b
#[test]
fn a_row_matched_before_a_trigger_order_pause_goes_on_the_stack_once() {
    let mut state = whip_state(Step::PostCombatMain);
    end_step_watcher(&mut state, P0);
    let mut engine = Engine::new(state);
    cast(&mut engine, P0, SPELL, vec![Target::Object(BEAR)]);
    pass_both(&mut engine);
    pass(&mut engine);
    let player = engine.state().priority_player;
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::PassPriority,
        })
        .unwrap();
    let StepStatus::AwaitingTriggerOrderChoice { player, group } = drain(&mut engine) else {
        panic!("two end-step triggers of one controller ask for an order")
    };
    assert_eq!(player, P0);
    assert_eq!(group.len(), 2);
    assert!(engine.state().delayed_triggers[0].triggered);

    engine.submit_trigger_order(group).unwrap();
    drain(&mut engine);
    assert_eq!(engine.state().stack.len(), 2);
    assert_eq!(delayed_on_stack(&engine).len(), 1);
    assert!(engine.state().delayed_triggers.is_empty());
    resolve_stack(&mut engine);
    assert_eq!(bear_zones(&engine), vec![ZoneKind::Exile]);
}

// CR 603.3b
#[test]
fn a_delayed_trigger_is_ordered_with_other_end_step_triggers_in_apnap_order() {
    let mut state = whip_state(Step::PostCombatMain);
    end_step_watcher(&mut state, P1);
    let mut engine = Engine::new(state);
    cast(&mut engine, P0, SPELL, vec![Target::Object(BEAR)]);
    pass_both(&mut engine);
    pass_to_end_step(&mut engine, 2);

    let stack = &engine.state().stack;
    assert_eq!(stack.len(), 2);
    assert_eq!(
        stack[0].controller, P0,
        "the active player's trigger goes on first"
    );
    assert!(!stack[0].delayed_refers.is_empty());
    assert_eq!(stack[1].controller, P1);
    assert_eq!(stack[1].source, WATCHER);
}

// CR 800.4a, 800.4d
#[test]
fn a_leaving_players_rows_leave_with_them() {
    let mut state = state_at(Step::PostCombatMain);
    for (id, controller) in [(1, P0), (2, P1)] {
        state.delayed_triggers.push(DelayedTrigger {
            id: AbilityId(id),
            source: SPELL,
            source_lki: None,
            controller,
            when: DelayedWhen::NextEndStep,
            effect: AbilityEffect::Exile,
            refers: Vec::new(),
            created_after: EventId(0),
            triggered: false,
        });
    }
    state.player_mut(P1).unwrap().life = 0;
    check_state_based_actions(&mut state, &ScriptRuntime::new());
    let left: Vec<PlayerId> = state
        .delayed_triggers
        .iter()
        .map(|r| r.controller)
        .collect();
    assert_eq!(left, vec![P0]);
}

// CR 400.7j, 603.7c
#[test]
fn a_flickered_creature_returns_at_the_next_end_step() {
    let mut state = state_at(Step::PostCombatMain);
    place(
        &mut state,
        BEAR,
        creature(1, "Bear"),
        P1,
        ZoneKind::Battlefield,
    );
    place(
        &mut state,
        SPELL,
        instant(
            99,
            "Test Flicker",
            op(
                r#"Game.Ops.Chain(Game.Ops.Exile, Game.Ops.AtNextEndStep(Game.Ops.ReturnToBattlefield({ control = "owner" }), "targets"))"#,
            ),
            creature_in(ZoneKind::Battlefield),
        ),
        P0,
        ZoneKind::Hand,
    );
    let mut engine = Engine::new(state);
    cast(&mut engine, P0, SPELL, vec![Target::Object(BEAR)]);
    pass_both(&mut engine);
    assert_eq!(bear_zones(&engine), vec![ZoneKind::Exile]);
    let exiled = engine
        .state()
        .objects
        .values()
        .find(|o| o.definition.name == "Bear")
        .map(|o| o.object_id)
        .unwrap();
    assert_eq!(
        engine.state().delayed_triggers[0].refers,
        vec![(exiled, ZoneKind::Exile)],
        "the row follows the target into exile"
    );

    pass_to_end_step(&mut engine, 2);
    resolve_stack(&mut engine);
    let returned = bear_on_battlefield(&engine);
    assert_eq!(engine.state().object(returned).unwrap().controller, P1);
}

// CR 603.7
#[test]
fn at_next_end_step_outside_a_chain_is_a_load_error() {
    let alone = decode(r#"Game.Ops.AtNextEndStep(Game.Ops.Exile, "self")"#).expect_err("alone");
    assert!(
        alone.contains("must be an entry of Game.Ops.Chain"),
        "{alone}"
    );
    let nested =
        decode(r#"Game.Ops.ForEachPlayer(Game.Ops.AtNextEndStep(Game.Ops.Exile, "self"))"#)
            .expect_err("inside ForEachPlayer");
    assert!(nested.contains("ForEachPlayer expects"), "{nested}");
}

// CR 603.7
#[test]
fn at_next_end_step_rejects_other_ops_and_refers() {
    let bad_op = decode(
        r#"Game.Ops.Chain(Game.Ops.Exile, Game.Ops.AtNextEndStep(Game.Ops.DrawCards(1), "self"))"#,
    )
    .expect_err("DrawCards");
    assert!(bad_op.contains("must be Exile, Sacrifice"), "{bad_op}");
    let bad_refers =
        decode(r#"Game.Ops.Chain(Game.Ops.Exile, Game.Ops.AtNextEndStep(Game.Ops.Exile, "it"))"#)
            .expect_err("refers");
    assert!(bad_refers.contains("refers must be"), "{bad_refers}");
    let uncalled = decode(
        r#"Game.Ops.Chain(Game.Ops.Exile, Game.Ops.AtNextEndStep(Game.Ops.ReturnToBattlefield, "targets"))"#,
    )
    .expect_err("uncalled");
    assert!(
        uncalled.contains("must be a Game.Ops.* value"),
        "{uncalled}"
    );
}

// CR 603.7
#[test]
fn at_next_end_step_decodes_as_a_chain_entry() {
    assert_eq!(
        op(r#"Game.Ops.Chain(Game.Ops.Exile, Game.Ops.AtNextEndStep(Game.Ops.Destroy, "self"))"#),
        AbilityEffect::Chain {
            entries: vec![
                AbilityEffect::Exile,
                AbilityEffect::AtNextEndStep {
                    effect: Box::new(AbilityEffect::Destroy),
                    refers: DelayedRefers::SelfObject,
                },
            ],
            words: None,
        }
    );
}
