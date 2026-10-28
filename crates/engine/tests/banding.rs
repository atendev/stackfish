//! CR 702.22 Banding in its plain form: band declaration, forced block propagation and who chooses a damage split.
//! See DESIGN.md for the rest.
// TODO: "bands with other [quality]" needs a keyword parameter KeywordSet cannot hold (CR 702.22b)
// TODO: effect-triggered blocking propagation (CR 702.22i)

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

// CR 702.22c/d
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

// CR 702.22c/d
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

// CR 702.22c/d
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

// CR 702.22h
#[test]
fn a_directly_blocked_band_member_forces_the_same_block_onto_a_shadow_bandmate() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let banding = ObjectId(0);
    // Shadow is normally blockable only by Shadow; the forced block overrides that.
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

    // Blocking the shadow creature directly would be illegal, so only the bandmate is submitted.
    declare_blockers(&mut engine, p1, vec![(blocker, banding)]).unwrap();

    let combat = engine.state().combat.as_ref().expect("still in combat");
    assert!(
        combat.blocks.contains(&(blocker, shadow_mate)),
        "expected the shadow bandmate to be forced-blocked by the same creature, got {:?}",
        combat.blocks
    );
    assert!(combat.blocks.contains(&(blocker, banding)));
}

// CR 702.22k
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

    // The forced blocker's split is required too, so omitting it is illegal.
    assert_illegal(assign_combat_damage(&mut engine, p0, Vec::new()));

    let status = assign_combat_damage(&mut engine, p0, vec![(blocker, vec![(a, 3), (b, 1)])]).unwrap();
    assert!(!matches!(status, StepStatus::Error(_)), "expected the split to be accepted, got {status:?}");
}

// CR 702.22j
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

    // The attacker has a banding blocker, so the active player does not choose.
    assert_illegal(assign_combat_damage(&mut engine, p0, vec![(attacker, vec![(bander, 2), (plain, 2)])]));

    let status = assign_combat_damage(&mut engine, p1, vec![(attacker, vec![(bander, 2), (plain, 2)])]).unwrap();
    assert!(!matches!(status, StepStatus::Error(_)), "expected the defending player's split to be accepted, got {status:?}");
}

// CR 702.22j
// A lone blocker with trample still needs a lethal-versus-excess split (CR 702.19b).
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

    assert_illegal(assign_combat_damage(&mut engine, p0, vec![(attacker, vec![(bander, 2)])]));

    let status = assign_combat_damage(&mut engine, p1, vec![(attacker, vec![(bander, 2)])]).unwrap();
    assert!(!matches!(status, StepStatus::Error(_)), "expected the defending player's split (2 lethal, 3 trampling) to be accepted, got {status:?}");
}

// p0 owns a plain double-blocked attacker and a banding-blocked one, so two players must each choose a split.
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

// CR 702.22j
#[test]
fn submitting_the_whole_steps_assignments_from_one_actor_is_still_rejected() {
    let (mut engine, p0, p1, plain_attacker, plain_blocker_a, plain_blocker_b, banded_attacker, bander, plain_blocker_c) =
        two_actor_split_fixture();

    let assignments = vec![
        (plain_attacker, vec![(plain_blocker_a, 2), (plain_blocker_b, 2)]),
        (banded_attacker, vec![(bander, 2), (plain_blocker_c, 2)]),
    ];
    // Two players choose, and each may submit only their own share.
    assert_illegal(assign_combat_damage(&mut engine, p0, assignments.clone()));
    assert_illegal(assign_combat_damage(&mut engine, p1, assignments));
}

// CR 702.22j
#[test]
fn an_actor_cannot_submit_the_other_actors_share() {
    let (mut engine, p0, p1, plain_attacker, _plain_blocker_a, _plain_blocker_b, banded_attacker, bander, plain_blocker_c) =
        two_actor_split_fixture();

    // p0 submits the banding-blocked attacker's split, which is not theirs.
    assert_illegal(assign_combat_damage(&mut engine, p0, vec![(banded_attacker, vec![(bander, 2), (plain_blocker_c, 2)])]));
    // p1 submits the reverse.
    assert_illegal(assign_combat_damage(&mut engine, p1, vec![(plain_attacker, vec![])]));
}

// CR 510.2
#[test]
fn each_actor_submitting_only_their_own_share_deals_damage_once_both_are_in() {
    let (mut engine, p0, p1, plain_attacker, plain_blocker_a, plain_blocker_b, banded_attacker, bander, plain_blocker_c) =
        two_actor_split_fixture();

    // p0 submits only their own share.
    let status = assign_combat_damage(&mut engine, p0, vec![(plain_attacker, vec![(plain_blocker_a, 2), (plain_blocker_b, 2)])]).unwrap();
    assert!(!matches!(status, StepStatus::Error(_)), "p0's own share should be accepted, got {status:?}");
    // Nothing is dealt until p1's share is in.
    assert!(!engine.state().combat.as_ref().unwrap().damage_assigned_this_step);
    assert_eq!(engine.state().object(plain_blocker_a).unwrap().damage_marked, 0);
    assert_eq!(engine.state().object(banded_attacker).unwrap().damage_marked, 0);

    // p1's share completes the step, so both splits deal damage together.
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

// CR 510.2
#[test]
fn the_two_actors_can_submit_in_either_order_with_the_same_result() {
    let (mut engine, p0, p1, plain_attacker, plain_blocker_a, plain_blocker_b, banded_attacker, bander, plain_blocker_c) =
        two_actor_split_fixture();

    // p1 goes first this time.
    let status = assign_combat_damage(&mut engine, p1, vec![(banded_attacker, vec![(bander, 2), (plain_blocker_c, 2)])]).unwrap();
    assert!(!matches!(status, StepStatus::Error(_)), "p1's own share should be accepted, got {status:?}");
    assert!(!engine.state().combat.as_ref().unwrap().damage_assigned_this_step);

    let status = assign_combat_damage(&mut engine, p0, vec![(plain_attacker, vec![(plain_blocker_a, 2), (plain_blocker_b, 2)])]).unwrap();
    assert!(!matches!(status, StepStatus::Error(_)), "p0's own share should be accepted, got {status:?}");
    assert!(engine.state().combat.as_ref().unwrap().damage_assigned_this_step);
    assert_eq!(engine.state().object(plain_blocker_a).unwrap().damage_marked, 2);
    assert_eq!(engine.state().object(bander).unwrap().damage_marked, 2);
}

// CR 702.22j
#[test]
fn an_actor_who_already_submitted_their_share_cannot_submit_again() {
    let (mut engine, p0, _p1, plain_attacker, plain_blocker_a, plain_blocker_b, _banded_attacker, _bander, _plain_blocker_c) =
        two_actor_split_fixture();

    assign_combat_damage(&mut engine, p0, vec![(plain_attacker, vec![(plain_blocker_a, 2), (plain_blocker_b, 2)])]).unwrap();
    // p0's share is already recorded and the other split is not theirs.
    assert_illegal(assign_combat_damage(&mut engine, p0, vec![(plain_attacker, vec![(plain_blocker_a, 4), (plain_blocker_b, 0)])]));
}

// CR 702.22j
#[test]
fn an_actor_cannot_submit_only_part_of_their_own_share() {
    // One actor required for two ids must still submit exactly their own share.
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

    // p0 submits only attacker_a's split and leaves attacker_b's out.
    assert_illegal(assign_combat_damage(&mut engine, p0, vec![(attacker_a, vec![(blocker_a1, 2), (blocker_a2, 2)])]));
}
