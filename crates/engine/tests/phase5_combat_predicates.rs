//! Ticket 026: menace and "attacks each combat if able" as whole-declaration
//! predicates shared by the entries (`legal_attackers`, `legal_blockers`) and
//! the command validation (`submit_action`).

use std::collections::HashMap;

use stackfish_engine::commands::Rejected;
use stackfish_engine::decision::{Answer, AttackerEntry, BlockerEntry, RejectionCode, Reply, ReplyShape};
use stackfish_engine::engine::{ActionError, Engine, StepStatus};
use stackfish_engine::*;
use stackfish_testutil::{assert_action_rejected, card_object, drain, minimal_card_definition, two_player_state, zones};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

fn creature(name: &str, keywords: KeywordSet) -> CardDefinition {
    CardDefinition {
        power: Some(PTValue::Fixed(2)),
        toughness: Some(PTValue::Fixed(2)),
        keywords,
        ..minimal_card_definition(1, name, name, ManaCost { symbols: Vec::new() }, CardTypeSet::CREATURE)
    }
}

fn combat_state(step: Step) -> GameState {
    GameState {
        phase: Phase::Combat,
        step,
        last_active_turn_number: HashMap::from([(P0, 1), (P1, 0)]),
        ..two_player_state(zones(P0, P1, &[ZoneKind::Hand, ZoneKind::Battlefield, ZoneKind::Graveyard]))
    }
}

fn put(state: &mut GameState, id: u64, owner: PlayerId, definition: CardDefinition) {
    let object = GameObject { controlled_since_turn: 0, ..card_object(ObjectId(id), definition, owner, ZoneKind::Battlefield) };
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(object.object_id);
    state.objects.insert(object.object_id, object);
}

fn with_combat(state: &mut GameState, attackers: &[u64]) {
    state.combat = Some(CombatState {
        attackers: attackers.iter().map(|&id| (ObjectId(id), Target::Player(P1))).collect(),
        blocks: Vec::new(),
        declared_defenders: Vec::new(),
        bands: Vec::new(),
        effect_blocked: Vec::new(),
        damage_assigned_this_step: false,
        pending_damage_splits: HashMap::new(),
    });
}

fn declare_attackers(attackers: &[u64]) -> PlayerAction {
    PlayerAction {
        player: P0,
        kind: PlayerActionKind::DeclareAttackers {
            attackers: attackers.iter().map(|&id| (ObjectId(id), Target::Player(P1))).collect(),
        },
    }
}

fn declare_blockers(blocks: &[(u64, u64)]) -> PlayerAction {
    PlayerAction {
        player: P1,
        kind: PlayerActionKind::DeclareBlockers {
            blocks: blocks.iter().map(|&(blocker, attacker)| (ObjectId(blocker), ObjectId(attacker))).collect(),
        },
    }
}

fn pass(player: PlayerId) -> PlayerAction {
    PlayerAction { player, kind: PlayerActionKind::PassPriority }
}

fn assert_illegal(error: ActionError) {
    assert!(matches!(error, ActionError::Rejected(Rejected::IllegalAction(_))), "expected an illegal action, got {error:?}");
}

fn no_filter() -> EffectFilter {
    EffectFilter {
        controller: None,
        card_types: None,
        self_only: false,
        subtypes: None,
        excluded_subtypes: None,
        supertypes: None,
        excluded_supertypes: None,
    }
}

fn self_keyword_effect(source: u64, primitive: EffectPrimitive, timestamp: u64) -> ContinuousEffect {
    ContinuousEffect {
        source: ObjectId(source),
        layer: Layer::AbilityAdding,
        affected: AffectedFilter::Primitive(EffectFilter { self_only: true, ..no_filter() }),
        effect: EffectPayload::Primitive(primitive),
        timestamp: SequenceNo(timestamp),
        duration: EffectDuration::Permanent,
        condition: None,
    }
}

// ---------------------------------------------------------------------
// Must attack (CR 508.1d)
// ---------------------------------------------------------------------

