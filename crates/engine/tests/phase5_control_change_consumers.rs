//! Phase 5 vertical slice: wires `layers::effective_controller` (the
//! query-only `phase5/layer-engine-control` slice) into real gameplay
//! consumers - backlog item 1.5b's follow-up. Every site migrated here was
//! previously reading `GameObject.controller` directly (never reacting to a
//! `GainControl` continuous effect); this file drives the real
//! `Engine`/`execute_command` pipeline to prove each migrated consumer
//! actually respects a control-changed permanent's *current* controller
//! instead of whoever it started under. Sites deliberately left unmigrated
//! (Hand/Graveyard/Command/Library-zone controller checks, `ContinuousEffect::
//! applies_to`'s own internals - see docs/implementation.md's
//! `phase5/layer-engine-control-consumers` slice notes for the
//! infinite-recursion reasoning) aren't covered here, since nothing about
//! them changed.

use std::collections::{HashMap, VecDeque};

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::*;

fn creature_definition(name: &str) -> CardDefinition {
    CardDefinition {
        definition_id: 1,
        name: name.to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::CREATURE,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: Some(PTValue::Fixed(2)),
        toughness: Some(PTValue::Fixed(2)),
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey(name.to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        effect: None,
        modes: Vec::new(),
    }
}

fn permanent(id: ObjectId, definition: CardDefinition, owner: PlayerId, controlled_since_turn: u32) -> GameObject {
    GameObject {
        object_id: id,
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
        controlled_since_turn,
        is_token: false,
        is_commander: false,
        visibility: VisibilityPolicy { public: true, visible_to: Vec::new() },
    }
}

fn player_state(id: PlayerId, life: i32) -> PlayerState {
    PlayerState {
        player_id: id,
        life,
        mana_pool: ManaPool::default(),
        counters: CounterSet::new(),
        emblems: Vec::new(),
        commander_damage_taken: Vec::new(),
        eliminated: false,
        drew_from_empty_library: false,
        mulligans_taken: 0,
        hand_kept: false,
        commander_casts: std::collections::HashMap::new(),
    }
}

fn put_on_battlefield(state: &mut GameState, object: GameObject) {
    let id = object.object_id;
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(id);
    state.objects.insert(id, object);
}

/// A permanent `GainControl`s continuous effect from `source` to
/// `new_controller`, unfiltered, timestamp 0 - the query-only slice's own
/// bootstrap fixture shape (`phase5_layer_engine_control.rs`), reused here
/// against the real engine pipeline instead of the bare `effective_controller`
/// query.
fn gain_control(source: ObjectId, new_controller: PlayerId) -> ContinuousEffect {
    ContinuousEffect {
        source,
        layer: Layer::ControlChangingEffects,
        affected: AffectedFilter::Primitive(EffectFilter { controller: None, card_types: None, self_only: false }),
        effect: EffectPayload::Primitive(EffectPrimitive::GainControl(new_controller)),
        timestamp: SequenceNo(0),
        duration: EffectDuration::Permanent,
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

/// `p0` active, `p1` nonactive - `p1` owns `creature`, but a `GainControl`
/// effect gives `p0` its effective control. `p0`'s own turn started at
/// `p0_last_active_turn` (default combat-ready shape has both players'
/// `last_active_turn_number` seeded so summoning-sickness math is exercised
/// deliberately, not accidentally defaulted).
fn control_changed_combat_state(p0_last_active_turn: u32, creature_controlled_since_turn: u32) -> (GameState, PlayerId, PlayerId, ObjectId) {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let creature = ObjectId(0);
    let mut state = GameState {
        players: vec![player_state(p0, 20), player_state(p1, 20)],
        turn_order: vec![p0, p1],
        active_player: p0,
        // p1 is a "veteran" (last active turn 10) throughout - fixed so the
        // two summoning-sickness tests can demonstrate the check now keys
        // off p0 (the effective controller), which would give a *different*
        // answer than checking against p1 (the raw owner) still would.
        turn_number: 10,
        phase: Phase::Combat,
        step: Step::DeclareAttackers,
        priority_player: p0,
        consecutive_passes: 0,
        lands_played_this_turn: HashMap::new(),
        combat: None,
        last_active_turn_number: HashMap::from([(p0, p0_last_active_turn), (p1, 10)]),
        objects: HashMap::new(),
        zones: vec![
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
        commander_return_offered: std::collections::HashSet::new(),
    };
    put_on_battlefield(&mut state, permanent(creature, creature_definition("Bear"), p1, creature_controlled_since_turn));
    state.continuous_effects.push(gain_control(creature, p0));
    (state, p0, p1, creature)
}

// ---------------------------------------------------------------------
// DeclareAttackers/DeclareBlockers (commands.rs's effective_controller checks)
// ---------------------------------------------------------------------

#[test]
fn only_the_new_controller_can_declare_a_gain_controlled_creature_as_an_attacker() {
    let (state, p0, _p1, creature) = control_changed_combat_state(1, 0);
    let mut engine = Engine::new(state);

    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::DeclareAttackers { attackers: vec![(creature, Target::Player(PlayerId(1)))] },
        })
        .expect("player holds priority");
    let status = drain(&mut engine);
    assert!(
        matches!(status, StepStatus::AwaitingAction { .. }),
        "p0 (the GainControl'd effective controller) should be allowed to attack with it, got {status:?}"
    );
    assert!(engine.state().object(creature).unwrap().tapped, "attacking taps it");
}

#[test]
fn the_original_owner_can_no_longer_attack_with_a_gain_controlled_creature() {
    let (state, _p0, p1, creature) = control_changed_combat_state(1, 0);
    let mut state = state;
    // p1 isn't the active player in this fixture, so also flip active_player
    // to p1 to isolate the actual claim under test: control, not turn order.
    state.active_player = p1;
    state.priority_player = p1;
    let mut engine = Engine::new(state);

    engine
        .submit_action(PlayerAction {
            player: p1,
            kind: PlayerActionKind::DeclareAttackers { attackers: vec![(creature, Target::Player(PlayerId(0)))] },
        })
        .expect("player holds priority");
    let status = drain(&mut engine);
    assert!(
        matches!(status, StepStatus::Error(_)),
        "p1 (the original owner, no longer the effective controller) must not be able to attack with it"
    );
    assert!(!engine.state().object(creature).unwrap().tapped, "rejected declaration must not tap it");
}

#[test]
fn only_the_new_controller_can_declare_it_as_a_blocker() {
    let (mut state, p0, p1, creature) = control_changed_combat_state(1, 0);
    // An unrelated p0 creature attacks p1, so p1 has something to block with
    // (blocking legality itself isn't under test - the control check is).
    let attacker = ObjectId(1);
    put_on_battlefield(&mut state, permanent(attacker, creature_definition("Attacker"), p0, 0));
    state.combat = Some(CombatState {
        attackers: vec![(attacker, Target::Player(p1))],
        blocks: Vec::new(),
        declared_defenders: Vec::new(),
    });
    state.step = Step::DeclareBlockers;
    let mut engine = Engine::new(state);

    // p1 no longer effectively controls `creature` (GainControl gave it to
    // p0) - blocking with it must be rejected even though p1 owns it.
    engine
        .submit_action(PlayerAction {
            player: p1,
            kind: PlayerActionKind::DeclareBlockers { blocks: vec![(creature, attacker)] },
        })
        .expect("player holds priority");
    let status = drain(&mut engine);
    assert!(
        matches!(status, StepStatus::Error(_)),
        "p1 no longer effectively controls the creature and must not be able to block with it"
    );
}

// ---------------------------------------------------------------------
// has_summoning_sickness
// ---------------------------------------------------------------------

/// `creature`'s `controlled_since_turn` (5) is *after* p0's own most recent
/// turn began (turn 1) but *before* veteran p1's (turn 10). Checked against
/// the raw `object.controller` (p1, the original owner) this would read as
/// long-since-controlled and NOT sick (`5 >= 10` is false) - the exact wrong
/// answer the pre-migration code would have given. Checked against the
/// *effective* controller (p0, via `GainControl` - `layers::
/// effective_controller`) it's `5 >= 1`, true, so it's sick under its new
/// controller - the real, corrected answer this slice's migration produces.
#[test]
fn summoning_sickness_is_checked_against_the_new_controllers_last_active_turn() {
    let (state, p0, _p1, creature) = control_changed_combat_state(1, 5);
    let mut engine = Engine::new(state);

    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::DeclareAttackers { attackers: vec![(creature, Target::Player(PlayerId(1)))] },
        })
        .expect("player holds priority");
    let status = drain(&mut engine);
    assert!(
        matches!(status, StepStatus::Error(_)),
        "the creature should read as summoning sick under its new controller p0 (5 >= p0's own \
         last active turn 1), got {status:?}"
    );
}

