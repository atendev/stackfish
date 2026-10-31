//! CR 510.1, 702.7: first strike and double strike. If any attacker or blocker has either, combat gets `Step::FirstStrikeDamage` before `Step::CombatDamage`, and with neither it stays one step.
//! `AssignCombatDamage` skips attackers and blockers that no longer resolve in `state.objects`, since a first striker routinely kills its blocker in the first step.

use std::collections::HashMap;

use stackfish_engine::commands::Rejected;
use stackfish_engine::engine::ActionError;
use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, two_player_state as base_two_player_state, zones};

fn creature_definition(
    name: &str,
    power: i32,
    toughness: i32,
    keywords: KeywordSet,
) -> CardDefinition {
    CardDefinition {
        definition_id: 1,
        name: name.to_string(),
        mana_cost: ManaCost {
            symbols: Vec::new(),
        },
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

fn permanent(
    id: ObjectId,
    definition: CardDefinition,
    owner: PlayerId,
    controlled_since_turn: u32,
) -> GameObject {
    GameObject {
        controlled_since_turn,
        ..card_object(id, definition, owner, ZoneKind::Battlefield)
    }
}

fn put_on_battlefield(state: &mut GameState, object: GameObject) {
    let id = object.object_id;
    state
        .zone_mut(ZoneKind::Battlefield, None)
        .unwrap()
        .push_bottom(id);
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
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::DeclareBlockers { blocks },
        })
        .expect("player holds priority");
    drain(engine)
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
    engine.submit_action(PlayerAction {
        player,
        kind: PlayerActionKind::AssignCombatDamage { assignments },
    })?;
    Ok(drain(engine))
}

// Drives AdvanceTurnStep until state.step == target.
fn pass_until_step(engine: &mut Engine, target: Step) {
    for _ in 0..20 {
        if engine.state().step == target {
            return;
        }
        let active = engine.state().priority_player;
        engine
            .submit_action(PlayerAction {
                player: active,
                kind: PlayerActionKind::PassPriority,
            })
            .expect("priority window");
        drain(engine);
    }
    panic!(
        "never reached {target:?} - state.step = {:?}",
        engine.state().step
    );
}

// One full priority round is exactly one AdvanceTurnStep transition, so the landed step is the immediate next one.
fn pass_one_round(engine: &mut Engine) {
    for _ in 0..2 {
        let active = engine.state().priority_player;
        engine
            .submit_action(PlayerAction {
                player: active,
                kind: PlayerActionKind::PassPriority,
            })
            .expect("priority window");
        drain(engine);
    }
}

#[test]
fn no_first_or_double_strike_skips_straight_to_combat_damage() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let mut state = combat_state();
    put_on_battlefield(
        &mut state,
        permanent(
            attacker,
            creature_definition("Bear", 2, 2, KeywordSet::empty()),
            p0,
            0,
        ),
    );

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
        permanent(
            attacker,
            creature_definition("Knight", 2, 2, KeywordSet::FIRST_STRIKE),
            p0,
            0,
        ),
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

#[test]
fn unblocked_first_strike_attacker_deals_damage_in_first_strike_step_only() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let mut state = combat_state();
    put_on_battlefield(
        &mut state,
        permanent(
            attacker,
            creature_definition("Knight", 3, 3, KeywordSet::FIRST_STRIKE),
            p0,
            0,
        ),
    );

    let mut engine = Engine::new(state);
    declare_attackers(&mut engine, p0, vec![(attacker, Target::Player(p1))]);
    pass_until_step(&mut engine, Step::DeclareBlockers);
    declare_blockers(&mut engine, p1, Vec::new());
    pass_until_step(&mut engine, Step::FirstStrikeDamage);

    let status = assign_combat_damage(&mut engine, p0, Vec::new());
    assert!(matches!(status, StepStatus::AwaitingAction { .. }));
    let life_after_first_strike = engine
        .state()
        .players
        .iter()
        .find(|p| p.player_id == p1)
        .unwrap()
        .life;
    assert_eq!(
        life_after_first_strike, 17,
        "first-strike attacker deals its power in the first-strike step"
    );

    pass_until_step(&mut engine, Step::CombatDamage);
    let status = assign_combat_damage(&mut engine, p0, Vec::new());
    assert!(matches!(status, StepStatus::AwaitingAction { .. }));
    let life_after_regular = engine
        .state()
        .players
        .iter()
        .find(|p| p.player_id == p1)
        .unwrap()
        .life;
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
        permanent(
            attacker,
            creature_definition("Champion", 3, 3, KeywordSet::DOUBLE_STRIKE),
            p0,
            0,
        ),
    );

    let mut engine = Engine::new(state);
    declare_attackers(&mut engine, p0, vec![(attacker, Target::Player(p1))]);
    pass_until_step(&mut engine, Step::DeclareBlockers);
    declare_blockers(&mut engine, p1, Vec::new());
    pass_until_step(&mut engine, Step::FirstStrikeDamage);

    assign_combat_damage(&mut engine, p0, Vec::new());
    let life_after_first_strike = engine
        .state()
        .players
        .iter()
        .find(|p| p.player_id == p1)
        .unwrap()
        .life;
    assert_eq!(
        life_after_first_strike, 17,
        "702.4c: double strike deals damage in the first-strike step too"
    );

    pass_until_step(&mut engine, Step::CombatDamage);
    assign_combat_damage(&mut engine, p0, Vec::new());
    let life_after_regular = engine
        .state()
        .players
        .iter()
        .find(|p| p.player_id == p1)
        .unwrap()
        .life;
    assert_eq!(
        life_after_regular, 14,
        "702.4c: and again in the regular step - total damage is 2x power"
    );
}

