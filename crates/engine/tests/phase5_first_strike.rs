//! Phase 5 vertical slice: first strike / double strike (CR 510.1a/702.7).
//! CR 510.1: if any attacker or blocker has first strike or double strike,
//! combat gets two damage steps (`Step::FirstStrikeDamage` then the regular
//! `Step::CombatDamage`) instead of one - only first-strike/double-strike
//! creatures assign/deal damage in the first step; everyone else (plus
//! double strikers again) does so in the regular step. When nobody has
//! either keyword, combat is unchanged: one step, same as before this slice.
//!
//! This also fixes a real pre-existing bug `AssignCombatDamage` carried:
//! it hard-errored (`InvalidTarget`) on any attacker/blocker that no longer
//! resolves in `state.objects` - harmless before first strike (rarely hit),
//! fatal once first strike is real (a first-striker routinely kills its
//! blocker in the first step, and that blocker's id genuinely won't resolve
//! by the time the regular step's `AssignCombatDamage` runs). See
//! `commands.rs`'s `participates_in_damage_step` and the filtering at the
//! top of `AssignCombatDamage`.

use std::collections::{HashMap, VecDeque};

use stackfish_engine::commands::CommandError as EngineCommandError;
use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::*;

fn creature_definition(name: &str, power: i32, toughness: i32, keywords: KeywordSet) -> CardDefinition {
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
        keywords,
        target_requirement: None,
        alternative_costs: Vec::new(),
        effect: None,
        modes: Vec::new(),
    }
}

fn permanent(id: ObjectId, definition: CardDefinition, owner: PlayerId, controlled_since_turn: u32) -> GameObject {
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
        controlled_since_turn,
        is_token: false,
        is_commander: false,
        visibility: VisibilityPolicy { public: true, visible_to: Vec::new() },
    }
}

fn put_on_battlefield(state: &mut GameState, object: GameObject) {
    let id = object.object_id;
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(id);
    state.objects.insert(id, object);
}

/// Two players, `p0` active/attacking, `p1` defending, `Phase::Combat`/
/// `Step::DeclareAttackers` - same shape as `phase5_combat_declarations.rs`'s
/// own `combat_state`, since this slice needs to drive the real
/// `DeclareAttackers` -> `DeclareBlockers` -> (`FirstStrikeDamage`?) ->
/// `CombatDamage` pipeline, not just construct a `CombatDamage`-step state
/// directly - the whole point under test is which step(s) get inserted.
fn combat_state() -> GameState {
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
                commander_casts: std::collections::HashMap::new(),
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
                commander_casts: std::collections::HashMap::new(),
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
        cost_modifiers: Vec::new(),
        hand_size_modifiers: Vec::new(),
        abilities: Vec::new(),
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

fn declare_attackers(
    engine: &mut Engine,
    player: PlayerId,
    attackers: Vec<(ObjectId, Target)>,
) -> StepStatus {
    engine
        .submit_action(PlayerAction { player, kind: PlayerActionKind::DeclareAttackers { attackers } })
        .expect("player holds priority");
    drain(engine)
}

fn declare_blockers(engine: &mut Engine, player: PlayerId, blocks: Vec<(ObjectId, ObjectId)>) -> StepStatus {
    engine
        .submit_action(PlayerAction { player, kind: PlayerActionKind::DeclareBlockers { blocks } })
        .expect("player holds priority");
    drain(engine)
}

fn assign_combat_damage(
    engine: &mut Engine,
    player: PlayerId,
    assignments: Vec<(ObjectId, Vec<(ObjectId, u32)>)>,
) -> StepStatus {
    engine
        .submit_action(PlayerAction { player, kind: PlayerActionKind::AssignCombatDamage { assignments } })
        .expect("turn-based action, no priority required");
    drain(engine)
}

/// Drives the real `AdvanceTurnStep` pipeline (both players passing with an
/// empty stack) until `state.step == target` - see
/// `phase5_combat_declarations.rs`'s `pass_to_declare_blockers` for why
/// there's no direct state-mutation shortcut for this.
fn pass_until_step(engine: &mut Engine, target: Step) {
    for _ in 0..20 {
        if engine.state().step == target {
            return;
        }
        let active = engine.state().priority_player;
        engine.submit_action(PlayerAction { player: active, kind: PlayerActionKind::PassPriority }).expect("priority window");
        drain(engine);
    }
    panic!("never reached {target:?} - state.step = {:?}", engine.state().step);
}

/// One full priority round (both players pass, in whichever order priority
/// currently sits) - exactly one `AdvanceTurnStep` transition given an empty
/// stack, so the step landed on afterward is the *immediate* next step, not
/// however many hops `pass_until_step` needed. Used to prove the
/// `FirstStrikeDamage` insertion happens (or doesn't) in a single hop from
/// `DeclareBlockers`, not to prove eventual arrival.
fn pass_one_round(engine: &mut Engine) {
    for _ in 0..2 {
        let active = engine.state().priority_player;
        engine.submit_action(PlayerAction { player: active, kind: PlayerActionKind::PassPriority }).expect("priority window");
        drain(engine);
    }
}

// ---------------------------------------------------------------------
// Step insertion (CR 510.1)
// ---------------------------------------------------------------------

#[test]
fn no_first_or_double_strike_skips_straight_to_combat_damage() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let mut state = combat_state();
    put_on_battlefield(&mut state, permanent(attacker, creature_definition("Bear", 2, 2, KeywordSet::empty()), p0, 0));

    let mut engine = Engine::new(state);
    declare_attackers(&mut engine, p0, vec![(attacker, Target::Player(p1))]);
    pass_until_step(&mut engine, Step::DeclareBlockers);
    declare_blockers(&mut engine, p1, Vec::new());

    pass_one_round(&mut engine);
    assert_eq!(
        engine.state().step,
        Step::CombatDamage,
        "510.1: nobody has first/double strike - the single AdvanceTurnStep hop from \
         DeclareBlockers must land directly on CombatDamage, never on FirstStrikeDamage"
    );
}