#[test]
fn summoning_sickness_is_bypassed_when_controlled_before_the_new_controllers_own_turn() {
    // Same fixture, but the creature has been controlled since turn 0 -
    // before p0's own most recent turn (1) began, so `0 >= 1` is false: not
    // sick under its new controller, and allowed to attack.
    let (state, p0, _p1, creature) = control_changed_combat_state(1, 0);
    let mut engine = Engine::new(state);

    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::DeclareAttackers { attackers: vec![(creature, Target::Player(PlayerId(1)))] },
        })
        .expect("player holds priority");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { .. }), "got {status:?}");
}

// ---------------------------------------------------------------------
// ActivateAbility (commands.rs's "actor does not control the source" check)
// ---------------------------------------------------------------------

fn control_changed_activation_state() -> (GameState, PlayerId, PlayerId, ObjectId) {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let source = ObjectId(0);
    let mut state = GameState {
        players: vec![player_state(p0, 20), player_state(p1, 20)],
        turn_order: vec![p0, p1],
        active_player: p0,
        turn_number: 1,
        phase: Phase::Main,
        step: Step::PreCombatMain,
        priority_player: p0,
        consecutive_passes: 0,
        lands_played_this_turn: HashMap::new(),
        combat: None,
        last_active_turn_number: HashMap::from([(p0, 1), (p1, 1)]),
        objects: HashMap::new(),
        zones: vec![
            Zone { kind: ZoneKind::Battlefield, owner: None, contents: VecDeque::new() },
            Zone { kind: ZoneKind::Graveyard, owner: Some(p0), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Graveyard, owner: Some(p1), contents: VecDeque::new() },
        ],
        stack: Vec::new(),
        continuous_effects: Vec::new(),
        replacement_effects: Vec::new(),
        cost_modifiers: Vec::new(),
        abilities: vec![RegisteredAbility {
            source,
            kind: AbilityKind::Activated,
            condition: None,
            cost: Some(CostSpec { mana: None, additional: Vec::new(), alternative: Vec::new() }),
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
    };
    put_on_battlefield(&mut state, permanent(source, creature_definition("Source"), p1, 0));
    state.continuous_effects.push(gain_control(source, p0));
    (state, p0, p1, source)
}

#[test]
fn the_new_controller_can_activate_a_gain_controlled_sources_ability() {
    let (state, p0, _p1, source) = control_changed_activation_state();
    let mut engine = Engine::new(state);

    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::ActivateAbility {
                source,
                ability_index: 0,
                targets: Vec::new(),
                chosen_colors: Vec::new(),
            },
        })
        .expect("p0 (the effective controller) should be able to activate it");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { .. }) || matches!(status, StepStatus::AwaitingPayment { .. }));
}

