//! Phase 5 vertical slice: `ActivateAbility` (CR 602) generalized off the
//! CR 605 mana-ability shape to a non-mana effect. Covers exactly `{T}` as
//! the whole cost (`AdditionalCost::TapSelf`) - unlike a mana ability, this
//! uses the stack (CR 602.2) rather than resolving immediately, and (as of
//! the general-activated-abilities slice) pauses on `AwaitingPayment` the
//! same way `CastSpell` does, even for this bare `{T}` cost - the actual tap
//! happens at `Engine::submit_payment` commit time, not at push time. Mana
//! cost, modes, and X values on an ability stay unmodeled; sacrifice/
//! discard/life/multi-tap additional costs are covered by
//! phase5_activate_ability_costs.rs instead of here.

use std::collections::{HashMap, VecDeque};

use stackfish_engine::commands::CommandError as EngineCommandError;
use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::*;

fn creature_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 1,
        name: "Test Cleric".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::CREATURE,
        subtypes: Vec::new(),
        oracle_text: "{T}: You gain 1 life.".to_string(),
        power: Some(PTValue::Fixed(1)),
        toughness: Some(PTValue::Fixed(1)),
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_cleric".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        effect: None,
        modes: Vec::new(),
    }
}

fn permanent_object(id: ObjectId, definition: CardDefinition, owner: PlayerId) -> GameObject {
    GameObject {
        object_id: id,
        copiable_override: None,
        lineage: ObjectLineage { previous: None },
        definition,
        owner,
        controller: owner,
        zone: ZoneKind::Battlefield,
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
        // Controlled well before turn 1 started - never summoning sick by
        // default. The sickness test overrides this field directly.
        controlled_since_turn: 0,
        is_token: false,
        is_commander: false,
        visibility: VisibilityPolicy { public: true, visible_to: Vec::new() },
    }
}

/// One player, main phase, empty stack, a battlefield creature with a
/// `{T}: gain 1 life` ability as its only registered ability.
fn state_with_source(source: ObjectId, controller: PlayerId, ability_owner: ObjectId) -> GameState {
    GameState {
        players: vec![PlayerState {
            player_id: controller,
            life: 20,
            mana_pool: ManaPool::default(),
            counters: CounterSet::new(),
            emblems: Vec::new(),
            commander_damage_taken: Vec::new(),
            eliminated: false,
            drew_from_empty_library: false,
            mulligans_taken: 0,
            hand_kept: false,
            commander_casts: std::collections::HashMap::new(),
        }],
        turn_order: vec![controller],
        active_player: controller,
        turn_number: 1,
        phase: Phase::Main,
        step: Step::PreCombatMain,
        priority_player: controller,
        consecutive_passes: 0,
        lands_played_this_turn: HashMap::new(),
        combat: None,
        last_active_turn_number: { let mut m = HashMap::new(); m.insert(controller, 1); m },
        objects: {
            let mut objects = HashMap::new();
            objects.insert(source, permanent_object(source, creature_definition(), controller));
            objects
        },
        zones: vec![Zone {
            kind: ZoneKind::Battlefield,
            owner: None,
            contents: VecDeque::from([source]),
        }],
        stack: Vec::new(),
        continuous_effects: Vec::new(),
        replacement_effects: Vec::new(),
        cost_modifiers: Vec::new(),
        hand_size_modifiers: Vec::new(),
        abilities: vec![RegisteredAbility {
            source: ability_owner,
            kind: AbilityKind::Activated,
            condition: None,
            cost: Some(CostSpec {
                mana: None,
                additional: vec![AdditionalCost::TapSelf],
                alternative: Vec::new(),
            }),
            target: None,
            operation: ScriptHandle(0),
            effect: Some(AbilityEffect::GainLife(1)),
            intervening_if: None,
        }],
        ledger: Vec::new(),
        rng: RngState { seed: 1, draws_consumed: 0 },
        next_event_id: 0,
        next_sequence_no: 0,
        next_object_id: 1000,
        pending_step_skips: Vec::new(),
        once_per_turn_triggered: std::collections::HashSet::new(),
        commander_return_offered: std::collections::HashSet::new(),
    }
}

fn drain(engine: &mut Engine) -> StepStatus {
    loop {
        let result = engine.step();
        match result.status {
            StepStatus::Progressed => continue,
            other => return other,
        }
    }
}

fn activate(engine: &mut Engine, player: PlayerId, source: ObjectId, targets: Vec<Target>) {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::ActivateAbility { source, ability_index: 0, targets, chosen_colors: Vec::new() },
        })
        .expect("player holds priority");
}

fn activate_and_drain(
    engine: &mut Engine,
    player: PlayerId,
    source: ObjectId,
    targets: Vec<Target>,
) -> StepStatus {
    activate(engine, player, source, targets);
    drain(engine)
}

/// This ability's whole cost is `{T}` on its own source - pays it via the
/// real `PaymentPlan` `Engine::submit_payment` now requires (CR 602.2f-h,
/// same `AwaitingPayment` pause `CastSpell`'s own tests pay through via
/// `pay_free_and_drain`), then drains the rest of that pipeline run. Asserts
/// the engine was actually sitting in `AwaitingPayment` first, same
/// self-check `pay_free_and_drain` performs elsewhere.
fn pay_tap_and_drain(engine: &mut Engine, source: ObjectId) -> StepStatus {
    let status = drain(engine);
    assert!(
        matches!(status, StepStatus::AwaitingPayment { .. }),
        "expected a pause for the ability's {{T}} cost, got {status:?}"
    );
    engine
        .submit_payment(PaymentPlan { payments: vec![CostPayment::Tap(source)] })
        .expect("a Tap(source) payment pays a bare TapSelf cost");
    drain(engine)
}

