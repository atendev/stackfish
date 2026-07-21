//! Phase 5 vertical slice: CR 601.2c/115.6 "up to N" target counts. Pulled
//! from the "Known unresolved gaps" list — the `multi-target-resolution`
//! slice's own UNRESOLVED section named this as the natural next follow-up.
//!
//! `TargetRequirement::up_to` lets a caster choose anywhere from 0 to
//! `count` targets instead of exactly `count`. Also fixes a small, real bug
//! found while designing this: `ResolveStackTop`'s fizzle check used to gate
//! on `req.count > 0` as a proxy for "were any targets actually chosen" -
//! that coincided with reality for the old exact-count-only requirement
//! (`validate_targets` always forced `targets.len() == count`), but breaks
//! once "up to N" lets a caster legally choose zero targets while `count`
//! itself stays nonzero. Fixed by gating on `!top.targets.is_empty()`
//! instead - see `zero_chosen_targets_never_produces_a_fizzled_event` below.

use std::collections::{HashMap, VecDeque};

use stackfish_engine::commands::CommandError;
use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::*;

fn creature_definition(definition_id: u64, name: &str) -> CardDefinition {
    CardDefinition {
        definition_id,
        name: name.to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::CREATURE,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: Some(PTValue::Fixed(2)),
        toughness: Some(PTValue::Fixed(5)),
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("bear".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        effect: None,
        modes: Vec::new(),
    }
}

/// "Test Volley deals 1 damage to each of up to two target creatures."
fn volley_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 20,
        name: "Test Volley".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::INSTANT,
        subtypes: Vec::new(),
        oracle_text: "Test Volley deals 1 damage to each of up to two target creatures.".to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_volley".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: Some(TargetSpec::Primitive(TargetRequirement {
            filter: TargetFilter::Object { excluded_card_types: None, excluded_subtypes: None, card_types: Some(CardTypeSet::CREATURE), subtypes: None, supertypes: None, excluded_supertypes: None, controller: None, zone: None },
            count: 2,
            up_to: true, at_least: false,
            exclude_source: false,
        })),
        alternative_costs: Vec::new(),
        effect: Some(AbilityEffect::Damage(1)),
        modes: Vec::new(),
    }
}

fn card_object(id: ObjectId, definition: CardDefinition, owner: PlayerId, zone: ZoneKind) -> GameObject {
    GameObject {
        object_id: id,
        copiable_override: None,
        lineage: ObjectLineage { previous: None },
        definition,
        owner,
        controller: owner,
        zone,
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

fn two_player_state() -> GameState {
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
        phase: Phase::Main,
        step: Step::PreCombatMain,
        priority_player: p0,
        consecutive_passes: 0,
        lands_played_this_turn: HashMap::new(),
        combat: None,
        last_active_turn_number: HashMap::from([(p0, 1), (p1, 1)]),
        objects: HashMap::new(),
        zones: vec![
            Zone { kind: ZoneKind::Hand, owner: Some(p0), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Hand, owner: Some(p1), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Battlefield, owner: None, contents: VecDeque::new() },
            Zone { kind: ZoneKind::Graveyard, owner: Some(p0), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Graveyard, owner: Some(p1), contents: VecDeque::new() },
        ],
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

fn cast_spell(engine: &mut Engine, player: PlayerId, card: ObjectId, targets: Vec<Target>) {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::CastSpell { card, modes: Vec::new(), targets, x_value: None, alternative_cost: None },
        })
        .expect("player holds priority, spell is in hand");
}

fn pay_free_and_drain(engine: &mut Engine) -> StepStatus {
    let status = drain(engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }), "expected a pause for the cast's (free) cost, got {status:?}");
    engine.submit_payment(PaymentPlan { payments: Vec::new() }).expect("empty plan pays a {0} cost");
    drain(engine)
}

fn pass_both_and_drain(engine: &mut Engine, p0: PlayerId, p1: PlayerId) -> StepStatus {
    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine);
    engine.submit_action(PlayerAction { player: p1, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine)
}

fn fizzled(engine: &Engine) -> bool {
    engine.state().ledger.iter().any(|e| matches!(e.kind, GameEventKind::Fizzled { .. }))
}

