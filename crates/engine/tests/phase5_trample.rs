//! Phase 5 vertical slice: trample (CR 702.19a/b/d). Scoped to trample
//! against creatures/players only - trample over planeswalkers (CR
//! 702.19c/e/f/g) isn't modeled, since planeswalkers/battles are an explicit
//! DESIGN.md non-goal (`Target::Object` attack targets never occur in
//! practice today).
//!
//! Also fixes a real pre-existing bug found while scoping this: `combat.
//! blocks` records "blocked" once, at declare-blockers time, and
//! `AssignCombatDamage` must keep honoring that even if every blocker
//! assigned to an attacker is gone by damage time (CR 509.1h) - before this
//! slice, an attacker in that state fell into the same code path as a
//! genuinely-unblocked attacker and dealt its full power to the defending
//! player regardless of trample. See `blocked_attacker_with_no_living_
//! blockers_deals_no_damage_without_trample` below.

use std::collections::HashMap;

use stackfish_engine::engine::ActionError;
use stackfish_engine::commands::Rejected;
use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, zones, two_player_state as base_two_player_state};

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
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn permanent(id: ObjectId, definition: CardDefinition, owner: PlayerId) -> GameObject {
    GameObject { tapped: true, ..card_object(id, definition, owner, ZoneKind::Battlefield) }
}

fn put_on_battlefield(state: &mut GameState, object: GameObject) {
    let id = object.object_id;
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(id);
    state.objects.insert(id, object);
}

/// Same shape as `phase5_combat_damage.rs`'s own `combat_damage_state` -
/// starts right at `Step::CombatDamage` with `combat` already populated, so
/// a "blocker never made it onto the battlefield" scenario (used below to
/// stand in for "the blocker died before this step" without needing the
/// first-strike machinery to actually kill anything) is just a `blocks`
/// entry with no matching `put_on_battlefield` call.
fn combat_damage_state(combat: CombatState) -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        phase: Phase::Combat,
        step: Step::CombatDamage,
        combat: Some(combat),
        last_active_turn_number: HashMap::from([(p0, 1), (p1, 0)]),
        ..base_two_player_state(zones(p0, p1, &[ZoneKind::Battlefield]))
    }
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

fn try_assign_combat_damage(
    engine: &mut Engine,
    player: PlayerId,
    assignments: Vec<(ObjectId, Vec<(ObjectId, u32)>)>,
) -> Result<StepStatus, ActionError> {
    engine
        .submit_action(PlayerAction { player, kind: PlayerActionKind::AssignCombatDamage { assignments } })
        ?;
    Ok(drain(engine))
}

fn p1_life(engine: &Engine, p1: PlayerId) -> i32 {
    engine.state().players.iter().find(|p| p.player_id == p1).unwrap().life
}

/// Reads the actual dealt amount back from the ledger rather than the
/// blocker's `damage_marked` field - several tests below assign exactly (or
/// more than) a blocker's lethal amount, which kills it via the real SBA
/// pass `drain` already runs through, so the object itself is gone by the
/// time the assertion would run.
fn damage_event_amount(engine: &Engine, source: ObjectId, target_object: ObjectId) -> Option<u32> {
    engine.state().ledger.iter().find_map(|e| match e.kind {
        GameEventKind::DamageDealt { source: s, target: Target::Object(t), amount, .. }
            if s == source && t == target_object =>
        {
            Some(amount)
        }
        _ => None,
    })
}

// ---------------------------------------------------------------------
// Trample is a no-op unless blocked (CR 702.19a).
// ---------------------------------------------------------------------

#[test]
fn unblocked_trampler_is_a_plain_full_power_hit() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let mut state = combat_damage_state(CombatState {
        attackers: vec![(attacker, Target::Player(p1))],
        blocks: Vec::new(),
        declared_defenders: vec![p1],
        bands: Vec::new(), effect_blocked: Vec::new(), damage_assigned_this_step: false, pending_damage_splits: std::collections::HashMap::new(),
    });
    put_on_battlefield(&mut state, permanent(attacker, creature_definition("Rampager", 5, 5, KeywordSet::TRAMPLE), p0));

    let mut engine = Engine::new(state);
    let status = assign_combat_damage(&mut engine, p0, Vec::new());
    assert!(matches!(status, StepStatus::AwaitingAction { .. }), "unblocked needs no split, trample or not");
    assert_eq!(p1_life(&engine, p1), 15, "702.19a: trample has no effect on an unblocked attacker");
}