#[test]
fn a_creature_that_must_attack_cannot_stay_home() {
    let mut state = combat_state(Step::DeclareAttackers);
    put(&mut state, 1, P0, creature("Berserker", KeywordSet::ATTACKS_EACH_COMBAT));
    put(&mut state, 2, P0, creature("Bear", KeywordSet::empty()));
    let mut engine = Engine::new(state);

    assert_illegal(assert_action_rejected(&mut engine, "empty declaration", declare_attackers(&[])));
    assert_illegal(assert_action_rejected(&mut engine, "only the other creature", declare_attackers(&[2])));
    engine.submit_action(declare_attackers(&[1])).expect("the required creature alone is a legal declaration");
}

#[test]
fn every_creature_that_must_attack_has_to_attack() {
    let mut state = combat_state(Step::DeclareAttackers);
    put(&mut state, 1, P0, creature("Berserker", KeywordSet::ATTACKS_EACH_COMBAT));
    put(&mut state, 2, P0, creature("Brute", KeywordSet::ATTACKS_EACH_COMBAT));
    let mut engine = Engine::new(state);

    assert_illegal(assert_action_rejected(&mut engine, "one of two", declare_attackers(&[1])));
    engine.submit_action(declare_attackers(&[1, 2])).expect("both attack");
}

#[test]
fn a_creature_that_cannot_attack_is_not_required_to() {
    let tapped = |state: &mut GameState| state.objects.get_mut(&ObjectId(1)).unwrap().tapped = true;
    let sick = |state: &mut GameState| state.objects.get_mut(&ObjectId(1)).unwrap().controlled_since_turn = 1;
    let eliminated = |state: &mut GameState| state.players.iter_mut().find(|p| p.player_id == P1).unwrap().eliminated = true;
    let cases: [(&str, KeywordSet, &dyn Fn(&mut GameState)); 4] = [
        ("tapped", KeywordSet::ATTACKS_EACH_COMBAT, &tapped),
        ("summoning sick", KeywordSet::ATTACKS_EACH_COMBAT, &sick),
        ("defender", KeywordSet::ATTACKS_EACH_COMBAT | KeywordSet::DEFENDER, &|_| {}),
        ("nobody to attack", KeywordSet::ATTACKS_EACH_COMBAT, &eliminated),
    ];
    for (name, keywords, tweak) in cases {
        let mut state = combat_state(Step::DeclareAttackers);
        put(&mut state, 1, P0, creature("Berserker", keywords));
        tweak(&mut state);
        let mut engine = Engine::new(state);
        engine.submit_action(declare_attackers(&[])).unwrap_or_else(|e| panic!("{name}: an empty declaration is legal, got {e:?}"));
    }
}

#[test]
fn passing_priority_cannot_skip_a_required_attack() {
    let mut state = combat_state(Step::DeclareAttackers);
    put(&mut state, 1, P0, creature("Berserker", KeywordSet::ATTACKS_EACH_COMBAT));
    let mut engine = Engine::new(state);
    assert_illegal(assert_action_rejected(&mut engine, "pass with a required attacker", pass(P0)));

    let mut state = combat_state(Step::DeclareAttackers);
    put(&mut state, 1, P0, creature("Bear", KeywordSet::empty()));
    let mut engine = Engine::new(state);
    engine.submit_action(pass(P0)).expect("nothing is required, so passing is fine");
}

#[test]
fn a_granted_requirement_counts_and_a_removed_one_does_not() {
    let mut state = combat_state(Step::DeclareAttackers);
    put(&mut state, 1, P0, creature("Bear", KeywordSet::empty()));
    state.continuous_effects.push(self_keyword_effect(1, EffectPrimitive::AddKeyword(KeywordSet::ATTACKS_EACH_COMBAT), 0));
    let mut engine = Engine::new(state);
    assert_illegal(assert_action_rejected(&mut engine, "granted requirement", declare_attackers(&[])));

    let mut state = combat_state(Step::DeclareAttackers);
    put(&mut state, 1, P0, creature("Berserker", KeywordSet::ATTACKS_EACH_COMBAT));
    state.continuous_effects.push(self_keyword_effect(1, EffectPrimitive::RemoveKeyword(KeywordSet::ATTACKS_EACH_COMBAT), 0));
    let mut engine = Engine::new(state);
    engine.submit_action(declare_attackers(&[])).expect("the requirement was removed");
}