fn damage_marked_on(engine: &Engine, id: ObjectId) -> u32 {
    engine.state().object(id).map(|o| o.damage_marked).unwrap_or(0)
}

/// Two creatures on the battlefield (`p1`'s), a "Test Volley" card in `p0`'s
/// hand - the common setup every test below starts from.
fn setup_with_two_creatures() -> (GameState, ObjectId, ObjectId, ObjectId) {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let mut state = two_player_state();
    let target_a = ObjectId(0);
    let target_b = ObjectId(1);
    state.objects.insert(target_a, card_object(target_a, creature_definition(1, "Bear A"), p1, ZoneKind::Battlefield));
    state.objects.insert(target_b, card_object(target_b, creature_definition(2, "Bear B"), p1, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(target_a);
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(target_b);
    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, volley_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);
    (state, target_a, target_b, card)
}

#[test]
fn casting_with_zero_targets_is_legal_under_up_to_n() {
    let (state, _target_a, _target_b, card) = setup_with_two_creatures();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, Vec::new());
    pay_free_and_drain(&mut engine);
    let status = pass_both_and_drain(&mut engine, p0, p1);

    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    // CR 400.7: resolution mints a fresh id moving stack->graveyard, so the
    // original `card` id no longer resolves - check the graveyard's
    // contents instead of the old id.
    assert!(engine.state().object(card).is_none());
    assert_eq!(
        engine.state().zone(ZoneKind::Graveyard, Some(p0)).unwrap().contents.len(),
        1,
        "the spell still resolved and moved to the graveyard, not stuck on the stack"
    );
}

#[test]
fn zero_chosen_targets_never_produces_a_fizzled_event() {
    let (state, _target_a, _target_b, card) = setup_with_two_creatures();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, Vec::new());
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    assert!(!fizzled(&engine), "choosing zero targets under 'up to N' is legal - it must resolve, not fizzle, even though req.count is 2");
}

#[test]
fn casting_with_one_of_two_allowed_targets_is_legal() {
    let (state, target_a, _target_b, card) = setup_with_two_creatures();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(target_a)]);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    assert!(!fizzled(&engine));
    assert_eq!(damage_marked_on(&engine, target_a), 1);
}

#[test]
fn casting_with_the_maximum_allowed_targets_is_legal() {
    let (state, target_a, target_b, card) = setup_with_two_creatures();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(target_a), Target::Object(target_b)]);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    assert!(!fizzled(&engine));
    assert_eq!(damage_marked_on(&engine, target_a), 1);
    assert_eq!(damage_marked_on(&engine, target_b), 1);
}

#[test]
fn casting_with_more_than_the_maximum_is_rejected() {
    let (mut state, target_a, target_b, card) = setup_with_two_creatures();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let target_c = ObjectId(3);
    state.objects.insert(target_c, card_object(target_c, creature_definition(3, "Bear C"), p1, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(target_c);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(target_a), Target::Object(target_b), Target::Object(target_c)]);
    let status = drain(&mut engine);

    assert!(matches!(status, StepStatus::Error(CommandError::InvalidTarget)), "got {status:?}");
    assert_eq!(engine.state().object(card).unwrap().zone, ZoneKind::Hand, "cast never happened");
}

#[test]
fn casting_with_a_duplicate_target_is_rejected() {
    let (state, target_a, _target_b, card) = setup_with_two_creatures();
    let p0 = PlayerId(0);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(target_a), Target::Object(target_a)]);
    let status = drain(&mut engine);

    assert!(matches!(status, StepStatus::Error(CommandError::InvalidTarget)), "got {status:?}");
}

#[test]
fn the_lone_chosen_target_becoming_illegal_fully_fizzles() {
    let (state, target_a, _target_b, card) = setup_with_two_creatures();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(target_a)]);
    pay_free_and_drain(&mut engine);

    engine.state_mut().zone_mut(ZoneKind::Battlefield, None).unwrap().remove(target_a);
    engine.state_mut().objects.remove(&target_a);

    pass_both_and_drain(&mut engine, p0, p1);

    assert!(fizzled(&engine), "the only chosen target is now illegal - CR 608.2b: the spell doesn't resolve at all");
}
