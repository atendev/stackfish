//! Phase 5 vertical slice: CR 702.22 Banding, narrowed to the plain form
//! (CR 702.22b/c's "bands with other [quality]" variant needs a parameter -
//! same shape gap `KeywordSet::BANDING`'s own doc comment documents for
//! Landwalk - and isn't modeled).
//!
//! Four real pieces:
//! - CR 508.1e/702.22c/d: `PlayerActionKind::DeclareBands` groups just-
//!   declared attackers into `CombatState::bands`, validated (>=1 banding
//!   member, <=1 non-banding member, everyone shares one attack target).
//! - CR 702.22h: `DeclareBlockers` propagates a direct block onto every
//!   other band member, bypassing every other evasion/protection check -
//!   even a member that couldn't legally be blocked on its own.
//! - CR 702.22j/k (`phase5/banding-split-actor`): who chooses a combat
//!   damage split is no longer hardcoded to the active player.
//!   `AssignCombatDamage` now computes a required chooser per split id -
//!   the object's own controller by default (510.1c/d), flipped to the
//!   *other* side when a `BANDING` creature is involved in that particular
//!   block (702.22j: attacker blocked by a banding creature -> defending
//!   player; 702.22k: blocker blocking a banding creature -> active
//!   player). 702.22k's blocker-forced-multi-block case was already
//!   correct before this slice, but only by accident: every band needs
//!   >=1 banding member, so the old blanket-active-player hardcode always
//!   happened to match.
//!
//! Deliberately unmodeled, named rather than guessed at: CR 702.22i
//! (effect-triggered blocking propagation - no such effect exists yet in
//! this engine), the "bands with other [quality]" variant (needs a
//! parameter, same gap `KeywordSet::BANDING`'s own doc comment names for
//! Landwalk), and a combat damage step that needs splits chosen by *two
//! different* players at once (e.g. a plain multi-block plus a separate
//! banding-blocked attacker, simultaneously) - `AssignCombatDamage` rejects
//! that combination outright rather than attempting a two-phase
//! per-player submission, see that action's own doc comment.

use std::collections::{HashMap, VecDeque};

use stackfish_engine::commands::CommandError as EngineCommandError;
use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::*;

