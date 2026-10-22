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
//! - `phase5/banding-two-actor-damage-split`: a combat damage step needing
//!   splits chosen by *two different* players at once (e.g. a plain
//!   multi-block plus a separate banding-blocked attacker, simultaneously)
//!   is real now too - `CombatState::pending_damage_splits` accumulates
//!   each required actor's own validated share across separate
//!   `AssignCombatDamage` calls (the same `declared_defenders` precedent
//!   `DeclareBlockers` already established for "this turn-based action can
//!   need input from more than one player"), and only actually builds and
//!   deals damage once every required id is covered (CR 510.2: still one
//!   simultaneous batch, not per-actor). An actor still can't submit
//!   anything but exactly their own still-pending share in one call - not
//!   the whole step's assignments, not another actor's ids, not a partial
//!   subset of their own.
//!
//! Deliberately unmodeled, named rather than guessed at: CR 702.22i
//! (effect-triggered blocking propagation - no such effect exists yet in
//! this engine), and the "bands with other [quality]" variant (needs a
//! parameter, same gap `KeywordSet::BANDING`'s own doc comment names for
//! Landwalk).

use std::collections::HashMap;

use stackfish_engine::engine::ActionError;
use stackfish_engine::commands::Rejected;
use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, zones, two_player_state as base_two_player_state};

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
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
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
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn permanent(id: ObjectId, definition: CardDefinition, owner: PlayerId) -> GameObject {
    card_object(id, definition, owner, ZoneKind::Battlefield)
}

fn put_on_battlefield(state: &mut GameState, object: GameObject) {
    let id = object.object_id;
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(id);
    state.objects.insert(id, object);
}

fn combat_state() -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        phase: Phase::Combat,
        step: Step::DeclareAttackers,
        last_active_turn_number: HashMap::from([(p0, 1), (p1, 0)]),
        ..base_two_player_state(zones(p0, p1, &[ZoneKind::Battlefield]))
    }
}

fn declare_attackers(engine: &mut Engine, player: PlayerId, attackers: Vec<(ObjectId, Target)>) -> Result<StepStatus, ActionError> {
    engine
        .submit_action(PlayerAction { player, kind: PlayerActionKind::DeclareAttackers { attackers } })
        ?;
    Ok(drain(engine))
}

fn declare_bands(engine: &mut Engine, player: PlayerId, bands: Vec<Vec<ObjectId>>) -> Result<StepStatus, ActionError> {
    engine
        .submit_action(PlayerAction { player, kind: PlayerActionKind::DeclareBands { bands } })
        ?;
    Ok(drain(engine))
}

fn declare_blockers(engine: &mut Engine, player: PlayerId, blocks: Vec<(ObjectId, ObjectId)>) -> Result<StepStatus, ActionError> {
    engine
        .submit_action(PlayerAction { player, kind: PlayerActionKind::DeclareBlockers { blocks } })
        ?;
    Ok(drain(engine))
}