#[test]
fn the_attackers_entry_marks_a_required_creature() {
    let mut state = combat_state(Step::DeclareAttackers);
    put(&mut state, 1, P0, creature("Berserker", KeywordSet::ATTACKS_EACH_COMBAT));
    put(&mut state, 2, P0, creature("Bear", KeywordSet::empty()));
    let engine = Engine::new(state);
    assert_eq!(
        engine.legal_attackers(),
        vec![
            AttackerEntry { attacker: ObjectId(1), defenders: vec![Target::Player(P1)], required: true },
            AttackerEntry { attacker: ObjectId(2), defenders: vec![Target::Player(P1)], required: false },
        ]
    );
}

#[test]
fn the_facade_refuses_a_reply_that_leaves_a_required_attacker_home() {
    let mut state = combat_state(Step::DeclareAttackers);
    put(&mut state, 1, P0, creature("Berserker", KeywordSet::ATTACKS_EACH_COMBAT));
    put(&mut state, 2, P0, creature("Bear", KeywordSet::empty()));
    let mut engine = Engine::new(state);
    let decision = engine.open_decision().expect("a declare attackers Decision");
    assert!(matches!(&decision.shape, ReplyShape::DeclareAttackers { attackers } if attackers[0].required));

    let before = engine.snapshot();
    let bear_only = Reply::DeclareAttackers(vec![(ObjectId(2), Target::Player(P1))]);
    let refusal = engine.answer(P0, Answer { decision: decision.id, reply: bear_only }).unwrap_err();
    assert_eq!(refusal.code, RejectionCode::InvalidChoice);
    assert_eq!(engine.snapshot(), before, "a refused Answer changes nothing");
    assert_eq!(engine.open_decision().unwrap().id, decision.id, "the same Decision stays open");

    let both = Reply::DeclareAttackers(vec![(ObjectId(1), Target::Player(P1)), (ObjectId(2), Target::Player(P1))]);
    engine.answer(P0, Answer { decision: decision.id, reply: both }).unwrap();
    assert!(!matches!(drain(&mut engine), StepStatus::Error(_)));
}

// ---------------------------------------------------------------------
// Menace (CR 509.1b, 702.111b)
// ---------------------------------------------------------------------

fn menace_board(defenders: &[(u64, KeywordSet)]) -> GameState {
    let mut state = combat_state(Step::DeclareBlockers);
    put(&mut state, 1, P0, creature("Menacer", KeywordSet::MENACE));
    for &(id, keywords) in defenders {
        put(&mut state, id, P1, creature("Defender", keywords));
    }
    with_combat(&mut state, &[1]);
    state
}

#[test]
fn a_menace_attacker_is_offered_only_when_two_creatures_could_block_it() {
    let one = Engine::new(menace_board(&[(10, KeywordSet::empty())]));
    assert!(one.legal_blockers(P1).is_empty(), "one creature can never block a menace attacker");

    let two = Engine::new(menace_board(&[(10, KeywordSet::empty()), (11, KeywordSet::empty())]));
    assert_eq!(
        two.legal_blockers(P1),
        vec![
            BlockerEntry { blocker: ObjectId(10), attackers: vec![ObjectId(1)] },
            BlockerEntry { blocker: ObjectId(11), attackers: vec![ObjectId(1)] },
        ]
    );
}

#[test]
fn a_blocker_that_cannot_block_does_not_count_toward_menace() {
    let mut state = combat_state(Step::DeclareBlockers);
    put(&mut state, 1, P0, creature("Flying Menacer", KeywordSet::MENACE | KeywordSet::FLYING));
    put(&mut state, 10, P1, creature("Spider", KeywordSet::REACH));
    put(&mut state, 11, P1, creature("Bear", KeywordSet::empty()));
    with_combat(&mut state, &[1]);
    assert!(Engine::new(state).legal_blockers(P1).is_empty(), "the bear can't block a flyer, so only one creature could");
}

#[test]
fn a_plain_attacker_is_still_offered_to_a_single_blocker() {
    let mut state = combat_state(Step::DeclareBlockers);
    put(&mut state, 1, P0, creature("Menacer", KeywordSet::MENACE));
    put(&mut state, 2, P0, creature("Bear", KeywordSet::empty()));
    put(&mut state, 10, P1, creature("Elk", KeywordSet::empty()));
    with_combat(&mut state, &[1, 2]);
    assert_eq!(
        Engine::new(state).legal_blockers(P1),
        vec![BlockerEntry { blocker: ObjectId(10), attackers: vec![ObjectId(2)] }]
    );
}

