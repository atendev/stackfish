//! Phase 5 vertical slice: real multi-target effect resolution + CR 608.2b
//! partial fizzle. Pulled from the "Known unresolved gaps" list rather than
//! its own numbered backlog item.
//!
//! Found while scoping: every `AbilityEffect` resolution arm in `engine.rs`
//! (`Damage`, `Sacrifice`, `Destroy`, `Exile`, `Counters`) only ever read
//! `top.targets.first()` - already documented at each site as a deliberate
//! narrowing, but bigger than it first looked: a `TargetRequirement` with
//! `count: 2+` validated fine at cast time, but only ever affected the first
//! chosen target at resolution. This slice makes every legal target receive
//! the effect independently (CR 601.2d's "divide as you choose" stays
//! unmodeled - each legal target gets the full stated amount, not a split),
//! and makes CR 608.2b's actual partial-fizzle text real: a spell/ability
//! with some (not all) targets now illegal still resolves, just skipping the
//! individually-illegal ones, rather than the previous all-or-nothing check.

use std::collections::HashMap;

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
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

/// "Test Doom Blast deals 2 damage to each of two target creatures."
fn doom_blast_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 20,
        name: "Test Doom Blast".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::INSTANT,
        subtypes: Vec::new(),
        oracle_text: "Test Doom Blast deals 2 damage to each of two target creatures.".to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_doom_blast".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: Some(TargetSpec::Primitive(TargetRequirement {
            filter: TargetFilter::Object { mana_value: None, excluded_card_types: None, excluded_subtypes: None, card_types: Some(CardTypeSet::CREATURE), subtypes: None, supertypes: None, excluded_supertypes: None, controller: None, zone: None },
            count: 2,
            up_to: false, at_least: false,
            exclude_source: false,
        })),
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: Some(AbilityEffect::Damage(2)),
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

/// "Test Slay Two: Destroy two target creatures."
fn slay_two_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 21,
        name: "Test Slay Two".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::INSTANT,
        subtypes: Vec::new(),
        oracle_text: "Destroy two target creatures.".to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_slay_two".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: Some(TargetSpec::Primitive(TargetRequirement {
            filter: TargetFilter::Object { mana_value: None, excluded_card_types: None, excluded_subtypes: None, card_types: Some(CardTypeSet::CREATURE), subtypes: None, supertypes: None, excluded_supertypes: None, controller: None, zone: None },
            count: 2,
            up_to: false, at_least: false,
            exclude_source: false,
        })),
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: Some(AbilityEffect::Destroy),
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
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
            kind: PlayerActionKind::CastSpell { card, modes: Vec::new(), targets, x_value: None, alternative_cost: None , pay_buyback: false},
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

fn damage_dealt_events(engine: &Engine) -> Vec<(Target, u32)> {
    engine
        .state()
        .ledger
        .iter()
        .filter_map(|e| match e.kind {
            GameEventKind::DamageDealt { target, amount, .. } => Some((target, amount)),
            _ => None,
        })
        .collect()
}

// ---------------------------------------------------------------------
// Damage - CR 601.2d (no division) + CR 608.2b (partial fizzle).
// ---------------------------------------------------------------------

#[test]
fn two_target_damage_spell_deals_full_amount_to_each_independently() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let target_a = ObjectId(0);
    let target_b = ObjectId(1);
    state.objects.insert(target_a, card_object(target_a, creature_definition(1, "Bear A"), p1, ZoneKind::Battlefield));
    state.objects.insert(target_b, card_object(target_b, creature_definition(2, "Bear B"), p1, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(target_a);
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(target_b);
    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, doom_blast_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(target_a), Target::Object(target_b)]);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    assert_eq!(engine.state().object(target_a).unwrap().damage_marked, 2, "each target takes the full amount, not a split");
    assert_eq!(engine.state().object(target_b).unwrap().damage_marked, 2);
    let mut events = damage_dealt_events(&engine);
    events.sort_by_key(|(t, _)| match t {
        Target::Object(id) => id.0,
        Target::Player(p) => p.0 as u64,
        Target::Ability(id) => id.0,
    });
    assert_eq!(events, vec![(Target::Object(target_a), 2), (Target::Object(target_b), 2)]);
}