fn assign_combat_damage(
    engine: &mut Engine,
    player: PlayerId,
    assignments: Vec<(ObjectId, Vec<(ObjectId, u32)>)>,
) -> Result<StepStatus, ActionError> {
    engine
        .submit_action(PlayerAction { player, kind: PlayerActionKind::AssignCombatDamage { assignments } })
        ?;
    Ok(drain(engine))
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

fn assert_illegal(status: Result<StepStatus, ActionError>) {
    assert!(matches!(status, Err(ActionError::Rejected(Rejected::IllegalAction(_)))), "expected an illegal action, got {status:?}");
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
    declare_attackers(&mut engine, p0, vec![(a, Target::Player(p1)), (b, Target::Player(p1))]).unwrap();
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
    ).unwrap();
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
    declare_attackers(&mut engine, p0, vec![(banding, Target::Player(p1)), (other, Target::Object(pw))]).unwrap();
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
    declare_attackers(&mut engine, p0, vec![(banding, Target::Player(p1)), (shadow_mate, Target::Player(p1))]).unwrap();
    assert!(matches!(declare_bands(&mut engine, p0, vec![vec![banding, shadow_mate]]).unwrap(), StepStatus::AwaitingAction { .. }));
    pass_to_step(&mut engine, Step::DeclareBlockers);

    // Directly blocking the shadow creature would be illegal on its own -
    // only the non-shadow bandmate is submitted as a direct block.
    declare_blockers(&mut engine, p1, vec![(blocker, banding)]).unwrap();

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
    declare_attackers(&mut engine, p0, vec![(a, Target::Player(p1)), (b, Target::Player(p1))]).unwrap();
    declare_bands(&mut engine, p0, vec![vec![a, b]]).unwrap();
    pass_to_step(&mut engine, Step::DeclareBlockers);
    declare_blockers(&mut engine, p1, vec![(blocker, a)]).unwrap();

    let combat = engine.state().combat.as_ref().unwrap();
    assert!(combat.blocks.contains(&(blocker, a)));
    assert!(combat.blocks.contains(&(blocker, b)));

    pass_to_step(&mut engine, Step::CombatDamage);

    // Omitting the forced blocker's split is illegal (510.1e atomicity,
    // now covering the blocker side too).
    assert_illegal(assign_combat_damage(&mut engine, p0, Vec::new()));

    let status = assign_combat_damage(&mut engine, p0, vec![(blocker, vec![(a, 3), (b, 1)])]).unwrap();
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
    declare_attackers(&mut engine, p0, vec![(attacker, Target::Player(p1))]).unwrap();
    pass_to_step(&mut engine, Step::DeclareBlockers);
    declare_blockers(&mut engine, p1, vec![(bander, attacker), (plain, attacker)]).unwrap();
    pass_to_step(&mut engine, Step::CombatDamage);

    // The active player no longer gets to choose this split - one of the
    // attacker's blockers has banding.
    assert_illegal(assign_combat_damage(&mut engine, p0, vec![(attacker, vec![(bander, 2), (plain, 2)])]));

    let status = assign_combat_damage(&mut engine, p1, vec![(attacker, vec![(bander, 2), (plain, 2)])]).unwrap();
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
    declare_attackers(&mut engine, p0, vec![(attacker, Target::Player(p1))]).unwrap();
    pass_to_step(&mut engine, Step::DeclareBlockers);
    declare_blockers(&mut engine, p1, vec![(bander, attacker)]).unwrap();
    pass_to_step(&mut engine, Step::CombatDamage);

    // A single blocker with trample still needs a chosen lethal-vs-excess
    // split (CR 702.19b) - and per 702.22j it's the defending player's
    // choice here, since the lone blocker has banding.
    assert_illegal(assign_combat_damage(&mut engine, p0, vec![(attacker, vec![(bander, 2)])]));

    let status = assign_combat_damage(&mut engine, p1, vec![(attacker, vec![(bander, 2)])]).unwrap();
    assert!(!matches!(status, StepStatus::Error(_)), "expected the defending player's split (2 lethal, 3 trampling) to be accepted, got {status:?}");
}

// ---------------------------------------------------------------------
// A damage step needing splits chosen by two different players at once
// (phase5/banding-two-actor-damage-split): neither player can submit the
// *other* player's share, or the whole step's assignments in one call -
// each must submit exactly their own still-pending share, separately.
// ---------------------------------------------------------------------

/// Shared two-actor fixture: `plain_attacker` (p0's) is double-blocked by
/// two plain creatures (needs p0's own choice, the attacker's controller -
/// no banding involved at all on that side). `banded_attacker` (also p0's)
/// is *also* double-blocked (a real split is needed there too - CR 510.1c
/// needs 2+ blockers, not just banding's presence, for an actual choice to
/// exist), but one of its two blockers has `BANDING`, which flips the
/// chooser to the defending player instead (CR 702.22j). Positioned at
/// `Step::CombatDamage`, ready for `AssignCombatDamage`.
fn two_actor_split_fixture() -> (Engine, PlayerId, PlayerId, ObjectId, ObjectId, ObjectId, ObjectId, ObjectId, ObjectId) {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let plain_attacker = ObjectId(0);
    let plain_blocker_a = ObjectId(1);
    let plain_blocker_b = ObjectId(2);
    let banded_attacker = ObjectId(3);
    let bander = ObjectId(4);
    let plain_blocker_c = ObjectId(5);

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
    put_on_battlefield(&mut state, permanent(plain_blocker_c, creature_definition(6, "BlockerC", 1, 5, KeywordSet::empty()), p1));

    let mut engine = Engine::new(state);
    declare_attackers(
        &mut engine,
        p0,
        vec![(plain_attacker, Target::Player(p1)), (banded_attacker, Target::Player(p1))],
    ).unwrap();
    pass_to_step(&mut engine, Step::DeclareBlockers);
    declare_blockers(
        &mut engine,
        p1,
        vec![
            (plain_blocker_a, plain_attacker),
            (plain_blocker_b, plain_attacker),
            (bander, banded_attacker),
            (plain_blocker_c, banded_attacker),
        ],
    ).unwrap();
    pass_to_step(&mut engine, Step::CombatDamage);
    (engine, p0, p1, plain_attacker, plain_blocker_a, plain_blocker_b, banded_attacker, bander, plain_blocker_c)
}