#[test]
fn the_blockers_must_add_up_to_two_for_a_menace_attacker() {
    let board = || menace_board(&[(10, KeywordSet::empty()), (11, KeywordSet::empty()), (12, KeywordSet::empty())]);
    let mut engine = Engine::new(board());
    assert_illegal(assert_action_rejected(&mut engine, "one blocker", declare_blockers(&[(10, 1)])));
    for blocks in [&[(10, 1), (11, 1)][..], &[(10, 1), (11, 1), (12, 1)][..], &[][..]] {
        Engine::new(board()).submit_action(declare_blockers(blocks)).unwrap_or_else(|e| panic!("{blocks:?}: {e:?}"));
    }
}

#[test]
fn two_menace_attackers_cannot_share_the_same_two_blockers() {
    let mut state = combat_state(Step::DeclareBlockers);
    put(&mut state, 1, P0, creature("Menacer", KeywordSet::MENACE));
    put(&mut state, 2, P0, creature("Menacer", KeywordSet::MENACE));
    put(&mut state, 10, P1, creature("Elk", KeywordSet::empty()));
    put(&mut state, 11, P1, creature("Elk", KeywordSet::empty()));
    with_combat(&mut state, &[1, 2]);
    let mut engine = Engine::new(state);
    assert_eq!(engine.legal_blockers(P1).len(), 2, "each blocker is listed for both attackers");
    assert_illegal(assert_action_rejected(&mut engine, "one blocker each", declare_blockers(&[(10, 1), (11, 2)])));
}

/// Every way to assign three defenders to two attackers (or to nobody) is
/// checked against the entries: a declaration that uses a pair the entries
/// left out is refused, and every listed pair is part of some accepted one.
#[test]
fn the_menace_filter_removes_exactly_the_pairs_no_declaration_can_use() {
    let board = || {
        let mut state = combat_state(Step::DeclareBlockers);
        put(&mut state, 1, P0, creature("Menacer", KeywordSet::MENACE));
        put(&mut state, 2, P0, creature("Flying Menacer", KeywordSet::MENACE | KeywordSet::FLYING));
        put(&mut state, 3, P0, creature("Bear", KeywordSet::empty()));
        put(&mut state, 10, P1, creature("Elk", KeywordSet::empty()));
        put(&mut state, 11, P1, creature("Elk", KeywordSet::empty()));
        put(&mut state, 12, P1, creature("Spider", KeywordSet::REACH));
        with_combat(&mut state, &[1, 2, 3]);
        state
    };
    let listed: Vec<(u64, u64)> = Engine::new(board())
        .legal_blockers(P1)
        .iter()
        .flat_map(|entry| entry.attackers.iter().map(move |a| (entry.blocker.0, a.0)))
        .collect();

    let blockers = [10u64, 11, 12];
    let choices = [None, Some(1u64), Some(2), Some(3)];
    let mut accepted: Vec<Vec<(u64, u64)>> = Vec::new();
    let mut seen_pairs = std::collections::HashSet::new();
    for a in choices {
        for b in choices {
            for c in choices {
                let blocks: Vec<(u64, u64)> =
                    blockers.iter().zip([a, b, c]).filter_map(|(&blocker, attacker)| attacker.map(|at| (blocker, at))).collect();
                let result = Engine::new(board()).submit_action(declare_blockers(&blocks));
                if blocks.iter().any(|pair| !listed.contains(pair)) {
                    assert!(result.is_err(), "{blocks:?} uses a pair the entries left out, yet it was accepted");
                }
                if result.is_ok() {
                    seen_pairs.extend(blocks.iter().copied());
                    accepted.push(blocks);
                }
            }
        }
    }
    assert!(!accepted.is_empty());
    for pair in &listed {
        assert!(seen_pairs.contains(pair), "{pair:?} is listed but no accepted declaration uses it");
    }
    assert!(!listed.iter().any(|&(_, attacker)| attacker == 2), "only the spider could block the flying menace attacker");
}
