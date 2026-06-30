//! Phase 5 vertical slice: combat damage (CR 510). `AssignCombatDamage`
//! collapses 510.1 (assign) and 510.2 (deal) into one turn-based action,
//! same shape as `DeclareAttackers`/`DeclareBlockers` - see
//! `command.rs`'s doc comment on the variant for why only attackers with
//! 2+ blockers ever need a real `assignments` entry. First/double strike
//! (702.7), deathtouch (702.2), damage prevention/replacement (CR 615/616),
//! and planeswalker/battle damage are all out of scope - see the TODOs left
//! in `commands.rs`.

use std::collections::{HashMap, VecDeque};

use stackfish_engine::commands::CommandError as EngineCommandError;
use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::*;

fn creature_definition(name: &str, power: i32, toughness: i32) -> CardDefinition {
    CardDefinition {
        definition_id: 1,
        name: name.to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::CREATURE,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: Some(PTValue::Fixed(power)),
        toughness: Some(PTValue::Fixed(toughness)),
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey(name.to_string()),
    }
}

fn permanent(id: ObjectId, definition: CardDefinition, owner: PlayerId) -> GameObject {
    GameObject {
        object_id: id,
        lineage: ObjectLineage { previous: None },
        definition,
        owner,
        controller: owner,
        zone: ZoneKind::Battlefield,
        tapped: true, // attackers/blockers are already mid-combat - tapped is irrelevant to this slice either way
        flipped: false,
        face_down: false,
        phased_out: false,
        damage_marked: 0,
        counters: CounterSet::new(),
        attachments: Vec::new(),
        attached_to: None,
        timestamp: SequenceNo(0),
        controlled_since_turn: 0,
        visibility: VisibilityPolicy { public: true, visible_to: Vec::new() },
    }
}

fn put_on_battlefield(state: &mut GameState, object: GameObject) {
    let id = object.object_id;
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(id);
    state.objects.insert(id, object);
}

/// Two players, `p0` active/attacking, `p1` defending,
/// `Phase::Combat`/`Step::CombatDamage` with `combat` already populated (as
/// if `DeclareAttackers`/`DeclareBlockers` already ran) - `AssignCombatDamage`
/// is immediately legal.
fn combat_damage_state(combat: CombatState) -> GameState {
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
        step: Step::CombatDamage,
        priority_player: p0,
        consecutive_passes: 0,
        lands_played_this_turn: HashMap::new(),
        combat: Some(combat),
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

fn drain(engine: &mut Engine) -> StepStatus {
    loop {
        let result = engine.step();
        match result.status {
            StepStatus::Progressed => continue,
            other => return other,
        }
    }
}

fn assign_combat_damage(
    engine: &mut Engine,
    player: PlayerId,
    assignments: Vec<(ObjectId, Vec<(ObjectId, u32)>)>,
) -> StepStatus {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::AssignCombatDamage { assignments },
        })
        .expect("turn-based action, no priority required");
    drain(engine)
}

// ---------------------------------------------------------------------
// Unblocked
// ---------------------------------------------------------------------

#[test]
fn unblocked_attacker_deals_damage_to_original_target() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let mut state =
        combat_damage_state(CombatState { attackers: vec![(attacker, Target::Player(p1))], blocks: Vec::new(), blockers_declared: true });
    put_on_battlefield(&mut state, permanent(attacker, creature_definition("Bear", 3, 3), p0));

    let mut engine = Engine::new(state);
    let status = assign_combat_damage(&mut engine, p0, Vec::new());
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));

    let p1_life = engine.state().players.iter().find(|p| p.player_id == p1).unwrap().life;
    assert_eq!(p1_life, 17, "510.1b: unblocked attacker's full power hits the original target");

    let kinds: Vec<_> = engine.state().ledger.iter().map(|e| &e.kind).collect();
    assert!(kinds.iter().any(|k| matches!(
        k,
        GameEventKind::DamageDealt { source, target: Target::Player(t), amount } if *source == attacker && *t == p1 && *amount == 3
    )));
}

// ---------------------------------------------------------------------
// Single blocker
// ---------------------------------------------------------------------