#[test]
fn submitting_the_whole_steps_assignments_from_one_actor_is_still_rejected() {
    let (mut engine, p0, p1, plain_attacker, plain_blocker_a, plain_blocker_b, banded_attacker, bander, plain_blocker_c) =
        two_actor_split_fixture();

    let assignments = vec![
        (plain_attacker, vec![(plain_blocker_a, 2), (plain_blocker_b, 2)]),
        (banded_attacker, vec![(bander, 2), (plain_blocker_c, 2)]),
    ];
    // Neither player can submit this in one shot - the two splits need
    // different choosers (active player for the plain double-block,
    // defending player for the banding-blocked one), and each actor may
    // only ever submit their own share.
    assert_illegal(assign_combat_damage(&mut engine, p0, assignments.clone()));
    assert_illegal(assign_combat_damage(&mut engine, p1, assignments));
}

#[test]
fn an_actor_cannot_submit_the_other_actors_share() {
    let (mut engine, p0, p1, plain_attacker, _plain_blocker_a, _plain_blocker_b, banded_attacker, bander, plain_blocker_c) =
        two_actor_split_fixture();

    // p0 (the plain double-block's own chooser) tries to submit the
    // banding-blocked attacker's split instead - not theirs to choose.
    assert_illegal(assign_combat_damage(&mut engine, p0, vec![(banded_attacker, vec![(bander, 2), (plain_blocker_c, 2)])]));
    // p1 (the banding split's own chooser) tries the reverse.
    assert_illegal(assign_combat_damage(&mut engine, p1, vec![(plain_attacker, vec![])]));
}

#[test]
fn each_actor_submitting_only_their_own_share_deals_damage_once_both_are_in() {
    let (mut engine, p0, p1, plain_attacker, plain_blocker_a, plain_blocker_b, banded_attacker, bander, plain_blocker_c) =
        two_actor_split_fixture();

    // p0 submits first - only their own share (the plain double-block).
    let status = assign_combat_damage(&mut engine, p0, vec![(plain_attacker, vec![(plain_blocker_a, 2), (plain_blocker_b, 2)])]).unwrap();
    assert!(!matches!(status, StepStatus::Error(_)), "p0's own share should be accepted, got {status:?}");
    // Nothing dealt yet - CR 510.2 damage is simultaneous, still waiting on
    // p1's own share for the banding-blocked attacker.
    assert!(!engine.state().combat.as_ref().unwrap().damage_assigned_this_step);
    assert_eq!(engine.state().object(plain_blocker_a).unwrap().damage_marked, 0);
    assert_eq!(engine.state().object(banded_attacker).unwrap().damage_marked, 0);

    // p1 submits their own share (the banding-blocked attacker) - this
    // completes the step, so both splits' damage lands together.
    let status = assign_combat_damage(&mut engine, p1, vec![(banded_attacker, vec![(bander, 2), (plain_blocker_c, 2)])]).unwrap();
    assert!(!matches!(status, StepStatus::Error(_)), "p1's own share should be accepted, got {status:?}");
    assert!(engine.state().combat.as_ref().unwrap().damage_assigned_this_step);
    assert_eq!(engine.state().object(plain_blocker_a).unwrap().damage_marked, 2);
    assert_eq!(engine.state().object(plain_blocker_b).unwrap().damage_marked, 2);
    assert_eq!(engine.state().object(bander).unwrap().damage_marked, 2);
    assert_eq!(engine.state().object(plain_blocker_c).unwrap().damage_marked, 2);
    // The banded attacker itself also took damage back from both its blockers.
    assert_eq!(engine.state().object(banded_attacker).unwrap().damage_marked, 2);
}