#[test]
fn first_strike_attacker_kills_blocker_before_it_deals_damage_back() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let blocker = ObjectId(1);
    let mut state = combat_state();
    put_on_battlefield(
        &mut state,
        permanent(
            attacker,
            creature_definition("Knight", 3, 3, KeywordSet::FIRST_STRIKE),
            p0,
            0,
        ),
    );
    put_on_battlefield(
        &mut state,
        permanent(
            blocker,
            creature_definition("Wall", 2, 2, KeywordSet::empty()),
            p1,
            0,
        ),
    );

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

    // AssignCombatDamage must accept an id that no longer resolves in state.objects, which a first-strike kill produces.
    pass_until_step(&mut engine, Step::CombatDamage);
    let status = assign_combat_damage(&mut engine, p0, Vec::new());
    assert!(
        matches!(status, StepStatus::AwaitingAction { .. }),
        "the regular step's AssignCombatDamage must silently skip the dead blocker, not \
         InvalidTarget-error the whole command: got {status:?}"
    );
    let p1_life = engine
        .state()
        .players
        .iter()
        .find(|p| p.player_id == p1)
        .unwrap()
        .life;
    assert_eq!(
        p1_life, 20,
        "the first-strike attacker (no double strike) doesn't strike again in the regular step"
    );
}

#[test]
fn multi_blocked_split_requirement_is_scoped_to_the_current_step() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    // attacker_b forces a FirstStrikeDamage step to be inserted, and attacker_a has neither keyword.
    let attacker_a = ObjectId(0);
    let attacker_b = ObjectId(1);
    let blocker_1 = ObjectId(2);
    let blocker_2 = ObjectId(3);
    let mut state = combat_state();
    put_on_battlefield(
        &mut state,
        permanent(
            attacker_a,
            creature_definition("Bear", 5, 10, KeywordSet::empty()),
            p0,
            0,
        ),
    );
    put_on_battlefield(
        &mut state,
        permanent(
            attacker_b,
            creature_definition("Knight", 1, 1, KeywordSet::FIRST_STRIKE),
            p0,
            0,
        ),
    );
    put_on_battlefield(
        &mut state,
        permanent(
            blocker_1,
            creature_definition("Wall1", 1, 10, KeywordSet::empty()),
            p1,
            0,
        ),
    );
    put_on_battlefield(
        &mut state,
        permanent(
            blocker_2,
            creature_definition("Wall2", 1, 10, KeywordSet::empty()),
            p1,
            0,
        ),
    );

    let mut engine = Engine::new(state);
    declare_attackers(
        &mut engine,
        p0,
        vec![
            (attacker_a, Target::Player(p1)),
            (attacker_b, Target::Player(p1)),
        ],
    );
    pass_until_step(&mut engine, Step::DeclareBlockers);
    declare_blockers(
        &mut engine,
        p1,
        vec![(blocker_1, attacker_a), (blocker_2, attacker_a)],
    );
    pass_until_step(&mut engine, Step::FirstStrikeDamage);

    // attacker_a skips the first-strike step, so neither of its blockers needs a split and an empty assignments list is accepted.
    let status = assign_combat_damage(&mut engine, p0, Vec::new());
    assert!(
        matches!(status, StepStatus::AwaitingAction { .. }),
        "no split required for a non-participating attacker during the first-strike step: got {status:?}"
    );
    assert_eq!(engine.state().object(blocker_1).unwrap().damage_marked, 0);
    assert_eq!(engine.state().object(blocker_2).unwrap().damage_marked, 0);

    pass_until_step(&mut engine, Step::CombatDamage);
    // attacker_a participates in the regular step and is blocked by both walls, so the split requirement applies.
    let err = try_assign_combat_damage(&mut engine, p0, Vec::new()).unwrap_err();
    assert!(
        matches!(err, ActionError::Rejected(Rejected::IllegalAction(_))),
        "a split is required once attacker_a actually participates in the regular step: got {err:?}"
    );
}