#[test]
fn first_strike_attacker_inserts_the_first_strike_damage_step() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let mut state = combat_state();
    put_on_battlefield(
        &mut state,
        permanent(attacker, creature_definition("Knight", 2, 2, KeywordSet::FIRST_STRIKE), p0, 0),
    );

    let mut engine = Engine::new(state);
    declare_attackers(&mut engine, p0, vec![(attacker, Target::Player(p1))]);
    pass_until_step(&mut engine, Step::DeclareBlockers);
    declare_blockers(&mut engine, p1, Vec::new());

    pass_one_round(&mut engine);
    assert_eq!(
        engine.state().step,
        Step::FirstStrikeDamage,
        "510.1a: an attacker with first strike opens a first-strike damage step before the regular one"
    );
}

// ---------------------------------------------------------------------
// Unblocked first strike / double strike
// ---------------------------------------------------------------------

#[test]
fn unblocked_first_strike_attacker_deals_damage_in_first_strike_step_only() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let mut state = combat_state();
    put_on_battlefield(
        &mut state,
        permanent(attacker, creature_definition("Knight", 3, 3, KeywordSet::FIRST_STRIKE), p0, 0),
    );

    let mut engine = Engine::new(state);
    declare_attackers(&mut engine, p0, vec![(attacker, Target::Player(p1))]);
    pass_until_step(&mut engine, Step::DeclareBlockers);
    declare_blockers(&mut engine, p1, Vec::new());
    pass_until_step(&mut engine, Step::FirstStrikeDamage);

    let status = assign_combat_damage(&mut engine, p0, Vec::new());
    assert!(matches!(status, StepStatus::AwaitingAction { .. }));
    let life_after_first_strike = engine.state().players.iter().find(|p| p.player_id == p1).unwrap().life;
    assert_eq!(life_after_first_strike, 17, "first-strike attacker deals its power in the first-strike step");

    pass_until_step(&mut engine, Step::CombatDamage);
    let status = assign_combat_damage(&mut engine, p0, Vec::new());
    assert!(matches!(status, StepStatus::AwaitingAction { .. }));
    let life_after_regular = engine.state().players.iter().find(|p| p.player_id == p1).unwrap().life;
    assert_eq!(
        life_after_regular, 17,
        "702.7: a plain first striker (no double strike) sits out the regular step entirely - no second hit"
    );
}

#[test]
fn double_strike_attacker_deals_damage_in_both_steps() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let mut state = combat_state();
    put_on_battlefield(
        &mut state,
        permanent(attacker, creature_definition("Champion", 3, 3, KeywordSet::DOUBLE_STRIKE), p0, 0),
    );

    let mut engine = Engine::new(state);
    declare_attackers(&mut engine, p0, vec![(attacker, Target::Player(p1))]);
    pass_until_step(&mut engine, Step::DeclareBlockers);
    declare_blockers(&mut engine, p1, Vec::new());
    pass_until_step(&mut engine, Step::FirstStrikeDamage);

    assign_combat_damage(&mut engine, p0, Vec::new());
    let life_after_first_strike = engine.state().players.iter().find(|p| p.player_id == p1).unwrap().life;
    assert_eq!(life_after_first_strike, 17, "702.4c: double strike deals damage in the first-strike step too");

    pass_until_step(&mut engine, Step::CombatDamage);
    assign_combat_damage(&mut engine, p0, Vec::new());
    let life_after_regular = engine.state().players.iter().find(|p| p.player_id == p1).unwrap().life;
    assert_eq!(life_after_regular, 14, "702.4c: and again in the regular step - total damage is 2x power");
}

// ---------------------------------------------------------------------
// Core interaction: first strike kills its blocker before it can strike back
// ---------------------------------------------------------------------