#[test]
fn tap_ability_pushes_stack_object_and_awaits_payment() {
    let p0 = PlayerId(0);
    let source = ObjectId(0);
    let state = state_with_source(source, p0, source);
    let mut engine = Engine::new(state);

    let status = activate_and_drain(&mut engine, p0, source, Vec::new());
    assert!(matches!(status, StepStatus::AwaitingPayment { player, .. } if player == p0));

    // The tap is a cost payment, not an immediate side effect of announcing
    // the activation (CR 602.2f-h) - it hasn't happened yet.
    assert!(!engine.state().object(source).unwrap().tapped, "source doesn't tap until payment commits");
    assert_eq!(engine.state().consecutive_passes, 0, "activating breaks the pass chain (CR 117.4)");
    assert_eq!(engine.state().priority_player, p0, "priority stays with the activator (CR 117.3c)");

    let events: Vec<_> = engine.state().ledger.iter().map(|e| &e.kind).collect();
    assert!(matches!(events[0], GameEventKind::StackObjectPushed { .. }));

    let status = pay_tap_and_drain(&mut engine, source);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert!(engine.state().object(source).unwrap().tapped, "source taps once the cost is paid");

    let events: Vec<_> = engine.state().ledger.iter().map(|e| &e.kind).collect();
    assert!(events.iter().any(|e| matches!(e, GameEventKind::Tapped { object } if *object == source)));
    assert!(events.iter().any(|e| matches!(e, GameEventKind::CostPaid { .. })));
}

#[test]
fn tap_ability_resolves_gain_life_via_fast_path_and_empties_stack() {
    let p0 = PlayerId(0);
    let source = ObjectId(0);
    let state = state_with_source(source, p0, source);
    let mut engine = Engine::new(state);

    activate(&mut engine, p0, source, Vec::new());
    pay_tap_and_drain(&mut engine, source);
    // Sole player passes priority with a non-empty stack - resolves the
    // pushed ActivatedAbility.
    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(&mut engine);

    assert_eq!(engine.state().player(p0).unwrap().life, 21, "gain 1 life resolved");
    assert!(engine.state().stack.is_empty(), "stack empties once the ability resolves");
    assert!(engine.state().object(source).unwrap().tapped);
}

#[test]
fn reject_when_actor_does_not_control_source() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let source = ObjectId(0);
    let mut state = state_with_source(source, p0, source);
    state.object_mut(source).unwrap().controller = p1;

    let mut engine = Engine::new(state);
    let status = activate_and_drain(&mut engine, p0, source, Vec::new());
    assert!(matches!(status, StepStatus::Error(EngineCommandError::IllegalAction(_))));
    assert!(!engine.state().object(source).unwrap().tapped, "no partial mutation");
}

#[test]
fn reject_when_source_already_tapped() {
    let p0 = PlayerId(0);
    let source = ObjectId(0);
    let mut state = state_with_source(source, p0, source);
    state.object_mut(source).unwrap().tapped = true;

    let mut engine = Engine::new(state);
    let status = activate_and_drain(&mut engine, p0, source, Vec::new());
    assert!(matches!(status, StepStatus::Error(EngineCommandError::IllegalAction(_))));
    assert!(engine.state().stack.is_empty(), "nothing pushed");
}

#[test]
fn reject_when_source_has_summoning_sickness() {
    let p0 = PlayerId(0);
    let source = ObjectId(0);
    let mut state = state_with_source(source, p0, source);
    // last_active_turn_number[p0] == 1 (state_with_source's default) - a
    // source controlled since turn 1 (the current turn) hasn't been
    // controlled since the turn *began*, so it's sick (CR 302.6, creature
    // source - state_with_source's fixture is already a creature).
    state.object_mut(source).unwrap().controlled_since_turn = 1;

    let mut engine = Engine::new(state);
    let status = activate_and_drain(&mut engine, p0, source, Vec::new());
    assert!(matches!(status, StepStatus::Error(EngineCommandError::IllegalAction(_))));
    assert!(!engine.state().object(source).unwrap().tapped, "no partial mutation");
    assert!(engine.state().stack.is_empty(), "nothing pushed");
}

#[test]
fn reject_ability_kind_not_activated() {
    let p0 = PlayerId(0);
    let source = ObjectId(0);
    let mut state = state_with_source(source, p0, source);
    state.abilities[0].kind = AbilityKind::Triggered;

    let mut engine = Engine::new(state);
    let status = activate_and_drain(&mut engine, p0, source, Vec::new());
    assert!(matches!(status, StepStatus::Error(EngineCommandError::IllegalAction(_))));
}

/// This ability's `target: None` (CR 601.2c's "no targets required") -
/// supplying one anyway is rejected the same as CastSpell would reject an
/// unrequested target (see phase5_targeting_basics.rs for the full
/// targeting-system slice). Nothing is mutated: no tap, no stack push.
#[test]
fn unrequested_target_is_rejected() {
    let p0 = PlayerId(0);
    let source = ObjectId(0);
    let state = state_with_source(source, p0, source);
    let mut engine = Engine::new(state);

    let status = activate_and_drain(&mut engine, p0, source, vec![Target::Player(p0)]);
    assert!(matches!(status, StepStatus::Error(EngineCommandError::InvalidTarget)));
    assert!(!engine.state().object(source).unwrap().tapped);
}