fn creature_definition(id: u64, name: &str, power: i32, toughness: i32, keywords: KeywordSet) -> CardDefinition {
    CardDefinition {
        definition_id: id,
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

fn planeswalker_definition(id: u64, name: &str) -> CardDefinition {
    CardDefinition {
        definition_id: id,
        name: name.to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::PLANESWALKER,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: None,
        toughness: None,
        loyalty: Some(PTValue::Fixed(5)),
        defense: None,
        script_ref: ScriptReference::ByKey(name.to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        effect: None,
        modes: Vec::new(),
    }
}

fn permanent(id: ObjectId, definition: CardDefinition, owner: PlayerId) -> GameObject {
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
        controlled_since_turn: 0,
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

fn player_state(id: PlayerId) -> PlayerState {
    PlayerState {
        player_id: id,
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
    }
}

fn combat_state() -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        players: vec![player_state(p0), player_state(p1)],
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
        match engine.step().status {
            StepStatus::Progressed => continue,
            other => return other,
        }
    }
}

fn declare_attackers(engine: &mut Engine, player: PlayerId, attackers: Vec<(ObjectId, Target)>) -> StepStatus {
    engine
        .submit_action(PlayerAction { player, kind: PlayerActionKind::DeclareAttackers { attackers } })
        .expect("player holds priority");
    drain(engine)
}

fn declare_bands(engine: &mut Engine, player: PlayerId, bands: Vec<Vec<ObjectId>>) -> StepStatus {
    engine
        .submit_action(PlayerAction { player, kind: PlayerActionKind::DeclareBands { bands } })
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

fn pass_to_step(engine: &mut Engine, step: Step) {
    for _ in 0..20 {
        if engine.state().step == step {
            return;
        }
        let active = engine.state().priority_player;
        engine.submit_action(PlayerAction { player: active, kind: PlayerActionKind::PassPriority }).expect("priority window");
        drain(engine);
    }
    panic!("never reached {step:?} - state.step = {:?}", engine.state().step);
}

fn assert_illegal(status: StepStatus) {
    assert!(matches!(status, StepStatus::Error(EngineCommandError::IllegalAction(_))), "expected an illegal action, got {status:?}");
}

// ---------------------------------------------------------------------
// CR 702.22c/d: declaring bands
// ---------------------------------------------------------------------

#[test]
fn a_band_needs_at_least_one_banding_member() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let a = ObjectId(0);
    let b = ObjectId(1);

    let mut state = combat_state();
    put_on_battlefield(&mut state, permanent(a, creature_definition(1, "Bear", 2, 2, KeywordSet::empty()), p0));
    put_on_battlefield(&mut state, permanent(b, creature_definition(2, "Bear2", 2, 2, KeywordSet::empty()), p0));

    let mut engine = Engine::new(state);
    declare_attackers(&mut engine, p0, vec![(a, Target::Player(p1)), (b, Target::Player(p1))]);
    assert_illegal(declare_bands(&mut engine, p0, vec![vec![a, b]]));
}

#[test]
fn a_band_allows_at_most_one_non_banding_member() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let banding = ObjectId(0);
    let bear1 = ObjectId(1);
    let bear2 = ObjectId(2);

    let mut state = combat_state();
    put_on_battlefield(&mut state, permanent(banding, creature_definition(1, "Bander", 2, 2, KeywordSet::BANDING), p0));
    put_on_battlefield(&mut state, permanent(bear1, creature_definition(2, "Bear1", 2, 2, KeywordSet::empty()), p0));
    put_on_battlefield(&mut state, permanent(bear2, creature_definition(3, "Bear2", 2, 2, KeywordSet::empty()), p0));

    let mut engine = Engine::new(state);
    declare_attackers(
        &mut engine,
        p0,
        vec![(banding, Target::Player(p1)), (bear1, Target::Player(p1)), (bear2, Target::Player(p1))],
    );
    assert_illegal(declare_bands(&mut engine, p0, vec![vec![banding, bear1, bear2]]));
}

#[test]
fn every_band_member_must_attack_the_same_target() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let banding = ObjectId(0);
    let other = ObjectId(1);
    let pw = ObjectId(2);

    let mut state = combat_state();
    put_on_battlefield(&mut state, permanent(banding, creature_definition(1, "Bander", 2, 2, KeywordSet::BANDING), p0));
    put_on_battlefield(&mut state, permanent(other, creature_definition(2, "Bear", 2, 2, KeywordSet::empty()), p0));
    put_on_battlefield(&mut state, permanent(pw, planeswalker_definition(3, "Test Walker"), p1));

    let mut engine = Engine::new(state);
    declare_attackers(&mut engine, p0, vec![(banding, Target::Player(p1)), (other, Target::Object(pw))]);
    assert_illegal(declare_bands(&mut engine, p0, vec![vec![banding, other]]));
}

// ---------------------------------------------------------------------
// CR 702.22h: forced block propagation
// ---------------------------------------------------------------------

#[test]
fn a_directly_blocked_band_member_forces_the_same_block_onto_a_shadow_bandmate() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let banding = ObjectId(0);
    // Shadow: normally blockable only by another Shadow creature - CR
    // 702.22h overrides that entirely once its bandmate is blocked.
    let shadow_mate = ObjectId(1);
    let blocker = ObjectId(2);

    let mut state = combat_state();
    put_on_battlefield(&mut state, permanent(banding, creature_definition(1, "Bander", 2, 2, KeywordSet::BANDING), p0));
    put_on_battlefield(
        &mut state,
        permanent(shadow_mate, creature_definition(2, "Wraith", 2, 2, KeywordSet::BANDING | KeywordSet::SHADOW), p0),
    );
    put_on_battlefield(&mut state, permanent(blocker, creature_definition(3, "Bear", 2, 2, KeywordSet::empty()), p1));

    let mut engine = Engine::new(state);
    declare_attackers(&mut engine, p0, vec![(banding, Target::Player(p1)), (shadow_mate, Target::Player(p1))]);
    assert!(matches!(declare_bands(&mut engine, p0, vec![vec![banding, shadow_mate]]), StepStatus::AwaitingAction { .. }));
    pass_to_step(&mut engine, Step::DeclareBlockers);

    // Directly blocking the shadow creature would be illegal on its own -
    // only the non-shadow bandmate is submitted as a direct block.
    declare_blockers(&mut engine, p1, vec![(blocker, banding)]);

    let combat = engine.state().combat.as_ref().expect("still in combat");
    assert!(
        combat.blocks.contains(&(blocker, shadow_mate)),
        "expected the shadow bandmate to be forced-blocked by the same creature, got {:?}",
        combat.blocks
    );
    assert!(combat.blocks.contains(&(blocker, banding)));
}