// ---------------------------------------------------------------------
// Single blocker (CR 702.19b).
// ---------------------------------------------------------------------

#[test]
fn single_blocker_trample_now_requires_a_chosen_split() {
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
    put_on_battlefield(&mut state, permanent(attacker, creature_definition("Rampager", 5, 5, KeywordSet::TRAMPLE), p0));
    put_on_battlefield(&mut state, permanent(blocker, creature_definition("Wall", 0, 2, KeywordSet::empty()), p1));

    let mut engine = Engine::new(state);
    let err = try_assign_combat_damage(&mut engine, p0, Vec::new()).unwrap_err();
    assert!(matches!(err, ActionError::Rejected(Rejected::IllegalAction(_))),
        "a single trampling blocker still needs a chosen lethal-vs-excess split: got {err:?}");
}

#[test]
fn single_blocker_trample_splits_lethal_and_excess_to_the_player() {
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
    put_on_battlefield(&mut state, permanent(attacker, creature_definition("Rampager", 5, 5, KeywordSet::TRAMPLE), p0));
    put_on_battlefield(&mut state, permanent(blocker, creature_definition("Wall", 0, 2, KeywordSet::empty()), p1));

    let mut engine = Engine::new(state);
    let status = assign_combat_damage(&mut engine, p0, vec![(attacker, vec![(blocker, 2)])]);
    assert!(matches!(status, StepStatus::AwaitingAction { .. }), "got {status:?}");
    assert_eq!(damage_event_amount(&engine, attacker, blocker), Some(2), "the blocker got exactly its lethal amount");
    assert_eq!(p1_life(&engine, p1), 17, "702.19b: the remaining 3 damage spills to the defending player");
}

#[test]
fn trample_cannot_send_excess_until_the_blocker_gets_lethal() {
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
    put_on_battlefield(&mut state, permanent(attacker, creature_definition("Rampager", 5, 5, KeywordSet::TRAMPLE), p0));
    put_on_battlefield(&mut state, permanent(blocker, creature_definition("Wall", 0, 2, KeywordSet::empty()), p1));

    let mut engine = Engine::new(state);
    // Only 1 of the 2 needed to be lethal - claiming any of the remaining 4
    // for the player must be rejected outright (CR 702.19b's "in that case
    // can't assign any damage to the player").
    let err = try_assign_combat_damage(&mut engine, p0, vec![(attacker, vec![(blocker, 1)])]).unwrap_err();
    assert!(matches!(err, ActionError::Rejected(Rejected::IllegalAction(_))), "got {err:?}");
}

#[test]
fn trample_may_assign_everything_to_the_blocker_leaving_nothing_for_the_player() {
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
    put_on_battlefield(&mut state, permanent(attacker, creature_definition("Rampager", 5, 5, KeywordSet::TRAMPLE), p0));
    put_on_battlefield(&mut state, permanent(blocker, creature_definition("Wall", 0, 2, KeywordSet::empty()), p1));

    let mut engine = Engine::new(state);
    let status = assign_combat_damage(&mut engine, p0, vec![(attacker, vec![(blocker, 5)])]);
    assert!(matches!(status, StepStatus::AwaitingAction { .. }), "got {status:?}");
    assert_eq!(damage_event_amount(&engine, attacker, blocker), Some(5));
    assert_eq!(p1_life(&engine, p1), 20, "controller may choose not to trample through at all");
}

// ---------------------------------------------------------------------
// Multiple blockers (CR 702.19b, same rule as a single trampling blocker).
// ---------------------------------------------------------------------

#[test]
fn multi_blocker_trample_distributes_lethal_then_spills_the_remainder() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let blocker_1 = ObjectId(1);
    let blocker_2 = ObjectId(2);
    let mut state = combat_damage_state(CombatState {
        attackers: vec![(attacker, Target::Player(p1))],
        blocks: vec![(blocker_1, attacker), (blocker_2, attacker)],
        declared_defenders: vec![p1],
        bands: Vec::new(), effect_blocked: Vec::new(), damage_assigned_this_step: false, pending_damage_splits: std::collections::HashMap::new(),
    });
    put_on_battlefield(&mut state, permanent(attacker, creature_definition("Rampager", 7, 7, KeywordSet::TRAMPLE), p0));
    put_on_battlefield(&mut state, permanent(blocker_1, creature_definition("Wall1", 0, 2, KeywordSet::empty()), p1));
    put_on_battlefield(&mut state, permanent(blocker_2, creature_definition("Wall2", 0, 3, KeywordSet::empty()), p1));

    let mut engine = Engine::new(state);
    let status = assign_combat_damage(&mut engine, p0, vec![(attacker, vec![(blocker_1, 2), (blocker_2, 3)])]);
    assert!(matches!(status, StepStatus::AwaitingAction { .. }), "got {status:?}");
    assert_eq!(damage_event_amount(&engine, attacker, blocker_1), Some(2));
    assert_eq!(damage_event_amount(&engine, attacker, blocker_2), Some(3));
    assert_eq!(p1_life(&engine, p1), 18, "7 power - 2 - 3 = 2 spills to the defending player");
}

