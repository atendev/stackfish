//! Phase 5 vertical slice: CR 702's remaining "who can block me" evasion
//! keywords beyond Flying/Reach/Menace (already covered by
//! `phase5_keyword_primitives.rs`) - Fear (702.36), Intimidate (702.13),
//! Shadow (702.28), Horsemanship (702.31). Same shape as Flying/Menace:
//! a `KeywordSet` bit checked in `commands::ActivateAbility`'s
//! `DeclareBlockers` handler, through `effective_keywords`/
//! `effective_characteristics` so a continuous-effect-granted instance
//! (not just the printed one) is honored.
//!
//! `KeywordSet` widened from `u16` to `u32` to make room - every bit 0-15
//! was already in use.
//!
//! Deliberately out of scope, named rather than guessed at: Landwalk (CR
//! 702.14 - checks the *defending player's own battlefield* for a land
//! type, not the blocker's own characteristics; parametrized by land type,
//! closer to protection's dynamic-field shape than a plain bit) and Banding
//! (CR 702.22 - a rules-heavy legacy mechanic that overrides combat-damage
//! assignment order, essentially its own vertical slice). Flanking/Rampage
//! are triggered abilities, not blocking restrictions - a different gap.

use std::collections::HashMap;

use stackfish_engine::engine::ActionError;
use stackfish_engine::commands::Rejected;
use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, zones, two_player_state as base_two_player_state};