// ---------------------------------------------------------------------
// CR 702.22k: a band-forced multi-block splits the blocker's damage
// ---------------------------------------------------------------------

#[test]
fn a_band_forced_blocker_must_split_its_damage_among_both_attackers() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let a = ObjectId(0);
    let b = ObjectId(1);
    let blocker = ObjectId(2);

    let mut state = combat_state();
    put_on_battlefield(&mut state, permanent(a, creature_definition(1, "Bander", 1, 5, KeywordSet::BANDING), p0));
    put_on_battlefield(&mut state, permanent(b, creature_definition(2, "Bandmate", 1, 5, KeywordSet::BANDING), p0));
    put_on_battlefield(&mut state, permanent(blocker, creature_definition(3, "Ogre", 4, 4, KeywordSet::empty()), p1));

    let mut engine = Engine::new(state);
    declare_attackers(&mut engine, p0, vec![(a, Target::Player(p1)), (b, Target::Player(p1))]);
    declare_bands(&mut engine, p0, vec![vec![a, b]]);
    pass_to_step(&mut engine, Step::DeclareBlockers);
    declare_blockers(&mut engine, p1, vec![(blocker, a)]);

    let combat = engine.state().combat.as_ref().unwrap();
    assert!(combat.blocks.contains(&(blocker, a)));
    assert!(combat.blocks.contains(&(blocker, b)));

    pass_to_step(&mut engine, Step::CombatDamage);

    // Omitting the forced blocker's split is illegal (510.1e atomicity,
    // now covering the blocker side too).
    assert_illegal(assign_combat_damage(&mut engine, p0, Vec::new()));

    let status = assign_combat_damage(&mut engine, p0, vec![(blocker, vec![(a, 3), (b, 1)])]);
    assert!(!matches!(status, StepStatus::Error(_)), "expected the split to be accepted, got {status:?}");
}

// ---------------------------------------------------------------------
// CR 702.22j: an attacker blocked by a banding creature has its split
// chosen by the defending player, not the active player.
// ---------------------------------------------------------------------

#[test]
fn attacker_double_blocked_by_a_banding_creature_needs_the_defending_players_choice() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let bander = ObjectId(1);
    let plain = ObjectId(2);

    let mut state = combat_state();
    put_on_battlefield(&mut state, permanent(attacker, creature_definition(1, "Attacker", 4, 4, KeywordSet::empty()), p0));
    put_on_battlefield(&mut state, permanent(bander, creature_definition(2, "Bander", 1, 5, KeywordSet::BANDING), p1));
    put_on_battlefield(&mut state, permanent(plain, creature_definition(3, "Plain", 1, 5, KeywordSet::empty()), p1));

    let mut engine = Engine::new(state);
    declare_attackers(&mut engine, p0, vec![(attacker, Target::Player(p1))]);
    pass_to_step(&mut engine, Step::DeclareBlockers);
    declare_blockers(&mut engine, p1, vec![(bander, attacker), (plain, attacker)]);
    pass_to_step(&mut engine, Step::CombatDamage);

    // The active player no longer gets to choose this split - one of the
    // attacker's blockers has banding.
    assert_illegal(assign_combat_damage(&mut engine, p0, vec![(attacker, vec![(bander, 2), (plain, 2)])]));

    let status = assign_combat_damage(&mut engine, p1, vec![(attacker, vec![(bander, 2), (plain, 2)])]);
    assert!(!matches!(status, StepStatus::Error(_)), "expected the defending player's split to be accepted, got {status:?}");
}

