//! CR 510: `AssignCombatDamage` collapses assigning (CR 510.1) and dealing (CR 510.2) into one action.
//! Only an attacker with two or more blockers needs an `assignments` entry.

use std::collections::HashMap;

use stackfish_engine::engine::ActionError;
use stackfish_engine::commands::Rejected;
use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, minimal_card_definition, two_player_state as base_two_player_state};

fn creature_definition(name: &str, power: i32, toughness: i32) -> CardDefinition {
    CardDefinition {
        power: Some(PTValue::Fixed(power)),
        toughness: Some(PTValue::Fixed(toughness)),
        ..minimal_card_definition(1, name, name, ManaCost { symbols: Vec::new() }, CardTypeSet::CREATURE)
    }
}

fn permanent(id: ObjectId, definition: CardDefinition, owner: PlayerId) -> GameObject {
    // Attackers and blockers are mid-combat, so tapped state does not matter.
    GameObject { tapped: true, ..card_object(id, definition, owner, ZoneKind::Battlefield) }
}

fn put_on_battlefield(state: &mut GameState, object: GameObject) {
    let id = object.object_id;
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(id);
    state.objects.insert(id, object);
}

fn combat_damage_state(combat: CombatState) -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        phase: Phase::Combat,
        step: Step::CombatDamage,
        combat: Some(combat),
        last_active_turn_number: HashMap::from([(p0, 1), (p1, 0)]),
        ..base_two_player_state(vec![Zone::new(ZoneKind::Battlefield, None)])
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

fn try_assign_combat_damage(
    engine: &mut Engine,
    player: PlayerId,
    assignments: Vec<(ObjectId, Vec<(ObjectId, u32)>)>,
) -> Result<StepStatus, ActionError> {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::AssignCombatDamage { assignments },
        })
        ?;
    Ok(drain(engine))
}

#[test]
fn unblocked_attacker_deals_damage_to_original_target() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let mut state =
        combat_damage_state(CombatState { attackers: vec![(attacker, Target::Player(p1))], blocks: Vec::new(), declared_defenders: vec![p1] , bands: Vec::new(), effect_blocked: Vec::new(), damage_assigned_this_step: false, pending_damage_splits: std::collections::HashMap::new() });
    put_on_battlefield(&mut state, permanent(attacker, creature_definition("Bear", 3, 3), p0));

    let mut engine = Engine::new(state);
    let status = assign_combat_damage(&mut engine, p0, Vec::new());
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));

    let p1_life = engine.state().players.iter().find(|p| p.player_id == p1).unwrap().life;
    assert_eq!(p1_life, 17, "510.1b: unblocked attacker's full power hits the original target");

    let kinds: Vec<_> = engine.state().ledger.iter().map(|e| &e.kind).collect();
    assert!(kinds.iter().any(|k| matches!(
        k,
        GameEventKind::DamageDealt { source, target: Target::Player(t), amount, .. } if *source == attacker && *t == p1 && *amount == 3
    )));
}