fn creature_definition(name: &str, keywords: KeywordSet, color: Option<ColorSet>, extra_types: CardTypeSet) -> CardDefinition {
    CardDefinition {
        definition_id: 1,
        name: name.to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: color,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::CREATURE | extra_types,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: Some(PTValue::Fixed(2)),
        toughness: Some(PTValue::Fixed(2)),
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

fn declare_blockers(engine: &mut Engine, player: PlayerId, blocks: Vec<(ObjectId, ObjectId)>) -> Result<StepStatus, ActionError> {
    engine
        .submit_action(PlayerAction { player, kind: PlayerActionKind::DeclareBlockers { blocks } })
        ?;
    Ok(drain(engine))
}

fn pass_to_declare_blockers(engine: &mut Engine) {
    for _ in 0..10 {
        if engine.state().step == Step::DeclareBlockers {
            return;
        }
        let active = engine.state().priority_player;
        engine.submit_action(PlayerAction { player: active, kind: PlayerActionKind::PassPriority }).expect("priority window");
        drain(engine);
    }
    panic!("never reached DeclareBlockers - state.step = {:?}", engine.state().step);
}

fn attack_and_try_block(attacker_def: CardDefinition, blocker_def: CardDefinition) -> Result<StepStatus, ActionError> {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let blocker = ObjectId(1);
    let mut state = combat_state();
    put_on_battlefield(&mut state, permanent(attacker, attacker_def, p0));
    put_on_battlefield(&mut state, permanent(blocker, blocker_def, p1));

    let mut engine = Engine::new(state);
    declare_attackers(&mut engine, p0, vec![(attacker, Target::Player(p1))]).unwrap();
    pass_to_declare_blockers(&mut engine);
    declare_blockers(&mut engine, p1, vec![(blocker, attacker)])
}

fn assert_legal_block(status: Result<StepStatus, ActionError>) {
    let status = status.expect("the engine should accept the action");    assert!(matches!(status, StepStatus::AwaitingAction { .. }), "expected a legal block, got {status:?}");
}

fn assert_illegal_block(status: Result<StepStatus, ActionError>) {
    assert!(matches!(status, Err(ActionError::Rejected(Rejected::IllegalAction(_)))), "expected an illegal block, got {status:?}");
}

// ---------------------------------------------------------------------
// Fear (CR 702.36)
// ---------------------------------------------------------------------

#[test]
fn fear_attacker_rejects_a_colorless_nonartifact_blocker() {
    let attacker = creature_definition("Fearsome", KeywordSet::FEAR, None, CardTypeSet::empty());
    let blocker = creature_definition("Bear", KeywordSet::empty(), None, CardTypeSet::empty());
    assert_illegal_block(attack_and_try_block(attacker, blocker));
}

#[test]
fn fear_attacker_accepts_a_black_blocker() {
    let attacker = creature_definition("Fearsome", KeywordSet::FEAR, None, CardTypeSet::empty());
    let blocker = creature_definition("Shade", KeywordSet::empty(), Some(ColorSet::BLACK), CardTypeSet::empty());
    assert_legal_block(attack_and_try_block(attacker, blocker));
}

#[test]
fn fear_attacker_accepts_an_artifact_blocker() {
    let attacker = creature_definition("Fearsome", KeywordSet::FEAR, None, CardTypeSet::empty());
    let blocker = creature_definition("Golem", KeywordSet::empty(), None, CardTypeSet::ARTIFACT);
    assert_legal_block(attack_and_try_block(attacker, blocker));
}

// ---------------------------------------------------------------------
// Intimidate (CR 702.13)
// ---------------------------------------------------------------------

#[test]
fn intimidate_attacker_rejects_a_blocker_sharing_no_color() {
    let attacker = creature_definition("Sneak", KeywordSet::INTIMIDATE, Some(ColorSet::BLUE), CardTypeSet::empty());
    let blocker = creature_definition("Bear", KeywordSet::empty(), Some(ColorSet::GREEN), CardTypeSet::empty());
    assert_illegal_block(attack_and_try_block(attacker, blocker));
}

#[test]
fn intimidate_attacker_accepts_a_blocker_sharing_a_color() {
    let attacker = creature_definition("Sneak", KeywordSet::INTIMIDATE, Some(ColorSet::BLUE), CardTypeSet::empty());
    let blocker = creature_definition("Drake", KeywordSet::empty(), Some(ColorSet::BLUE), CardTypeSet::empty());
    assert_legal_block(attack_and_try_block(attacker, blocker));
}

#[test]
fn intimidate_attacker_accepts_an_artifact_blocker() {
    let attacker = creature_definition("Sneak", KeywordSet::INTIMIDATE, Some(ColorSet::BLUE), CardTypeSet::empty());
    let blocker = creature_definition("Golem", KeywordSet::empty(), None, CardTypeSet::ARTIFACT);
    assert_legal_block(attack_and_try_block(attacker, blocker));
}

// ---------------------------------------------------------------------
// Shadow (CR 702.28) - symmetric
// ---------------------------------------------------------------------

#[test]
fn shadow_attacker_cannot_be_blocked_by_a_non_shadow_creature() {
    let attacker = creature_definition("Wraith", KeywordSet::SHADOW, None, CardTypeSet::empty());
    let blocker = creature_definition("Bear", KeywordSet::empty(), None, CardTypeSet::empty());
    assert_illegal_block(attack_and_try_block(attacker, blocker));
}

#[test]
fn non_shadow_attacker_cannot_be_blocked_by_a_shadow_creature() {
    let attacker = creature_definition("Bear", KeywordSet::empty(), None, CardTypeSet::empty());
    let blocker = creature_definition("Wraith", KeywordSet::SHADOW, None, CardTypeSet::empty());
    assert_illegal_block(attack_and_try_block(attacker, blocker));
}

#[test]
fn shadow_attacker_can_be_blocked_by_another_shadow_creature() {
    let attacker = creature_definition("Wraith", KeywordSet::SHADOW, None, CardTypeSet::empty());
    let blocker = creature_definition("Spirit", KeywordSet::SHADOW, None, CardTypeSet::empty());
    assert_legal_block(attack_and_try_block(attacker, blocker));
}

// ---------------------------------------------------------------------
// Horsemanship (CR 702.31)
// ---------------------------------------------------------------------

#[test]
fn horsemanship_attacker_cannot_be_blocked_by_a_non_horsemanship_creature() {
    let attacker = creature_definition("Rider", KeywordSet::HORSEMANSHIP, None, CardTypeSet::empty());
    let blocker = creature_definition("Bear", KeywordSet::empty(), None, CardTypeSet::empty());
    assert_illegal_block(attack_and_try_block(attacker, blocker));
}

#[test]
fn horsemanship_attacker_can_be_blocked_by_a_horsemanship_creature() {
    let attacker = creature_definition("Rider", KeywordSet::HORSEMANSHIP, None, CardTypeSet::empty());
    let blocker = creature_definition("Rider2", KeywordSet::HORSEMANSHIP, None, CardTypeSet::empty());
    assert_legal_block(attack_and_try_block(attacker, blocker));
}

#[test]
fn horsemanship_creature_may_itself_block_anything() {
    let attacker = creature_definition("Bear", KeywordSet::empty(), None, CardTypeSet::empty());
    let blocker = creature_definition("Rider", KeywordSet::HORSEMANSHIP, None, CardTypeSet::empty());
    assert_legal_block(attack_and_try_block(attacker, blocker));
}
