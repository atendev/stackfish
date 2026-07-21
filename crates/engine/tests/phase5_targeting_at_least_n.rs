//! Phase 5 vertical slice: CR 601.2c/115.6 "N or more" target counts —
//! `TargetRequirement`'s third count shape alongside exact-count and "up to
//! N" (`phase5_targeting_up_to_n.rs`). `TargetRequirement::at_least` lets a
//! caster choose `count` or more targets, with no upper bound of its own
//! (capped only by how many legal candidates the caster actually submits,
//! same as every other shape — `commands::validate_targets` never
//! enumerates "all possible legal targets" itself).
//!
//! No resolution-time fizzle-logic change needed — `ResolveStackTop`'s
//! fizzle gate was already fixed to key off `!top.targets.is_empty()`
//! rather than the requirement's own `count` field back in
//! `targeting-up-to-n`, so it's shape-agnostic for free.

use std::collections::HashMap;

use stackfish_engine::commands::CommandError;
use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, hand_battlefield_graveyard_zones, two_player_state as base_two_player_state};

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

/// "Test Volley deals 1 damage to each of two or more target creatures."
fn volley_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 20,
        name: "Test Volley".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::INSTANT,
        subtypes: Vec::new(),
        oracle_text: "Test Volley deals 1 damage to each of two or more target creatures.".to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_volley".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: Some(TargetSpec::Primitive(TargetRequirement {
            filter: TargetFilter::Object { excluded_card_types: None, excluded_subtypes: None, card_types: Some(CardTypeSet::CREATURE), subtypes: None, supertypes: None, excluded_supertypes: None, controller: None, zone: None },
            count: 2,
            up_to: false,
            at_least: true,
            exclude_source: false,
        })),
        alternative_costs: Vec::new(),
        effect: Some(AbilityEffect::Damage(1)),
        modes: Vec::new(),
    }
}

fn two_player_state() -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        step: Step::PreCombatMain,
        last_active_turn_number: HashMap::from([(p0, 1), (p1, 1)]),
        ..base_two_player_state(hand_battlefield_graveyard_zones(p0, p1))
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

/// `n` creatures on the battlefield (`p1`'s), a "Test Volley" card in `p0`'s
/// hand.
fn setup_with_creatures(n: u32) -> (GameState, Vec<ObjectId>, ObjectId) {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let mut state = two_player_state();
    let mut ids = Vec::new();
    for i in 0..n {
        let id = ObjectId(i as u64);
        state.objects.insert(id, card_object(id, creature_definition((i + 1) as u64, &format!("Bear {i}")), p1, ZoneKind::Battlefield));
        state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(id);
        ids.push(id);
    }
    let card = ObjectId(1000);
    state.objects.insert(card, card_object(card, volley_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);
    (state, ids, card)
}

#[test]
fn casting_with_exactly_the_minimum_is_legal() {
    let (state, targets, card) = setup_with_creatures(2);
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, targets.iter().map(|id| Target::Object(*id)).collect());
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    assert!(!fizzled(&engine));
    for id in &targets {
        assert_eq!(damage_marked_on(&engine, *id), 1);
    }
}

#[test]
fn casting_with_more_than_the_minimum_is_legal() {
    let (state, targets, card) = setup_with_creatures(3);
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, targets.iter().map(|id| Target::Object(*id)).collect());
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    assert!(!fizzled(&engine));
    for id in &targets {
        assert_eq!(damage_marked_on(&engine, *id), 1);
    }
}

#[test]
fn casting_with_fewer_than_the_minimum_is_rejected() {
    let (state, targets, card) = setup_with_creatures(2);
    let p0 = PlayerId(0);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(targets[0])]);
    let status = drain(&mut engine);

    assert!(matches!(status, StepStatus::Error(CommandError::InvalidTarget)), "got {status:?}");
    assert_eq!(engine.state().object(card).unwrap().zone, ZoneKind::Hand, "cast never happened");
}

#[test]
fn casting_with_zero_targets_is_rejected() {
    let (state, _targets, card) = setup_with_creatures(2);
    let p0 = PlayerId(0);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, Vec::new());
    let status = drain(&mut engine);

    assert!(
        matches!(status, StepStatus::Error(CommandError::InvalidTarget)),
        "\"N or more\" with N=2 still requires at least 2, unlike \"up to N\" which allows zero - got {status:?}"
    );
}

#[test]
fn casting_with_a_duplicate_target_is_rejected() {
    let (state, targets, card) = setup_with_creatures(2);
    let p0 = PlayerId(0);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(targets[0]), Target::Object(targets[0])]);
    let status = drain(&mut engine);

    assert!(matches!(status, StepStatus::Error(CommandError::InvalidTarget)), "got {status:?}");
}

#[test]
fn one_of_several_chosen_targets_becoming_illegal_partially_fizzles() {
    let (state, targets, card) = setup_with_creatures(2);
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, targets.iter().map(|id| Target::Object(*id)).collect());
    pay_free_and_drain(&mut engine);

    engine.state_mut().zone_mut(ZoneKind::Battlefield, None).unwrap().remove(targets[0]);
    engine.state_mut().objects.remove(&targets[0]);

    pass_both_and_drain(&mut engine, p0, p1);

    assert!(!fizzled(&engine), "one of two targets is still legal - CR 608.2b: the spell still resolves against it");
    assert_eq!(damage_marked_on(&engine, targets[1]), 1);
}