// ---------------------------------------------------------------------
// Deathtouch + trample (CR 702.2b/702.19b).
// ---------------------------------------------------------------------

#[test]
fn deathtouch_trample_only_needs_one_damage_to_call_a_blocker_lethal() {
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
    put_on_battlefield(
        &mut state,
        permanent(attacker, creature_definition("Rampager", 5, 5, KeywordSet::TRAMPLE | KeywordSet::DEATHTOUCH), p0),
    );
    put_on_battlefield(&mut state, permanent(blocker, creature_definition("Wall", 0, 10, KeywordSet::empty()), p1));

    let mut engine = Engine::new(state);
    let status = assign_combat_damage(&mut engine, p0, vec![(attacker, vec![(blocker, 1)])]);
    assert!(matches!(status, StepStatus::AwaitingAction { .. }), "got {status:?}");
    assert_eq!(damage_event_amount(&engine, attacker, blocker), Some(1));
    assert_eq!(p1_life(&engine, p1), 16, "deathtouch makes 1 damage lethal, the other 4 spill through");
}

// ---------------------------------------------------------------------
// CR 509.1h/510.1c: blocked, but no living blockers by damage time.
// ---------------------------------------------------------------------

#[test]
fn blocked_attacker_with_no_living_blockers_deals_no_damage_without_trample() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let long_dead_blocker = ObjectId(1);
    // `blocks` still records the block (as CR 509.1h requires), but the
    // blocker was never even placed on the battlefield here - standing in
    // for "died before this step" without needing first-strike machinery.
    let mut state = combat_damage_state(CombatState {
        attackers: vec![(attacker, Target::Player(p1))],
        blocks: vec![(long_dead_blocker, attacker)],
        declared_defenders: vec![p1],
        bands: Vec::new(), effect_blocked: Vec::new(), damage_assigned_this_step: false, pending_damage_splits: std::collections::HashMap::new(),
    });
    put_on_battlefield(&mut state, permanent(attacker, creature_definition("Bear", 3, 3, KeywordSet::empty()), p0));

    let mut engine = Engine::new(state);
    let status = assign_combat_damage(&mut engine, p0, Vec::new());
    assert!(matches!(status, StepStatus::AwaitingAction { .. }), "got {status:?}");
    assert_eq!(p1_life(&engine, p1), 20, "509.1h/510.1c: blocked with zero living blockers deals no damage, no trample");
    assert!(
        !engine.state().ledger.iter().any(|e| matches!(
            e.kind,
            GameEventKind::DamageDealt { source, .. } if source == attacker
        )),
        "the attacker shouldn't deal damage to anything at all this step"
    );
}

#[test]
fn blocked_trampler_with_no_living_blockers_deals_full_power_per_702_19d() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let long_dead_blocker = ObjectId(1);
    let mut state = combat_damage_state(CombatState {
        attackers: vec![(attacker, Target::Player(p1))],
        blocks: vec![(long_dead_blocker, attacker)],
        declared_defenders: vec![p1],
        bands: Vec::new(), effect_blocked: Vec::new(), damage_assigned_this_step: false, pending_damage_splits: std::collections::HashMap::new(),
    });
    put_on_battlefield(&mut state, permanent(attacker, creature_definition("Rampager", 5, 5, KeywordSet::TRAMPLE), p0));

    let mut engine = Engine::new(state);
    let status = assign_combat_damage(&mut engine, p0, Vec::new());
    assert!(matches!(status, StepStatus::AwaitingAction { .. }), "got {status:?}");
    assert_eq!(p1_life(&engine, p1), 15, "702.19d: as though every blocker got lethal, so all 5 hit the defending player");
}