#[test]
fn the_two_actors_can_submit_in_either_order_with_the_same_result() {
    let (mut engine, p0, p1, plain_attacker, plain_blocker_a, plain_blocker_b, banded_attacker, bander, plain_blocker_c) =
        two_actor_split_fixture();

    // p1 (the banding split's own chooser) goes first this time.
    let status = assign_combat_damage(&mut engine, p1, vec![(banded_attacker, vec![(bander, 2), (plain_blocker_c, 2)])]).unwrap();
    assert!(!matches!(status, StepStatus::Error(_)), "p1's own share should be accepted, got {status:?}");
    assert!(!engine.state().combat.as_ref().unwrap().damage_assigned_this_step);

    let status = assign_combat_damage(&mut engine, p0, vec![(plain_attacker, vec![(plain_blocker_a, 2), (plain_blocker_b, 2)])]).unwrap();
    assert!(!matches!(status, StepStatus::Error(_)), "p0's own share should be accepted, got {status:?}");
    assert!(engine.state().combat.as_ref().unwrap().damage_assigned_this_step);
    assert_eq!(engine.state().object(plain_blocker_a).unwrap().damage_marked, 2);
    assert_eq!(engine.state().object(bander).unwrap().damage_marked, 2);
}

#[test]
fn an_actor_who_already_submitted_their_share_cannot_submit_again() {
    let (mut engine, p0, _p1, plain_attacker, plain_blocker_a, plain_blocker_b, _banded_attacker, _bander, _plain_blocker_c) =
        two_actor_split_fixture();

    assign_combat_damage(&mut engine, p0, vec![(plain_attacker, vec![(plain_blocker_a, 2), (plain_blocker_b, 2)])]).unwrap();
    // p0 has nothing left to contribute this step - their own share is
    // already recorded, and the banding-blocked attacker was never theirs
    // to choose.
    assert_illegal(assign_combat_damage(&mut engine, p0, vec![(plain_attacker, vec![(plain_blocker_a, 4), (plain_blocker_b, 0)])]));
}

#[test]
fn an_actor_cannot_submit_only_part_of_their_own_share() {
    // A single actor required for 2+ ids (not the two-actor case above,
    // but the same "exactly this actor's own share, no partial subset"
    // atomicity, now scoped per-actor rather than step-wide) - two
    // separate plain double-blocked attackers, both chosen by p0.
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker_a = ObjectId(0);
    let blocker_a1 = ObjectId(1);
    let blocker_a2 = ObjectId(2);
    let attacker_b = ObjectId(3);
    let blocker_b1 = ObjectId(4);
    let blocker_b2 = ObjectId(5);

    let mut state = combat_state();
    put_on_battlefield(&mut state, permanent(attacker_a, creature_definition(1, "AttackerA", 4, 4, KeywordSet::empty()), p0));
    put_on_battlefield(&mut state, permanent(blocker_a1, creature_definition(2, "BlockerA1", 1, 5, KeywordSet::empty()), p1));
    put_on_battlefield(&mut state, permanent(blocker_a2, creature_definition(3, "BlockerA2", 1, 5, KeywordSet::empty()), p1));
    put_on_battlefield(&mut state, permanent(attacker_b, creature_definition(4, "AttackerB", 4, 4, KeywordSet::empty()), p0));
    put_on_battlefield(&mut state, permanent(blocker_b1, creature_definition(5, "BlockerB1", 1, 5, KeywordSet::empty()), p1));
    put_on_battlefield(&mut state, permanent(blocker_b2, creature_definition(6, "BlockerB2", 1, 5, KeywordSet::empty()), p1));

    let mut engine = Engine::new(state);
    declare_attackers(&mut engine, p0, vec![(attacker_a, Target::Player(p1)), (attacker_b, Target::Player(p1))]).unwrap();
    pass_to_step(&mut engine, Step::DeclareBlockers);
    declare_blockers(
        &mut engine,
        p1,
        vec![(blocker_a1, attacker_a), (blocker_a2, attacker_a), (blocker_b1, attacker_b), (blocker_b2, attacker_b)],
    ).unwrap();
    pass_to_step(&mut engine, Step::CombatDamage);

    // p0 is the sole chooser for both - but submits only attacker_a's
    // split, leaving attacker_b's out entirely.
    assert_illegal(assign_combat_damage(&mut engine, p0, vec![(attacker_a, vec![(blocker_a1, 2), (blocker_a2, 2)])]));
}