#[test]
fn single_blocker_deals_damage_both_directions() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let blocker = ObjectId(1);
    let mut state = combat_damage_state(CombatState {
        attackers: vec![(attacker, Target::Player(p1))],
        blocks: vec![(blocker, attacker)],
        declared_defenders: vec![p1],
        bands: Vec::new(), effect_blocked: Vec::new(), damage_assigned_this_step: false, pending_damage_splits: std::collections::HashMap::new(),
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
        declared_defenders: vec![p1],
        bands: Vec::new(), effect_blocked: Vec::new(), damage_assigned_this_step: false, pending_damage_splits: std::collections::HashMap::new(),
    });
    put_on_battlefield(&mut state, permanent(attacker, creature_definition("Bear", 3, 2), p0));
    put_on_battlefield(&mut state, permanent(blocker, creature_definition("Wall", 2, 2), p1));

    let mut engine = Engine::new(state);
    assign_combat_damage(&mut engine, p0, Vec::new());

    assert!(engine.state().object(attacker).is_none(), "704.5g: lethal damage destroys the attacker");
    assert!(engine.state().object(blocker).is_none(), "704.5g: lethal damage destroys the blocker");
}

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
        declared_defenders: vec![p1],
        bands: Vec::new(), effect_blocked: Vec::new(), damage_assigned_this_step: false, pending_damage_splits: std::collections::HashMap::new(),
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
        declared_defenders: vec![p1],
        bands: Vec::new(), effect_blocked: Vec::new(), damage_assigned_this_step: false, pending_damage_splits: std::collections::HashMap::new(),
    });
    put_on_battlefield(&mut state, permanent(attacker, creature_definition("Bear", 5, 10), p0));
    put_on_battlefield(&mut state, permanent(blocker_a, creature_definition("Wall1", 1, 10), p1));
    put_on_battlefield(&mut state, permanent(blocker_b, creature_definition("Wall2", 1, 10), p1));

    let mut engine = Engine::new(state);
    // 3 + 1 = 4, not the attacker's power of 5.
    let err = try_assign_combat_damage(&mut engine, p0, vec![(attacker, vec![(blocker_a, 3), (blocker_b, 1)])])
        .unwrap_err();
    assert!(matches!(err, ActionError::Rejected(Rejected::IllegalAction(_))));

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
        declared_defenders: vec![p1],
        bands: Vec::new(), effect_blocked: Vec::new(), damage_assigned_this_step: false, pending_damage_splits: std::collections::HashMap::new(),
    });
    put_on_battlefield(&mut state, permanent(attacker, creature_definition("Bear", 5, 10), p0));
    put_on_battlefield(&mut state, permanent(blocker_a, creature_definition("Wall1", 1, 10), p1));
    put_on_battlefield(&mut state, permanent(blocker_b, creature_definition("Wall2", 1, 10), p1));

    let mut engine = Engine::new(state);
    // No assignments entry at all for the double-blocked attacker.
    let err = try_assign_combat_damage(&mut engine, p0, Vec::new()).unwrap_err();
    assert!(matches!(err, ActionError::Rejected(Rejected::IllegalAction(_))));
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
        declared_defenders: vec![p1],
        bands: Vec::new(), effect_blocked: Vec::new(), damage_assigned_this_step: false, pending_damage_splits: std::collections::HashMap::new(),
    });
    put_on_battlefield(&mut state, permanent(attacker, creature_definition("Bear", 3, 5), p0));
    put_on_battlefield(&mut state, permanent(blocker, creature_definition("Wall", 2, 5), p1));

    let mut engine = Engine::new(state);
    // One blocker leaves no choice, so an assignments entry is disallowed and not just redundant.
    let err = try_assign_combat_damage(&mut engine, p0, vec![(attacker, vec![(blocker, 3)])]).unwrap_err();
    assert!(matches!(err, ActionError::Rejected(Rejected::IllegalAction(_))));
}

#[test]
fn zero_power_attacker_assigns_no_damage() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let mut state =
        combat_damage_state(CombatState { attackers: vec![(attacker, Target::Player(p1))], blocks: Vec::new(), declared_defenders: vec![p1] , bands: Vec::new(), effect_blocked: Vec::new(), damage_assigned_this_step: false, pending_damage_splits: std::collections::HashMap::new() });
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

fn pass_both(engine: &mut Engine, p0: PlayerId, p1: PlayerId) -> StepStatus {
    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine);
    engine.submit_action(PlayerAction { player: p1, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine)
}

#[test]
fn unblocked_attacker_deals_damage_with_no_assign_combat_damage_submission_at_all() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let mut state = combat_damage_state(CombatState {
        attackers: vec![(attacker, Target::Player(p1))],
        blocks: Vec::new(),
        declared_defenders: vec![p1],
        bands: Vec::new(),
        effect_blocked: Vec::new(),
        damage_assigned_this_step: false, pending_damage_splits: std::collections::HashMap::new(),
    });
    state.step = Step::DeclareBlockers;
    put_on_battlefield(&mut state, permanent(attacker, creature_definition("Bear", 3, 3), p0));

    let mut engine = Engine::new(state);
    let status = pass_both(&mut engine, p0, p1);
    assert!(
        matches!(status, StepStatus::AwaitingAction { player } if player == p0),
        "priority reopens normally on Step::CombatDamage afterward - only the assignment itself is automatic"
    );
    assert_eq!(engine.state().step, Step::CombatDamage);

    let p1_life = engine.state().players.iter().find(|p| p.player_id == p1).unwrap().life;
    assert_eq!(p1_life, 17, "damage already landed from just passing priority, no AssignCombatDamage ever submitted");
    assert!(engine.state().combat.as_ref().unwrap().damage_assigned_this_step);
}

#[test]
fn single_non_trample_blocker_also_deals_damage_automatically() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let blocker = ObjectId(1);
    let mut state = combat_damage_state(CombatState {
        attackers: vec![(attacker, Target::Player(p1))],
        blocks: vec![(blocker, attacker)],
        declared_defenders: vec![p1],
        bands: Vec::new(),
        effect_blocked: Vec::new(),
        damage_assigned_this_step: false, pending_damage_splits: std::collections::HashMap::new(),
    });
    state.step = Step::DeclareBlockers;
    put_on_battlefield(&mut state, permanent(attacker, creature_definition("Bear", 3, 5), p0));
    put_on_battlefield(&mut state, permanent(blocker, creature_definition("Wall", 2, 5), p1));

    let mut engine = Engine::new(state);
    pass_both(&mut engine, p0, p1);

    assert_eq!(engine.state().object(attacker).unwrap().damage_marked, 2, "510.1d, dealt automatically");
    assert_eq!(engine.state().object(blocker).unwrap().damage_marked, 3, "510.1c, dealt automatically");
}