#[test]
fn single_blocker_deals_damage_both_directions() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let blocker = ObjectId(1);
    let mut state = combat_damage_state(CombatState {
        attackers: vec![(attacker, Target::Player(p1))],
        blocks: vec![(blocker, attacker)],
        blockers_declared: true,
    });
    put_on_battlefield(&mut state, permanent(attacker, creature_definition("Bear", 3, 5), p0));
    put_on_battlefield(&mut state, permanent(blocker, creature_definition("Wall", 2, 5), p1));

    let mut engine = Engine::new(state);
    let status = assign_combat_damage(&mut engine, p0, Vec::new());
    assert!(matches!(status, StepStatus::AwaitingAction { .. }));

    assert_eq!(engine.state().object(attacker).unwrap().damage_marked, 2, "510.1d: blocker's full power hits the attacker");
    assert_eq!(engine.state().object(blocker).unwrap().damage_marked, 3, "510.1c: sole blocker takes the attacker's full power");
    let p1_life = engine.state().players.iter().find(|p| p.player_id == p1).unwrap().life;
    assert_eq!(p1_life, 20, "blocked attacker never hits the defending player");
}

#[test]
fn single_blocker_lethal_both_ways_kills_both_creatures_same_step() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let blocker = ObjectId(1);
    let mut state = combat_damage_state(CombatState {
        attackers: vec![(attacker, Target::Player(p1))],
        blocks: vec![(blocker, attacker)],
        blockers_declared: true,
    });
    put_on_battlefield(&mut state, permanent(attacker, creature_definition("Bear", 3, 2), p0));
    put_on_battlefield(&mut state, permanent(blocker, creature_definition("Wall", 2, 2), p1));

    let mut engine = Engine::new(state);
    assign_combat_damage(&mut engine, p0, Vec::new());

    assert!(engine.state().object(attacker).is_none(), "704.5g: lethal damage destroys the attacker");
    assert!(engine.state().object(blocker).is_none(), "704.5g: lethal damage destroys the blocker");
}

// ---------------------------------------------------------------------
// Multi-blocked: real split
// ---------------------------------------------------------------------

#[test]
fn double_blocked_attacker_accepts_a_legal_split() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let blocker_a = ObjectId(1);
    let blocker_b = ObjectId(2);
    let mut state = combat_damage_state(CombatState {
        attackers: vec![(attacker, Target::Player(p1))],
        blocks: vec![(blocker_a, attacker), (blocker_b, attacker)],
        blockers_declared: true,
    });
    put_on_battlefield(&mut state, permanent(attacker, creature_definition("Bear", 5, 10), p0));
    put_on_battlefield(&mut state, permanent(blocker_a, creature_definition("Wall1", 1, 10), p1));
    put_on_battlefield(&mut state, permanent(blocker_b, creature_definition("Wall2", 1, 10), p1));

    let mut engine = Engine::new(state);
    let status =
        assign_combat_damage(&mut engine, p0, vec![(attacker, vec![(blocker_a, 3), (blocker_b, 2)])]);
    assert!(matches!(status, StepStatus::AwaitingAction { .. }));

    assert_eq!(engine.state().object(blocker_a).unwrap().damage_marked, 3);
    assert_eq!(engine.state().object(blocker_b).unwrap().damage_marked, 2);
    assert_eq!(
        engine.state().object(attacker).unwrap().damage_marked,
        2,
        "510.1d: every blocker deals its own full power to the attacker, independent of the split"
    );
}

#[test]
fn double_blocked_attacker_rejects_an_illegal_split_sum() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let blocker_a = ObjectId(1);
    let blocker_b = ObjectId(2);
    let mut state = combat_damage_state(CombatState {
        attackers: vec![(attacker, Target::Player(p1))],
        blocks: vec![(blocker_a, attacker), (blocker_b, attacker)],
        blockers_declared: true,
    });
    put_on_battlefield(&mut state, permanent(attacker, creature_definition("Bear", 5, 10), p0));
    put_on_battlefield(&mut state, permanent(blocker_a, creature_definition("Wall1", 1, 10), p1));
    put_on_battlefield(&mut state, permanent(blocker_b, creature_definition("Wall2", 1, 10), p1));

    let mut engine = Engine::new(state);
    // 3 + 1 = 4, not the attacker's power of 5.
    let status =
        assign_combat_damage(&mut engine, p0, vec![(attacker, vec![(blocker_a, 3), (blocker_b, 1)])]);
    assert!(matches!(status, StepStatus::Error(EngineCommandError::IllegalAction(_))));

    assert_eq!(engine.state().object(attacker).unwrap().damage_marked, 0, "510.1e: illegal split rejects the whole action");
    assert_eq!(engine.state().object(blocker_a).unwrap().damage_marked, 0);
    assert_eq!(engine.state().object(blocker_b).unwrap().damage_marked, 0);
}

