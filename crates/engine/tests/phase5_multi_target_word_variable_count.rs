//! Phase 5 vertical slice: CR 601.2c/608.2b — a multi-target-word spell
//! where one word is variable-count (`up_to`/`at_least`) instead of every
//! word using an exact count. `multi-target-word-spells`'s own notes named
//! this combination as unsupported ("no way to split a flat list
//! unambiguously by a variable-length entry"); it's resolvable as long as
//! *at most one* word is variable — `commands::split_targets_by_requirement`
//! derives that one word's own count as "whatever's left after every
//! exact-count word claims its share," regardless of where the variable
//! word sits in the card's own word order. Two or more variable words stays
//! a real, unsupported ambiguity (see `up_to_and_up_to_two_variable_words_is_rejected`).

use std::collections::HashMap;

use stackfish_engine::commands::CommandError as EngineCommandError;
use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, hand_battlefield_graveyard_zones, two_player_state as base_two_player_state};

fn creature_definition(definition_id: u64, name: &str) -> CardDefinition {
    CardDefinition {
        definition_id,
        name: name.to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: Some(ColorSet::GREEN),
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::CREATURE,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: Some(PTValue::Fixed(2)),
        toughness: Some(PTValue::Fixed(2)),
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey(name.to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn land_definition(definition_id: u64, name: &str) -> CardDefinition {
    CardDefinition {
        definition_id,
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
        script_ref: ScriptReference::ByKey(name.to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn creature_filter() -> TargetFilter {
    TargetFilter::Object {
        card_types: Some(CardTypeSet::CREATURE),
        excluded_card_types: None,
        subtypes: None,
        excluded_subtypes: None,
        supertypes: None,
        excluded_supertypes: None,
        controller: None,
        zone: None,
    }
}

fn land_filter() -> TargetFilter {
    TargetFilter::Object {
        card_types: Some(CardTypeSet::LAND),
        excluded_card_types: None,
        subtypes: None,
        excluded_subtypes: None,
        supertypes: None,
        excluded_supertypes: None,
        controller: None,
        zone: None,
    }
}

fn up_to_two_creatures_word() -> TargetRequirement {
    TargetRequirement { filter: creature_filter(), count: 2, up_to: true, at_least: false, exclude_source: false }
}

fn at_least_one_creature_word() -> TargetRequirement {
    TargetRequirement { filter: creature_filter(), count: 1, up_to: false, at_least: true, exclude_source: false }
}

fn land_word() -> TargetRequirement {
    TargetRequirement { filter: land_filter(), count: 1, up_to: false, at_least: false, exclude_source: false }
}

/// "Test Variable Multi-Word: Destroy up to two target creatures and target
/// land." Variable word first, exact word second.
fn up_to_then_exact_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 20,
        name: "Test Up To Then Exact".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::SORCERY,
        subtypes: Vec::new(),
        oracle_text: "Destroy up to two target creatures and target land.".to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_up_to_then_exact".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: Some(TargetSpec::Multiple(vec![up_to_two_creatures_word(), land_word()])),
        alternative_costs: Vec::new(),
        effect: Some(AbilityEffect::Destroy),
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

/// Same idea, variable word *last* instead of first - "Destroy target land
/// and up to two target creatures." - proves the derivation doesn't depend
/// on word position.
fn exact_then_up_to_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 21,
        name: "Test Exact Then Up To".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::SORCERY,
        subtypes: Vec::new(),
        oracle_text: "Destroy target land and up to two target creatures.".to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_exact_then_up_to".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: Some(TargetSpec::Multiple(vec![land_word(), up_to_two_creatures_word()])),
        alternative_costs: Vec::new(),
        effect: Some(AbilityEffect::Destroy),
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

/// "Destroy at least one target creature and target land." - the `at_least`
/// shape instead of `up_to`.
fn at_least_then_exact_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 22,
        name: "Test At Least Then Exact".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::SORCERY,
        subtypes: Vec::new(),
        oracle_text: "Destroy at least one target creature and target land.".to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_at_least_then_exact".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: Some(TargetSpec::Multiple(vec![at_least_one_creature_word(), land_word()])),
        alternative_costs: Vec::new(),
        effect: Some(AbilityEffect::Destroy),
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

/// Two variable words at once ("up to two target creatures and up to two
/// target lands") - still a real, unsupported ambiguity.
fn two_variable_words_definition() -> CardDefinition {
    let up_to_two_lands_word =
        TargetRequirement { filter: land_filter(), count: 2, up_to: true, at_least: false, exclude_source: false };
    CardDefinition {
        definition_id: 23,
        name: "Test Two Variable Words".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::SORCERY,
        subtypes: Vec::new(),
        oracle_text: "Destroy up to two target creatures and up to two target lands.".to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_two_variable_words".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: Some(TargetSpec::Multiple(vec![up_to_two_creatures_word(), up_to_two_lands_word])),
        alternative_costs: Vec::new(),
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

fn put_on_battlefield(state: &mut GameState, object: GameObject) {
    let id = object.object_id;
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(id);
    state.objects.insert(id, object);
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

#[test]
fn up_to_word_first_claims_the_remainder_after_the_exact_word() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let bear = ObjectId(0);
    let wolf = ObjectId(1);
    let land = ObjectId(2);
    let card = ObjectId(3);
    let mut state = two_player_state();
    put_on_battlefield(&mut state, card_object(bear, creature_definition(1, "Bear"), p1, ZoneKind::Battlefield));
    put_on_battlefield(&mut state, card_object(wolf, creature_definition(2, "Wolf"), p1, ZoneKind::Battlefield));
    put_on_battlefield(&mut state, card_object(land, land_definition(3, "Forest"), p1, ZoneKind::Battlefield));
    state.objects.insert(card, card_object(card, up_to_then_exact_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    // 2 creatures + 1 land = 3 submitted - the up_to word (max 2) claims
    // both creatures, the exact word claims the land.
    cast_spell(&mut engine, p0, card, vec![Target::Object(bear), Target::Object(wolf), Target::Object(land)]);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    assert!(engine.state().object(bear).is_none(), "first up_to target destroyed");
    assert!(engine.state().object(wolf).is_none(), "second up_to target destroyed");
    assert!(engine.state().object(land).is_none(), "exact word's own target destroyed");
}

#[test]
fn up_to_word_can_claim_zero() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let land = ObjectId(0);
    let card = ObjectId(1);
    let mut state = two_player_state();
    put_on_battlefield(&mut state, card_object(land, land_definition(1, "Forest"), p1, ZoneKind::Battlefield));
    state.objects.insert(card, card_object(card, up_to_then_exact_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    // Only the land submitted - the up_to word legally claims zero.
    cast_spell(&mut engine, p0, card, vec![Target::Object(land)]);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    assert!(engine.state().object(land).is_none(), "exact word's own target still destroyed");
}

#[test]
fn up_to_word_second_still_derives_correctly_regardless_of_position() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let bear = ObjectId(0);
    let wolf = ObjectId(1);
    let land = ObjectId(2);
    let card = ObjectId(3);
    let mut state = two_player_state();
    put_on_battlefield(&mut state, card_object(bear, creature_definition(1, "Bear"), p1, ZoneKind::Battlefield));
    put_on_battlefield(&mut state, card_object(wolf, creature_definition(2, "Wolf"), p1, ZoneKind::Battlefield));
    put_on_battlefield(&mut state, card_object(land, land_definition(3, "Forest"), p1, ZoneKind::Battlefield));
    state.objects.insert(card, card_object(card, exact_then_up_to_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    // Submitted in word order: land first, then both creatures.
    cast_spell(&mut engine, p0, card, vec![Target::Object(land), Target::Object(bear), Target::Object(wolf)]);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    assert!(engine.state().object(land).is_none(), "exact word's own target destroyed");
    assert!(engine.state().object(bear).is_none(), "up_to word's first target destroyed");
    assert!(engine.state().object(wolf).is_none(), "up_to word's second target destroyed");
}

#[test]
fn up_to_word_exceeding_its_own_bound_is_rejected() {
    let p0 = PlayerId(0);
    let bear = ObjectId(0);
    let wolf = ObjectId(1);
    let fox = ObjectId(2);
    let land = ObjectId(3);
    let card = ObjectId(4);
    let mut state = two_player_state();
    for (id, name) in [(bear, "Bear"), (wolf, "Wolf"), (fox, "Fox")] {
        put_on_battlefield(&mut state, card_object(id, creature_definition(id.0, name), PlayerId(1), ZoneKind::Battlefield));
    }
    put_on_battlefield(&mut state, card_object(land, land_definition(4, "Forest"), PlayerId(1), ZoneKind::Battlefield));
    state.objects.insert(card, card_object(card, up_to_then_exact_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    // 3 creatures + 1 land = 4 submitted - after the exact word claims 1,
    // the derived up_to count is 3, exceeding its own max of 2.
    cast_spell(&mut engine, p0, card, vec![Target::Object(bear), Target::Object(wolf), Target::Object(fox), Target::Object(land)]);
    let status = drain(&mut engine);

    assert!(matches!(status, StepStatus::Error(EngineCommandError::InvalidTarget)));
    assert_eq!(engine.state().object(card).unwrap().zone, ZoneKind::Hand, "state untouched");
}

#[test]
fn at_least_word_below_its_own_bound_is_rejected() {
    let p0 = PlayerId(0);
    let land = ObjectId(0);
    let card = ObjectId(1);
    let mut state = two_player_state();
    put_on_battlefield(&mut state, card_object(land, land_definition(1, "Forest"), PlayerId(1), ZoneKind::Battlefield));
    state.objects.insert(card, card_object(card, at_least_then_exact_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    // Only the land submitted - after it claims its exact share, the
    // derived at_least count is 0, below its own minimum of 1.
    cast_spell(&mut engine, p0, card, vec![Target::Object(land)]);
    let status = drain(&mut engine);

    assert!(matches!(status, StepStatus::Error(EngineCommandError::InvalidTarget)));
    assert_eq!(engine.state().object(card).unwrap().zone, ZoneKind::Hand, "state untouched");
}

#[test]
fn at_least_word_meeting_its_own_bound_resolves() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let bear = ObjectId(0);
    let wolf = ObjectId(1);
    let land = ObjectId(2);
    let card = ObjectId(3);
    let mut state = two_player_state();
    put_on_battlefield(&mut state, card_object(bear, creature_definition(1, "Bear"), p1, ZoneKind::Battlefield));
    put_on_battlefield(&mut state, card_object(wolf, creature_definition(2, "Wolf"), p1, ZoneKind::Battlefield));
    put_on_battlefield(&mut state, card_object(land, land_definition(3, "Forest"), p1, ZoneKind::Battlefield));
    state.objects.insert(card, card_object(card, at_least_then_exact_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    // 2 creatures + 1 land - derived at_least count is 2, satisfying "at
    // least one."
    cast_spell(&mut engine, p0, card, vec![Target::Object(bear), Target::Object(wolf), Target::Object(land)]);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    assert!(engine.state().object(bear).is_none());
    assert!(engine.state().object(wolf).is_none());
    assert!(engine.state().object(land).is_none());
}

#[test]
fn two_variable_words_is_still_rejected_as_ambiguous() {
    let p0 = PlayerId(0);
    let bear = ObjectId(0);
    let land = ObjectId(1);
    let card = ObjectId(2);
    let mut state = two_player_state();
    put_on_battlefield(&mut state, card_object(bear, creature_definition(1, "Bear"), PlayerId(1), ZoneKind::Battlefield));
    put_on_battlefield(&mut state, card_object(land, land_definition(2, "Forest"), PlayerId(1), ZoneKind::Battlefield));
    state.objects.insert(card, card_object(card, two_variable_words_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(bear), Target::Object(land)]);
    let status = drain(&mut engine);

    assert!(matches!(status, StepStatus::Error(EngineCommandError::InvalidTarget)));
    assert_eq!(engine.state().object(card).unwrap().zone, ZoneKind::Hand, "state untouched");
}
