//! Phase 5 vertical slice: CR 601.2c/115/205.4b negative target filters —
//! `TargetFilter::Object`'s own long-standing TODO ("target creature you
//! don't control that isn't a Wall"), left open through every prior
//! targeting slice (subtype/supertype filters, "another target", scripted
//! predicates, "N or more" counts).
//!
//! `TargetFilter::Object` gains `excluded_card_types: Option<CardTypeSet>`
//! ("target nonland permanent") and `excluded_subtypes: Option<Vec<Subtype>>`
//! ("target creature that isn't a Wall") — the negation of the existing
//! `card_types`/`subtypes` "or" matches, checked independently so a filter
//! can combine a positive `card_types` with an `excluded_subtypes` (the
//! worked example above: `card_types: Creature, excluded_subtypes: [Wall],
//! controller: Opponent`).

use std::collections::HashMap;

use stackfish_engine::engine::ActionError;
use stackfish_engine::commands::Rejected;
use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, hand_battlefield_graveyard_zones, two_player_state as base_two_player_state};

fn creature_definition(id: u64, name: &str, subtypes: Vec<Subtype>) -> CardDefinition {
    CardDefinition {
        definition_id: id,
        name: name.to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::CREATURE,
        subtypes,
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

fn land_definition(id: u64, name: &str) -> CardDefinition {
    CardDefinition {
        definition_id: id,
        name: name.to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::LAND,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("land".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

/// "Test Bolt deals 1 damage to target nonland permanent."
fn bolt_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 20,
        name: "Test Bolt".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::INSTANT,
        subtypes: Vec::new(),
        oracle_text: "Test Bolt deals 1 damage to target nonland permanent.".to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_bolt".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: Some(TargetSpec::Primitive(TargetRequirement {
            filter: TargetFilter::Object { mana_value: None,
                card_types: None,
                excluded_card_types: Some(CardTypeSet::LAND),
                subtypes: None,
                excluded_subtypes: None,
                supertypes: None, excluded_supertypes: None,
                controller: None,
                zone: None,
            },
            count: 1,
            up_to: false,
            at_least: false,
            exclude_source: false,
        })),
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: Some(AbilityEffect::Damage(1)),
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

/// "Test Purge deals 1 damage to target creature that isn't a Wall."
fn purge_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 21,
        name: "Test Purge".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::INSTANT,
        subtypes: Vec::new(),
        oracle_text: "Test Purge deals 1 damage to target creature that isn't a Wall.".to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_purge".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: Some(TargetSpec::Primitive(TargetRequirement {
            filter: TargetFilter::Object { mana_value: None,
                card_types: Some(CardTypeSet::CREATURE),
                excluded_card_types: None,
                subtypes: None,
                excluded_subtypes: Some(vec![Subtype("Wall".to_string())]),
                supertypes: None, excluded_supertypes: None,
                controller: None,
                zone: None,
            },
            count: 1,
            up_to: false,
            at_least: false,
            exclude_source: false,
        })),
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: Some(AbilityEffect::Damage(1)),
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

/// This repo's own worked example for the negative-filter gap: "target
/// creature you don't control that isn't a Wall."
fn smite_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 22,
        name: "Test Smite".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::INSTANT,
        subtypes: Vec::new(),
        oracle_text: "Test Smite deals 1 damage to target creature you don't control that isn't a Wall."
            .to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_smite".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: Some(TargetSpec::Primitive(TargetRequirement {
            filter: TargetFilter::Object { mana_value: None,
                card_types: Some(CardTypeSet::CREATURE),
                excluded_card_types: None,
                subtypes: None,
                excluded_subtypes: Some(vec![Subtype("Wall".to_string())]),
                supertypes: None, excluded_supertypes: None,
                controller: Some(FilterController::Opponent),
                zone: None,
            },
            count: 1,
            up_to: false,
            at_least: false,
            exclude_source: false,
        })),
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: Some(AbilityEffect::Damage(1)),
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

