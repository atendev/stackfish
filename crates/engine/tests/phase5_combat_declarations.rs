//! Phase 5 vertical slice: baseline `DeclareAttackers`/`DeclareBlockers`
//! (CR 508.1/509.1), plus the CR 302.6/602.5a summoning-sickness check
//! folded in (gates attacking here; `phase5_activate_ability_mana.rs`
//! covers the tap-cost-ability side of the same check). No combat damage
//! (CR 510), no triggered abilities beyond the `AttackerDeclared`/
//! `BlockerDeclared` events themselves, no costs-to-attack/block, no
//! restrictions (continuous-effects query gap, same as `is_sorcery_speed`'s
//! existing TODO) - declarations only.

use std::collections::{HashMap, VecDeque};

use stackfish_engine::commands::CommandError as EngineCommandError;
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
        counters: CounterSet::new(),
        attachments: Vec::new(),
        attached_to: None,
        timestamp: SequenceNo(0),
        controlled_since_turn,
        visibility: VisibilityPolicy { public: true, visible_to: Vec::new() },
    }
}

/// Two players, `p0` active and attacking, `p1` defending, both turns
/// already seeded into `last_active_turn_number` (turn 1 for `p0`, turn 0
/// for `p1` - a never-active defending player still needs an entry, or
/// every one of their creatures would look summoning sick by the `None`
/// branch's conservative default). `Phase::Combat`/`Step::DeclareAttackers`
/// so `DeclareAttackers` is immediately legal.
fn combat_state() -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        players: vec![
            PlayerState {
                player_id: p0,
                life: 20,
                mana_pool: ManaPool { white: 0, blue: 0, black: 0, red: 0, green: 0, colorless: 0 },
                counters: CounterSet::new(),
                emblems: Vec::new(),
                commander_damage_taken: Vec::new(),
                eliminated: false,
            },
            PlayerState {
                player_id: p1,
                life: 20,
                mana_pool: ManaPool { white: 0, blue: 0, black: 0, red: 0, green: 0, colorless: 0 },
                counters: CounterSet::new(),
                emblems: Vec::new(),
                commander_damage_taken: Vec::new(),
                eliminated: false,
            },
        ],
        turn_order: vec![p0, p1],
        active_player: p0,
        turn_number: 1,
        phase: Phase::Combat,
        step: Step::DeclareAttackers,
        priority_player: p0,
        consecutive_passes: 0,
        lands_played_this_turn: HashMap::new(),
        combat: None,
        last_active_turn_number: HashMap::from([(p0, 1), (p1, 0)]),
        objects: HashMap::new(),
        zones: vec![Zone { kind: ZoneKind::Battlefield, owner: None, contents: VecDeque::new() }],
        stack: Vec::new(),
        continuous_effects: Vec::new(),
        replacement_effects: Vec::new(),
        abilities: Vec::new(),
        ledger: Vec::new(),
        rng: RngState { seed: 1, draws_consumed: 0 },
        next_event_id: 0,
        next_sequence_no: 0,
        next_object_id: 1000,
    }
}