#[test]
fn first_strike_attacker_kills_blocker_before_it_deals_damage_back() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let blocker = ObjectId(1);
    let mut state = combat_state();
    put_on_battlefield(
        &mut state,
        permanent(attacker, creature_definition("Knight", 3, 3, KeywordSet::FIRST_STRIKE), p0, 0),
    );
    put_on_battlefield(&mut state, permanent(blocker, creature_definition("Wall", 2, 2, KeywordSet::empty()), p1, 0));

    let mut engine = Engine::new(state);
    declare_attackers(&mut engine, p0, vec![(attacker, Target::Player(p1))]);
    pass_until_step(&mut engine, Step::DeclareBlockers);
    declare_blockers(&mut engine, p1, vec![(blocker, attacker)]);
    pass_until_step(&mut engine, Step::FirstStrikeDamage);

    let status = assign_combat_damage(&mut engine, p0, Vec::new());
    assert!(matches!(status, StepStatus::AwaitingAction { .. }));
    assert!(
        engine.state().object(blocker).is_none(),
        "510.1a + 704.5g: the first-strike attacker's 3 damage is lethal to the 2-toughness blocker \
         in the first-strike step"
    );
    assert_eq!(
        engine.state().object(attacker).unwrap().damage_marked,
        0,
        "702.7: an ordinary blocker (no first/double strike) never gets to deal damage back - it's \
         already dead before the regular step, and wasn't eligible for this step even if it weren't"
    );

    // Regression coverage for the bug this slice fixes: before the filter,
    // AssignCombatDamage hard-errored (InvalidTarget) the instant any
    // attacker/blocker id in `combat` no longer resolved in state.objects -
    // fatal now that a first-strike kill routinely produces exactly that.
    pass_until_step(&mut engine, Step::CombatDamage);
    let status = assign_combat_damage(&mut engine, p0, Vec::new());
    assert!(
        matches!(status, StepStatus::AwaitingAction { .. }),
        "the regular step's AssignCombatDamage must silently skip the dead blocker, not \
         InvalidTarget-error the whole command: got {status:?}"
    );
    let p1_life = engine.state().players.iter().find(|p| p.player_id == p1).unwrap().life;
    assert_eq!(p1_life, 20, "the first-strike attacker (no double strike) doesn't strike again in the regular step");
}

// ---------------------------------------------------------------------
// Multi-blocked split requirement is scoped to the current step
// ---------------------------------------------------------------------

#[test]
fn multi_blocked_split_requirement_is_scoped_to_the_current_step() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    // attacker_b exists purely to force a FirstStrikeDamage step to be
    // inserted at all - attacker_a itself has no first/double strike, so
    // the interesting question is what AssignCombatDamage requires of
    // *its* split in each step.
    let attacker_a = ObjectId(0);
    let attacker_b = ObjectId(1);
    let blocker_1 = ObjectId(2);
    let blocker_2 = ObjectId(3);
    let mut state = combat_state();
    put_on_battlefield(&mut state, permanent(attacker_a, creature_definition("Bear", 5, 10, KeywordSet::empty()), p0, 0));
    put_on_battlefield(
        &mut state,
        permanent(attacker_b, creature_definition("Knight", 1, 1, KeywordSet::FIRST_STRIKE), p0, 0),
    );
    put_on_battlefield(&mut state, permanent(blocker_1, creature_definition("Wall1", 1, 10, KeywordSet::empty()), p1, 0));
    put_on_battlefield(&mut state, permanent(blocker_2, creature_definition("Wall2", 1, 10, KeywordSet::empty()), p1, 0));

    let mut engine = Engine::new(state);
    declare_attackers(
        &mut engine,
        p0,
        vec![(attacker_a, Target::Player(p1)), (attacker_b, Target::Player(p1))],
    );
    pass_until_step(&mut engine, Step::DeclareBlockers);
    declare_blockers(&mut engine, p1, vec![(blocker_1, attacker_a), (blocker_2, attacker_a)]);
    pass_until_step(&mut engine, Step::FirstStrikeDamage);

    // attacker_a doesn't participate in the first-strike step at all (no
    // first/double strike), so neither of its blockers needs a split here -
    // an empty assignments list must be accepted.
    let status = assign_combat_damage(&mut engine, p0, Vec::new());
    assert!(
        matches!(status, StepStatus::AwaitingAction { .. }),
        "no split required for a non-participating attacker during the first-strike step: got {status:?}"
    );
    assert_eq!(engine.state().object(blocker_1).unwrap().damage_marked, 0);
    assert_eq!(engine.state().object(blocker_2).unwrap().damage_marked, 0);

    pass_until_step(&mut engine, Step::CombatDamage);
    // Now attacker_a does participate (regular step, no keyword needed) and
    // is still blocked by both walls - the same split requirement
    // pre-existing tests (phase5_combat_damage.rs) already cover applies.
    let status = assign_combat_damage(&mut engine, p0, Vec::new());
    assert!(
        matches!(status, StepStatus::Error(EngineCommandError::IllegalAction(_))),
        "a split is required once attacker_a actually participates in the regular step: got {status:?}"
    );
}