fn try_cast_spell(engine: &mut Engine, player: PlayerId, card: ObjectId, targets: Vec<Target>) -> Result<(), ActionError> {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::CastSpell { card, modes: Vec::new(), targets, x_value: None, alternative_cost: None , pay_buyback: false},
        })
        
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

fn damage_marked_on(engine: &Engine, id: ObjectId) -> u32 {
    engine.state().object(id).map(|o| o.damage_marked).unwrap_or(0)
}

fn setup(spell: CardDefinition, permanents: Vec<(ObjectId, CardDefinition, PlayerId)>) -> (GameState, ObjectId) {
    let p0 = PlayerId(0);
    let mut state = two_player_state();
    for (id, def, owner) in permanents {
        state.objects.insert(id, card_object(id, def, owner, ZoneKind::Battlefield));
        state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(id);
    }
    let card = ObjectId(1000);
    state.objects.insert(card, card_object(card, spell, p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);
    (state, card)
}

#[test]
fn nonland_filter_rejects_a_land_target() {
    let p0 = PlayerId(0);
    let land = ObjectId(0);
    let (state, card) = setup(bolt_definition(), vec![(land, land_definition(1, "Test Land"), p0)]);

    let mut engine = Engine::new(state);
    let err = try_cast_spell(&mut engine, p0, card, vec![Target::Object(land)]).unwrap_err();

    assert!(matches!(err, ActionError::Rejected(Rejected::InvalidTarget)), "got {err:?}");
}

#[test]
fn nonland_filter_accepts_a_creature_target() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let bear = ObjectId(0);
    let (state, card) = setup(bolt_definition(), vec![(bear, creature_definition(1, "Bear", Vec::new()), p1)]);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(bear)]);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    assert_eq!(damage_marked_on(&engine, bear), 1);
}

#[test]
fn isnt_a_wall_filter_rejects_a_wall_target() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let wall = ObjectId(0);
    let (state, card) =
        setup(purge_definition(), vec![(wall, creature_definition(1, "Test Wall", vec![Subtype("Wall".to_string())]), p1)]);

    let mut engine = Engine::new(state);
    let err = try_cast_spell(&mut engine, p0, card, vec![Target::Object(wall)]).unwrap_err();

    assert!(matches!(err, ActionError::Rejected(Rejected::InvalidTarget)), "got {err:?}");
}

#[test]
fn isnt_a_wall_filter_accepts_a_non_wall_creature_target() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let bear = ObjectId(0);
    let (state, card) = setup(purge_definition(), vec![(bear, creature_definition(1, "Bear", Vec::new()), p1)]);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(bear)]);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    assert_eq!(damage_marked_on(&engine, bear), 1);
}

#[test]
fn combined_filter_rejects_your_own_creature() {
    let p0 = PlayerId(0);
    let bear = ObjectId(0);
    let (state, card) = setup(smite_definition(), vec![(bear, creature_definition(1, "Bear", Vec::new()), p0)]);

    let mut engine = Engine::new(state);
    let err = try_cast_spell(&mut engine, p0, card, vec![Target::Object(bear)]).unwrap_err();

    assert!(matches!(err, ActionError::Rejected(Rejected::InvalidTarget)),
        "\"you don't control\" must still reject your own creature - got {err:?}");
}

#[test]
fn combined_filter_rejects_an_opponents_wall() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let wall = ObjectId(0);
    let (state, card) =
        setup(smite_definition(), vec![(wall, creature_definition(1, "Test Wall", vec![Subtype("Wall".to_string())]), p1)]);

    let mut engine = Engine::new(state);
    let err = try_cast_spell(&mut engine, p0, card, vec![Target::Object(wall)]).unwrap_err();

    assert!(matches!(err, ActionError::Rejected(Rejected::InvalidTarget)),
        "\"isn't a Wall\" must still reject an opponent's Wall - got {err:?}");
}

#[test]
fn combined_filter_accepts_an_opponents_non_wall_creature() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let bear = ObjectId(0);
    let (state, card) = setup(smite_definition(), vec![(bear, creature_definition(1, "Bear", Vec::new()), p1)]);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(bear)]);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    assert_eq!(damage_marked_on(&engine, bear), 1);
}