fn put_on_battlefield(state: &mut GameState, object: GameObject) {
    let id = object.object_id;
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(id);
    state.objects.insert(id, object);
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

fn declare_blockers(
    engine: &mut Engine,
    player: PlayerId,
    blocks: Vec<(ObjectId, ObjectId)>,
) -> StepStatus {
    engine
        .submit_action(PlayerAction { player, kind: PlayerActionKind::DeclareBlockers { blocks } })
        .expect("player holds priority");
    drain(engine)
}

/// Drives the real `AdvanceTurnStep` pipeline (both players passing with
/// the empty stack combat declarations leave behind) until `state.step`
/// reaches `DeclareBlockers` - there's no direct state-mutation shortcut
/// for this from outside `engine.rs` (`Engine::state()` is read-only), and
/// going through the real pipeline is also more honest: it's the same path
/// a live game takes from "attackers declared" to "blockers step opens."
fn pass_to_declare_blockers(engine: &mut Engine) {
    for _ in 0..10 {
        if engine.state().step == Step::DeclareBlockers {
            return;
        }
        let active = engine.state().priority_player;
        engine
            .submit_action(PlayerAction { player: active, kind: PlayerActionKind::PassPriority })
            .expect("priority window");
        drain(engine);
    }
    panic!("never reached DeclareBlockers - state.step = {:?}", engine.state().step);
}

// ---------------------------------------------------------------------
// DeclareAttackers
// ---------------------------------------------------------------------

#[test]
fn declare_attackers_taps_and_records_combat_state() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let mut state = combat_state();
    put_on_battlefield(&mut state, permanent(attacker, creature_definition("Bear"), p0, 0));

    let mut engine = Engine::new(state);
    let status = declare_attackers(&mut engine, p0, vec![(attacker, Target::Player(p1))]);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));

    assert!(engine.state().object(attacker).unwrap().tapped, "508.1f: attacking taps it");
    let combat = engine.state().combat.as_ref().expect("attackers recorded");
    assert_eq!(combat.attackers, vec![(attacker, Target::Player(p1))]);
    assert!(!combat.blockers_declared);
    assert_eq!(engine.state().consecutive_passes, 0, "508: declaring breaks the pass chain");

    let kinds: Vec<_> = engine.state().ledger.iter().map(|e| &e.kind).collect();
    assert!(matches!(kinds[0], GameEventKind::Tapped { object } if *object == attacker));
    assert!(matches!(
        kinds[1],
        GameEventKind::AttackerDeclared { attacker: a, target: Target::Player(t) } if *a == attacker && *t == p1
    ));
}

#[test]
fn declare_attackers_allows_zero_attackers() {
    let p0 = PlayerId(0);
    let state = combat_state();
    let mut engine = Engine::new(state);
    let status = declare_attackers(&mut engine, p0, Vec::new());
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert!(engine.state().combat.is_some(), "508.1: declaring zero attackers still declares");
}

#[test]
fn declare_attackers_rejects_non_active_player() {
    let p1 = PlayerId(1);
    let state = combat_state();
    let mut engine = Engine::new(state);
    let status = declare_attackers(&mut engine, p1, Vec::new());
    assert!(matches!(status, StepStatus::Error(EngineCommandError::IllegalAction(_))));
}

#[test]
fn declare_attackers_rejects_wrong_step() {
    let p0 = PlayerId(0);
    let mut state = combat_state();
    state.step = Step::PreCombatMain;
    state.phase = Phase::Main;
    let mut engine = Engine::new(state);
    let status = declare_attackers(&mut engine, p0, Vec::new());
    assert!(matches!(status, StepStatus::Error(EngineCommandError::IllegalAction(_))));
}

#[test]
fn declare_attackers_rejects_redeclaration() {
    let p0 = PlayerId(0);
    let state = combat_state();
    let mut engine = Engine::new(state);
    declare_attackers(&mut engine, p0, Vec::new());
    let status = declare_attackers(&mut engine, p0, Vec::new());
    assert!(matches!(status, StepStatus::Error(EngineCommandError::IllegalAction(_))));
}

#[test]
fn declare_attackers_rejects_uncontrolled_creature() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let mut state = combat_state();
    put_on_battlefield(&mut state, permanent(attacker, creature_definition("Bear"), p1, 0));

    let mut engine = Engine::new(state);
    let status = declare_attackers(&mut engine, p0, vec![(attacker, Target::Player(p1))]);
    assert!(matches!(status, StepStatus::Error(EngineCommandError::IllegalAction(_))));
    assert!(!engine.state().object(attacker).unwrap().tapped, "no partial mutation");
    assert!(engine.state().combat.is_none());
}

#[test]
fn declare_attackers_rejects_non_creature() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let mut definition = creature_definition("Rock");
    definition.card_types = CardTypeSet::ARTIFACT;
    let mut state = combat_state();
    put_on_battlefield(&mut state, permanent(attacker, definition, p0, 0));

    let mut engine = Engine::new(state);
    let status = declare_attackers(&mut engine, p0, vec![(attacker, Target::Player(p1))]);
    assert!(matches!(status, StepStatus::Error(EngineCommandError::IllegalAction(_))));
}