#[test]
fn double_blocked_attacker_still_waits_for_a_real_split_after_the_auto_fire_attempt() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let blocker_a = ObjectId(1);
    let blocker_b = ObjectId(2);
    let mut state = combat_damage_state(CombatState {
        attackers: vec![(attacker, Target::Player(p1))],
        blocks: vec![(blocker_a, attacker), (blocker_b, attacker)],
        declared_defenders: vec![p1],
        bands: Vec::new(),
        effect_blocked: Vec::new(),
        damage_assigned_this_step: false, pending_damage_splits: std::collections::HashMap::new(),
    });
    state.step = Step::DeclareBlockers;
    put_on_battlefield(&mut state, permanent(attacker, creature_definition("Bear", 5, 10), p0));
    put_on_battlefield(&mut state, permanent(blocker_a, creature_definition("Wall1", 1, 10), p1));
    put_on_battlefield(&mut state, permanent(blocker_b, creature_definition("Wall2", 1, 10), p1));

    let mut engine = Engine::new(state);
    let status = pass_both(&mut engine, p0, p1);
    assert!(matches!(status, StepStatus::AwaitingAction { .. }), "a real split still needs a real submission");
    assert_eq!(engine.state().object(attacker).unwrap().damage_marked, 0, "auto-fire correctly declined - nothing dealt yet");
    assert!(!engine.state().combat.as_ref().unwrap().damage_assigned_this_step);

    assign_combat_damage(&mut engine, p0, vec![(attacker, vec![(blocker_a, 3), (blocker_b, 2)])]);
    assert_eq!(engine.state().object(blocker_a).unwrap().damage_marked, 3);
    assert_eq!(engine.state().object(blocker_b).unwrap().damage_marked, 2);
}

#[test]
fn redundant_explicit_submission_after_auto_fire_is_a_harmless_noop() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let mut state = combat_damage_state(CombatState {
        attackers: vec![(attacker, Target::Player(p1))],
        blocks: Vec::new(),
        declared_defenders: vec![p1],
        bands: Vec::new(),
        effect_blocked: Vec::new(),
        damage_assigned_this_step: false, pending_damage_splits: std::collections::HashMap::new(),
    });
    state.step = Step::DeclareBlockers;
    put_on_battlefield(&mut state, permanent(attacker, creature_definition("Bear", 3, 3), p0));

    let mut engine = Engine::new(state);
    pass_both(&mut engine, p0, p1);
    let p1_life_after_auto_fire = engine.state().players.iter().find(|p| p.player_id == p1).unwrap().life;
    assert_eq!(p1_life_after_auto_fire, 17);

    // A zero-split submission stays legal and inert, and must not double-deal damage or error.
    assign_combat_damage(&mut engine, p0, Vec::new());
    let p1_life_after_redundant_submission =
        engine.state().players.iter().find(|p| p.player_id == p1).unwrap().life;
    assert_eq!(p1_life_after_redundant_submission, 17, "no second DamageDealt - damage was already assigned this step");
}

// CR 514.2
#[test]
fn damage_marked_is_cleared_entering_cleanup() {
    let p0 = PlayerId(0);
    let creature = ObjectId(0);
    let mut state = combat_damage_state(CombatState { attackers: Vec::new(), blocks: Vec::new(), declared_defenders: Vec::new() , bands: Vec::new(), effect_blocked: Vec::new(), damage_assigned_this_step: false, pending_damage_splits: std::collections::HashMap::new() });
    // Non-lethal marked damage survives to Cleanup, which isolates the reset from CR 704.5g.
    let mut object = permanent(creature, creature_definition("Bear", 3, 5), p0);
    object.damage_marked = 1;
    put_on_battlefield(&mut state, object);
    state.phase = Phase::Ending;
    state.step = Step::End;

    let mut engine = Engine::new(state);
    // Both players pass at End step, and Cleanup runs without a priority window (CR 514.3).
    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(&mut engine);
    engine.submit_action(PlayerAction { player: PlayerId(1), kind: PlayerActionKind::PassPriority }).unwrap();
    drain(&mut engine);
    // Cleanup cleared the damage when advance_turn_step set the step, then auto-advanced.
    assert_eq!(engine.state().object(creature).unwrap().damage_marked, 0, "514.2: marked damage is removed entering cleanup");
}

#[test]
fn lose_life_subtracts_from_life_total() {
    let mut player = PlayerState {
        player_id: PlayerId(0),
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
    };
    player.lose_life(7);
    assert_eq!(player.life, 13);
    // CR 119: life is not clamped to 0, since negative life matters to CR 704.5a.
    player.lose_life(20);
    assert_eq!(player.life, -7);
}
