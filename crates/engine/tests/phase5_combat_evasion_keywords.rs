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

use std::collections::{HashMap, VecDeque};

use stackfish_engine::commands::CommandError as EngineCommandError;
use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::*;

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

fn declare_blockers(engine: &mut Engine, player: PlayerId, blocks: Vec<(ObjectId, ObjectId)>) -> StepStatus {
    engine
        .submit_action(PlayerAction { player, kind: PlayerActionKind::DeclareBlockers { blocks } })
        .expect("player holds priority");
    drain(engine)
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

fn attack_and_try_block(attacker_def: CardDefinition, blocker_def: CardDefinition) -> StepStatus {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let blocker = ObjectId(1);
    let mut state = combat_state();
    put_on_battlefield(&mut state, permanent(attacker, attacker_def, p0));
    put_on_battlefield(&mut state, permanent(blocker, blocker_def, p1));

    let mut engine = Engine::new(state);
    declare_attackers(&mut engine, p0, vec![(attacker, Target::Player(p1))]);
    pass_to_declare_blockers(&mut engine);
    declare_blockers(&mut engine, p1, vec![(blocker, attacker)])
}

fn assert_legal_block(status: StepStatus) {
    assert!(matches!(status, StepStatus::AwaitingAction { .. }), "expected a legal block, got {status:?}");
}

fn assert_illegal_block(status: StepStatus) {
    assert!(matches!(status, StepStatus::Error(EngineCommandError::IllegalAction(_))), "expected an illegal block, got {status:?}");
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