#[test]
fn declare_attackers_rejects_already_tapped_creature() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let mut object = permanent(attacker, creature_definition("Bear"), p0, 0);
    object.tapped = true;
    let mut state = combat_state();
    put_on_battlefield(&mut state, object);

    let mut engine = Engine::new(state);
    let status = declare_attackers(&mut engine, p0, vec![(attacker, Target::Player(p1))]);
    assert!(matches!(status, StepStatus::Error(EngineCommandError::IllegalAction(_))));
}

#[test]
fn declare_attackers_rejects_summoning_sick_creature() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    // Controlled since turn 1 (the current turn) - combat_state seeds
    // p0's most recent turn start at 1 too, so this hasn't been controlled
    // since the turn *began*. Sick (CR 508.1a/302.6).
    let mut state = combat_state();
    put_on_battlefield(&mut state, permanent(attacker, creature_definition("Bear"), p0, 1));

    let mut engine = Engine::new(state);
    let status = declare_attackers(&mut engine, p0, vec![(attacker, Target::Player(p1))]);
    assert!(matches!(status, StepStatus::Error(EngineCommandError::IllegalAction(_))));
    assert!(!engine.state().object(attacker).unwrap().tapped, "no partial mutation");
}

#[test]
fn declare_attackers_allows_creature_controlled_since_before_turn_began() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    // controlled_since_turn 0 < last_active_turn_number[p0] (1) - not sick.
    let mut state = combat_state();
    put_on_battlefield(&mut state, permanent(attacker, creature_definition("Bear"), p0, 0));

    let mut engine = Engine::new(state);
    let status = declare_attackers(&mut engine, p0, vec![(attacker, Target::Player(p1))]);
    assert!(matches!(status, StepStatus::AwaitingAction { .. }));
    assert!(engine.state().object(attacker).unwrap().tapped);
}

#[test]
fn declare_attackers_rejects_invalid_target() {
    let p0 = PlayerId(0);
    let attacker = ObjectId(0);
    let mut state = combat_state();
    put_on_battlefield(&mut state, permanent(attacker, creature_definition("Bear"), p0, 0));

    let mut engine = Engine::new(state);
    // Targeting self is illegal - no opponent named p0.
    let status = declare_attackers(&mut engine, p0, vec![(attacker, Target::Player(p0))]);
    assert!(matches!(status, StepStatus::Error(EngineCommandError::IllegalAction(_))));
}

#[test]
fn declare_attackers_is_atomic_across_a_mixed_legal_illegal_batch() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let legal = ObjectId(0);
    let already_tapped = ObjectId(1);
    let mut state = combat_state();
    put_on_battlefield(&mut state, permanent(legal, creature_definition("Bear"), p0, 0));
    let mut tapped_object = permanent(already_tapped, creature_definition("Wolf"), p0, 0);
    tapped_object.tapped = true;
    put_on_battlefield(&mut state, tapped_object);

    let mut engine = Engine::new(state);
    let status = declare_attackers(
        &mut engine,
        p0,
        vec![(legal, Target::Player(p1)), (already_tapped, Target::Player(p1))],
    );
    assert!(matches!(status, StepStatus::Error(EngineCommandError::IllegalAction(_))));
    assert!(!engine.state().object(legal).unwrap().tapped, "508.1: illegal batch taps nobody");
    assert!(engine.state().combat.is_none());
}

// ---------------------------------------------------------------------
// DeclareBlockers
// ---------------------------------------------------------------------

#[test]
fn declare_blockers_records_blocks() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let blocker = ObjectId(1);
    let mut state = combat_state();
    put_on_battlefield(&mut state, permanent(attacker, creature_definition("Bear"), p0, 0));
    put_on_battlefield(&mut state, permanent(blocker, creature_definition("Wall"), p1, 0));

    let mut engine = Engine::new(state);
    declare_attackers(&mut engine, p0, vec![(attacker, Target::Player(p1))]);
    pass_to_declare_blockers(&mut engine);
    let status = declare_blockers(&mut engine, p1, vec![(blocker, attacker)]);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));

    let combat = engine.state().combat.as_ref().unwrap();
    assert_eq!(combat.blocks, vec![(blocker, attacker)]);
    assert!(combat.blockers_declared);
    assert!(!engine.state().object(blocker).unwrap().tapped, "509: blocking doesn't tap");

    let kinds: Vec<_> = engine.state().ledger.iter().map(|e| &e.kind).collect();
    assert!(kinds.iter().any(|k| matches!(
        k,
        GameEventKind::BlockerDeclared { blocker: b, attacker: a } if *b == blocker && *a == attacker
    )));
}