#[test]
fn double_blocked_attacker_rejects_a_missing_split() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let blocker_a = ObjectId(1);
    let blocker_b = ObjectId(2);
    let mut state = combat_damage_state(CombatState {
        attackers: vec![(attacker, Target::Player(p1))],
        blocks: vec![(blocker_a, attacker), (blocker_b, attacker)],
        blockers_declared: true,
    });
    put_on_battlefield(&mut state, permanent(attacker, creature_definition("Bear", 5, 10), p0));
    put_on_battlefield(&mut state, permanent(blocker_a, creature_definition("Wall1", 1, 10), p1));
    put_on_battlefield(&mut state, permanent(blocker_b, creature_definition("Wall2", 1, 10), p1));

    let mut engine = Engine::new(state);
    // No assignments entry at all for the double-blocked attacker.
    let status = assign_combat_damage(&mut engine, p0, Vec::new());
    assert!(matches!(status, StepStatus::Error(EngineCommandError::IllegalAction(_))));
}

#[test]
fn single_blocked_attacker_rejects_a_redundant_split_entry() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let blocker = ObjectId(1);
    let mut state = combat_damage_state(CombatState {
        attackers: vec![(attacker, Target::Player(p1))],
        blocks: vec![(blocker, attacker)],
        blockers_declared: true,
    });
    put_on_battlefield(&mut state, permanent(attacker, creature_definition("Bear", 3, 5), p0));
    put_on_battlefield(&mut state, permanent(blocker, creature_definition("Wall", 2, 5), p1));

    let mut engine = Engine::new(state);
    // Only one blocker - there's no real choice here, so an assignments
    // entry is disallowed input, not just redundant.
    let status = assign_combat_damage(&mut engine, p0, vec![(attacker, vec![(blocker, 3)])]);
    assert!(matches!(status, StepStatus::Error(EngineCommandError::IllegalAction(_))));
}

// ---------------------------------------------------------------------
// Zero power
// ---------------------------------------------------------------------

#[test]
fn zero_power_attacker_assigns_no_damage() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let mut state =
        combat_damage_state(CombatState { attackers: vec![(attacker, Target::Player(p1))], blocks: Vec::new(), blockers_declared: true });
    put_on_battlefield(&mut state, permanent(attacker, creature_definition("Sprout", 0, 1), p0));

    let mut engine = Engine::new(state);
    let status = assign_combat_damage(&mut engine, p0, Vec::new());
    assert!(matches!(status, StepStatus::AwaitingAction { .. }));

    let p1_life = engine.state().players.iter().find(|p| p.player_id == p1).unwrap().life;
    assert_eq!(p1_life, 20, "510.1a: 0 power assigns no damage");
    assert!(
        !engine.state().ledger.iter().any(|e| matches!(e.kind, GameEventKind::DamageDealt { .. })),
        "no DamageDealt event for a 0-power attacker"
    );
}

// ---------------------------------------------------------------------
// CR 514.2 cleanup
// ---------------------------------------------------------------------

#[test]
fn damage_marked_is_cleared_entering_cleanup() {
    let p0 = PlayerId(0);
    let creature = ObjectId(0);
    let mut state = combat_damage_state(CombatState { attackers: Vec::new(), blocks: Vec::new(), blockers_declared: true });
    // Non-lethal marked damage (toughness 5, damage 1) - survives to Cleanup
    // so this test isolates the reset itself, not 704.5g.
    let mut object = permanent(creature, creature_definition("Bear", 3, 5), p0);
    object.damage_marked = 1;
    put_on_battlefield(&mut state, object);
    state.phase = Phase::Ending;
    state.step = Step::End;

    let mut engine = Engine::new(state);
    for _ in 0..10 {
        if engine.state().step == Step::Cleanup {
            break;
        }
        let active = engine.state().priority_player;
        engine
            .submit_action(PlayerAction { player: active, kind: PlayerActionKind::PassPriority })
            .expect("priority window");
        drain(&mut engine);
    }
    assert_eq!(engine.state().step, Step::Cleanup, "never reached Cleanup");
    assert_eq!(engine.state().object(creature).unwrap().damage_marked, 0, "514.2: marked damage is removed entering cleanup");
}

// ---------------------------------------------------------------------
// PlayerState::lose_life
// ---------------------------------------------------------------------

#[test]
fn lose_life_subtracts_from_life_total() {
    let mut player = PlayerState {
        player_id: PlayerId(0),
        life: 20,
        mana_pool: ManaPool { white: 0, blue: 0, black: 0, red: 0, green: 0, colorless: 0 },
        counters: CounterSet::new(),
        emblems: Vec::new(),
        commander_damage_taken: Vec::new(),
        eliminated: false,
    };
    player.lose_life(7);
    assert_eq!(player.life, 13);
    // CR 119: not clamped to 0 - negative life is meaningful for the CR
    // 704.5a SBA check, not special-cased away here.
    player.lose_life(20);
    assert_eq!(player.life, -7);
}