#[test]
fn two_target_damage_spell_skips_only_the_target_that_became_illegal() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let target_a = ObjectId(0);
    let target_b = ObjectId(1);
    state.objects.insert(target_a, card_object(target_a, creature_definition(1, "Bear A"), p1, ZoneKind::Battlefield));
    state.objects.insert(target_b, card_object(target_b, creature_definition(2, "Bear B"), p1, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(target_a);
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(target_b);
    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, doom_blast_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(target_a), Target::Object(target_b)]);
    pay_free_and_drain(&mut engine);

    // CR 608.2b: target_b leaves the battlefield before this resolves - only
    // one of the two targets is now illegal.
    engine.state_mut().zone_mut(ZoneKind::Battlefield, None).unwrap().remove(target_b);
    engine.state_mut().objects.remove(&target_b);

    let status = pass_both_and_drain(&mut engine, p0, p1);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));

    assert!(
        !engine.state().ledger.iter().any(|e| matches!(e.kind, GameEventKind::Fizzled { .. })),
        "CR 608.2b: not every target is illegal, so the spell must still resolve, not fizzle"
    );
    assert_eq!(engine.state().object(target_a).unwrap().damage_marked, 2, "the still-legal target is unaffected by the other's illegality");
    assert_eq!(damage_dealt_events(&engine), vec![(Target::Object(target_a), 2)], "the illegal target gets no DamageDealt event at all");
}

#[test]
fn two_target_damage_spell_fully_fizzles_when_both_targets_become_illegal() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let target_a = ObjectId(0);
    let target_b = ObjectId(1);
    state.objects.insert(target_a, card_object(target_a, creature_definition(1, "Bear A"), p1, ZoneKind::Battlefield));
    state.objects.insert(target_b, card_object(target_b, creature_definition(2, "Bear B"), p1, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(target_a);
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(target_b);
    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, doom_blast_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(target_a), Target::Object(target_b)]);
    pay_free_and_drain(&mut engine);

    // Both targets leave the battlefield before this resolves.
    engine.state_mut().zone_mut(ZoneKind::Battlefield, None).unwrap().remove(target_a);
    engine.state_mut().objects.remove(&target_a);
    engine.state_mut().zone_mut(ZoneKind::Battlefield, None).unwrap().remove(target_b);
    engine.state_mut().objects.remove(&target_b);

    pass_both_and_drain(&mut engine, p0, p1);

    assert!(engine.state().stack.is_empty(), "fizzled spell still leaves the stack");
    assert!(damage_dealt_events(&engine).is_empty(), "a fully-fizzled spell never resolves its effect at all");
    let fizzled = engine.state().ledger.iter().any(|e| matches!(e.kind, GameEventKind::Fizzled { .. }));
    assert!(fizzled, "CR 608.2b: every target illegal - the spell must not resolve at all");
}

// ---------------------------------------------------------------------
// Destroy - same independent-per-legal-target pattern as Damage.
// ---------------------------------------------------------------------

#[test]
fn two_target_destroy_spell_destroys_each_independently() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let target_a = ObjectId(0);
    let target_b = ObjectId(1);
    state.objects.insert(target_a, card_object(target_a, creature_definition(1, "Bear A"), p1, ZoneKind::Battlefield));
    state.objects.insert(target_b, card_object(target_b, creature_definition(2, "Bear B"), p1, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(target_a);
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(target_b);
    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, slay_two_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(target_a), Target::Object(target_b)]);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    assert!(engine.state().object(target_a).is_none(), "target_a was destroyed");
    assert!(engine.state().object(target_b).is_none(), "target_b was destroyed too, independently");
    assert_eq!(engine.state().zone(ZoneKind::Graveyard, Some(p1)).unwrap().contents.len(), 2);
}

#[test]
fn two_target_destroy_spell_skips_the_target_that_already_left_the_battlefield() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let target_a = ObjectId(0);
    let target_b = ObjectId(1);
    state.objects.insert(target_a, card_object(target_a, creature_definition(1, "Bear A"), p1, ZoneKind::Battlefield));
    state.objects.insert(target_b, card_object(target_b, creature_definition(2, "Bear B"), p1, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(target_a);
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(target_b);
    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, slay_two_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(target_a), Target::Object(target_b)]);
    pay_free_and_drain(&mut engine);

    engine.state_mut().zone_mut(ZoneKind::Battlefield, None).unwrap().remove(target_b);
    engine.state_mut().objects.remove(&target_b);

    let status = pass_both_and_drain(&mut engine, p0, p1);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));

    assert!(
        !engine.state().ledger.iter().any(|e| matches!(e.kind, GameEventKind::Fizzled { .. })),
        "one legal target remains - must resolve, not fizzle"
    );
    assert!(engine.state().object(target_a).is_none(), "the still-legal target was destroyed");
    assert_eq!(engine.state().zone(ZoneKind::Graveyard, Some(p1)).unwrap().contents.len(), 1, "only one death, the other target was already gone");
}