#[test]
fn declare_blockers_allows_multiple_blockers_on_same_attacker() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let blocker_a = ObjectId(1);
    let blocker_b = ObjectId(2);
    let mut state = combat_state();
    put_on_battlefield(&mut state, permanent(attacker, creature_definition("Bear"), p0, 0));
    put_on_battlefield(&mut state, permanent(blocker_a, creature_definition("Wall"), p1, 0));
    put_on_battlefield(&mut state, permanent(blocker_b, creature_definition("Wall2"), p1, 0));

    let mut engine = Engine::new(state);
    declare_attackers(&mut engine, p0, vec![(attacker, Target::Player(p1))]);
    pass_to_declare_blockers(&mut engine);
    let status = declare_blockers(&mut engine, p1, vec![(blocker_a, attacker), (blocker_b, attacker)]);
    assert!(matches!(status, StepStatus::AwaitingAction { .. }));
    assert_eq!(engine.state().combat.as_ref().unwrap().blocks.len(), 2);
}

#[test]
fn declare_blockers_rejects_non_defending_player() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let mut state = combat_state();
    put_on_battlefield(&mut state, permanent(attacker, creature_definition("Bear"), p0, 0));

    let mut engine = Engine::new(state);
    declare_attackers(&mut engine, p0, vec![(attacker, Target::Player(p1))]);
    let status = declare_blockers(&mut engine, p0, Vec::new());
    assert!(matches!(status, StepStatus::Error(EngineCommandError::IllegalAction(_))));
}

#[test]
fn declare_blockers_rejects_wrong_step() {
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let blocker = ObjectId(1);
    // Attackers already recorded (state.combat is Some), but the step
    // hasn't advanced to DeclareBlockers yet - isolates the step check from
    // the "no attackers declared" check covered separately below.
    let mut state = combat_state();
    state.combat = Some(CombatState {
        attackers: vec![(attacker, Target::Player(p1))],
        blocks: Vec::new(),
        blockers_declared: false,
    });
    put_on_battlefield(&mut state, permanent(blocker, creature_definition("Wall"), p1, 0));

    let mut engine = Engine::new(state);
    let status = declare_blockers(&mut engine, p1, vec![(blocker, attacker)]);
    assert!(matches!(status, StepStatus::Error(EngineCommandError::IllegalAction(_))));
}

#[test]
fn declare_blockers_rejects_redeclaration() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let blocker = ObjectId(1);
    let mut state = combat_state();
    put_on_battlefield(&mut state, permanent(attacker, creature_definition("Bear"), p0, 0));
    put_on_battlefield(&mut state, permanent(blocker, creature_definition("Wall"), p1, 0));

    let mut engine = Engine::new(state);
    declare_attackers(&mut engine, p0, vec![(attacker, Target::Player(p1))]);
    pass_to_declare_blockers(&mut engine);
    declare_blockers(&mut engine, p1, vec![(blocker, attacker)]);
    // Step/phase haven't changed between calls (only AdvanceTurnStep moves
    // those) - this isolates the `blockers_declared` redeclaration check.
    let status = declare_blockers(&mut engine, p1, Vec::new());
    assert!(matches!(status, StepStatus::Error(EngineCommandError::IllegalAction(_))));
}

#[test]
fn declare_blockers_rejects_uncontrolled_or_tapped_blocker() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let tapped_blocker = ObjectId(1);
    let mut state = combat_state();
    put_on_battlefield(&mut state, permanent(attacker, creature_definition("Bear"), p0, 0));
    let mut blocker_object = permanent(tapped_blocker, creature_definition("Wall"), p1, 0);
    blocker_object.tapped = true;
    put_on_battlefield(&mut state, blocker_object);

    let mut engine = Engine::new(state);
    declare_attackers(&mut engine, p0, vec![(attacker, Target::Player(p1))]);
    pass_to_declare_blockers(&mut engine);
    let status = declare_blockers(&mut engine, p1, vec![(tapped_blocker, attacker)]);
    assert!(matches!(status, StepStatus::Error(EngineCommandError::IllegalAction(_))));
}