#[test]
fn trampler_blocked_by_a_single_banding_creature_still_needs_the_defending_players_choice() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let bander = ObjectId(1);

    let mut state = combat_state();
    put_on_battlefield(&mut state, permanent(attacker, creature_definition(1, "Trampler", 5, 5, KeywordSet::TRAMPLE), p0));
    put_on_battlefield(&mut state, permanent(bander, creature_definition(2, "Bander", 1, 2, KeywordSet::BANDING), p1));

    let mut engine = Engine::new(state);
    declare_attackers(&mut engine, p0, vec![(attacker, Target::Player(p1))]);
    pass_to_step(&mut engine, Step::DeclareBlockers);
    declare_blockers(&mut engine, p1, vec![(bander, attacker)]);
    pass_to_step(&mut engine, Step::CombatDamage);

    // A single blocker with trample still needs a chosen lethal-vs-excess
    // split (CR 702.19b) - and per 702.22j it's the defending player's
    // choice here, since the lone blocker has banding.
    assert_illegal(assign_combat_damage(&mut engine, p0, vec![(attacker, vec![(bander, 2)])]));

    let status = assign_combat_damage(&mut engine, p1, vec![(attacker, vec![(bander, 2)])]);
    assert!(!matches!(status, StepStatus::Error(_)), "expected the defending player's split (2 lethal, 3 trampling) to be accepted, got {status:?}");
}

// ---------------------------------------------------------------------
// A damage step needing splits chosen by two different players at once
// isn't supported - rejected outright rather than silently letting one
// player choose for both.
// ---------------------------------------------------------------------

#[test]
fn a_damage_step_needing_two_different_choosers_at_once_is_rejected() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let plain_attacker = ObjectId(0);
    let plain_blocker_a = ObjectId(1);
    let plain_blocker_b = ObjectId(2);
    let banded_attacker = ObjectId(3);
    let bander = ObjectId(4);

    let mut state = combat_state();
    put_on_battlefield(
        &mut state,
        permanent(plain_attacker, creature_definition(1, "PlainAttacker", 4, 4, KeywordSet::empty()), p0),
    );
    put_on_battlefield(&mut state, permanent(plain_blocker_a, creature_definition(2, "BlockerA", 1, 5, KeywordSet::empty()), p1));
    put_on_battlefield(&mut state, permanent(plain_blocker_b, creature_definition(3, "BlockerB", 1, 5, KeywordSet::empty()), p1));
    put_on_battlefield(
        &mut state,
        permanent(banded_attacker, creature_definition(4, "BandedAttacker", 4, 4, KeywordSet::empty()), p0),
    );
    put_on_battlefield(&mut state, permanent(bander, creature_definition(5, "Bander", 1, 5, KeywordSet::BANDING), p1));

    let mut engine = Engine::new(state);
    declare_attackers(
        &mut engine,
        p0,
        vec![(plain_attacker, Target::Player(p1)), (banded_attacker, Target::Player(p1))],
    );
    pass_to_step(&mut engine, Step::DeclareBlockers);
    declare_blockers(
        &mut engine,
        p1,
        vec![(plain_blocker_a, plain_attacker), (plain_blocker_b, plain_attacker), (bander, banded_attacker)],
    );
    pass_to_step(&mut engine, Step::CombatDamage);

    let assignments = vec![
        (plain_attacker, vec![(plain_blocker_a, 2), (plain_blocker_b, 2)]),
        (banded_attacker, vec![(bander, 4)]),
    ];
    // Neither player can submit this in one shot - the two splits need
    // different choosers (active player for the plain double-block,
    // defending player for the banding-blocked one).
    assert_illegal(assign_combat_damage(&mut engine, p0, assignments.clone()));
    assert_illegal(assign_combat_damage(&mut engine, p1, assignments));
}