#[test]
fn the_original_owner_can_no_longer_activate_a_gain_controlled_sources_ability() {
    let (state, _p0, p1, source) = control_changed_activation_state();
    let mut engine = Engine::new(state);

    let result = engine.submit_action(PlayerAction {
        player: p1,
        kind: PlayerActionKind::ActivateAbility {
            source,
            ability_index: 0,
            targets: Vec::new(),
            chosen_colors: Vec::new(),
        },
    });
    match result {
        Ok(_) => {
            let status = drain(&mut engine);
            assert!(
                matches!(status, StepStatus::Error(_)),
                "p1 no longer effectively controls the source and must not be able to activate it, got {status:?}"
            );
        }
        Err(_) => {} // rejected up front - also an acceptable "not allowed" outcome
    }
}

// ---------------------------------------------------------------------
// PaymentPlan::validate's Sacrifice cost (CR 701.20a: "you can only
// sacrifice a permanent you control")
// ---------------------------------------------------------------------

#[test]
fn sacrifice_cost_accepts_a_gain_controlled_creature_from_its_new_controller() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let source = ObjectId(0);
    let fodder = ObjectId(1);
    let cost = CostSpec {
        mana: None,
        additional: vec![AdditionalCost::Sacrifice {
            filter: FilterSpec {
                card_types: Some(CardTypeSet::CREATURE),
                supertypes: None,
                subtypes: None,
                controller: None,
                random: false,
                extra: None,
            },
        }],
        alternative: Vec::new(),
    };
    let mut state = GameState {
        players: vec![player_state(p0, 20), player_state(p1, 20)],
        turn_order: vec![p0, p1],
        active_player: p0,
        turn_number: 1,
        phase: Phase::Main,
        step: Step::PreCombatMain,
        priority_player: p0,
        consecutive_passes: 0,
        lands_played_this_turn: HashMap::new(),
        combat: None,
        last_active_turn_number: HashMap::from([(p0, 1), (p1, 1)]),
        objects: HashMap::new(),
        zones: vec![
            Zone { kind: ZoneKind::Battlefield, owner: None, contents: VecDeque::new() },
            Zone { kind: ZoneKind::Graveyard, owner: Some(p0), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Graveyard, owner: Some(p1), contents: VecDeque::new() },
        ],
        stack: Vec::new(),
        continuous_effects: Vec::new(),
        replacement_effects: Vec::new(),
        cost_modifiers: Vec::new(),
        abilities: vec![RegisteredAbility {
            source,
            kind: AbilityKind::Activated,
            condition: None,
            cost: Some(cost),
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
    };
    // The ability's source is p0's own, ordinary permanent - only `fodder`
    // (owned by p1, GainControl'd to p0) is under test.
    put_on_battlefield(&mut state, permanent(source, creature_definition("Source"), p0, 0));
    put_on_battlefield(&mut state, permanent(fodder, creature_definition("Fodder"), p1, 0));
    state.continuous_effects.push(gain_control(fodder, p0));

    let mut engine = Engine::new(state);
    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::ActivateAbility {
                source,
                ability_index: 0,
                targets: Vec::new(),
                chosen_colors: Vec::new(),
            },
        })
        .expect("player holds priority");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }), "got {status:?}");

    engine
        .submit_payment(PaymentPlan { payments: vec![CostPayment::Sacrifice(fodder)] })
        .expect(
            "p0 (fodder's effective controller via GainControl, even though p1 owns it) should be \
             able to sacrifice it to pay a 'sacrifice a creature you control' cost",
        );
}