#[test]
fn declare_blockers_rejects_unknown_attacker_reference() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let not_attacking = ObjectId(2);
    let blocker = ObjectId(1);
    let mut state = combat_state();
    put_on_battlefield(&mut state, permanent(attacker, creature_definition("Bear"), p0, 0));
    put_on_battlefield(&mut state, permanent(not_attacking, creature_definition("Bear2"), p0, 0));
    put_on_battlefield(&mut state, permanent(blocker, creature_definition("Wall"), p1, 0));

    let mut engine = Engine::new(state);
    declare_attackers(&mut engine, p0, vec![(attacker, Target::Player(p1))]);
    pass_to_declare_blockers(&mut engine);
    let status = declare_blockers(&mut engine, p1, vec![(blocker, not_attacking)]);
    assert!(matches!(status, StepStatus::Error(EngineCommandError::IllegalAction(_))));
}

#[test]
fn declare_blockers_rejects_duplicate_blocker_assignment() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker_a = ObjectId(0);
    let attacker_b = ObjectId(2);
    let blocker = ObjectId(1);
    let mut state = combat_state();
    put_on_battlefield(&mut state, permanent(attacker_a, creature_definition("Bear"), p0, 0));
    put_on_battlefield(&mut state, permanent(attacker_b, creature_definition("Bear2"), p0, 0));
    put_on_battlefield(&mut state, permanent(blocker, creature_definition("Wall"), p1, 0));

    let mut engine = Engine::new(state);
    declare_attackers(
        &mut engine,
        p0,
        vec![(attacker_a, Target::Player(p1)), (attacker_b, Target::Player(p1))],
    );
    pass_to_declare_blockers(&mut engine);
    let status = declare_blockers(&mut engine, p1, vec![(blocker, attacker_a), (blocker, attacker_b)]);
    assert!(matches!(status, StepStatus::Error(EngineCommandError::IllegalAction(_))));
}

#[test]
fn declare_blockers_rejects_no_attackers_declared_yet() {
    let p1 = PlayerId(1);
    let blocker = ObjectId(1);
    let mut state = combat_state();
    state.step = Step::DeclareBlockers;
    put_on_battlefield(&mut state, permanent(blocker, creature_definition("Wall"), p1, 0));

    let mut engine = Engine::new(state);
    let status = declare_blockers(&mut engine, p1, Vec::new());
    assert!(matches!(status, StepStatus::Error(EngineCommandError::IllegalAction(_))));
}

// ---------------------------------------------------------------------
// CombatState lifecycle
// ---------------------------------------------------------------------

#[test]
fn combat_state_clears_at_end_of_combat() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let mut state = combat_state();
    put_on_battlefield(&mut state, permanent(attacker, creature_definition("Bear"), p0, 0));

    let mut engine = Engine::new(state);
    declare_attackers(&mut engine, p0, vec![(attacker, Target::Player(p1))]);
    assert!(engine.state().combat.is_some());

    // Pass priority around until the all-pass empty-stack path advances
    // through DeclareBlockers/CombatDamage/EndOfCombat into PostCombatMain.
    // DeclareBlockers itself never got submitted (no PlayerAction has to
    // run there for the step to advance) - this purely exercises
    // AdvanceTurnStep's EndOfCombat clear.
    for _ in 0..20 {
        if engine.state().step == Step::PostCombatMain {
            break;
        }
        let active = engine.state().priority_player;
        engine
            .submit_action(PlayerAction { player: active, kind: PlayerActionKind::PassPriority })
            .expect("priority window");
        drain(&mut engine);
    }
    assert_eq!(engine.state().step, Step::PostCombatMain);
    assert!(engine.state().combat.is_none(), "506.4: combat status ends with the combat phase");
}
