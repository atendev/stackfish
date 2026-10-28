//! CR 601.2c, 608.2b: spells that use "target" in several places (Plague Spores). `TargetSpec::Multiple(Vec<TargetRequirement>)` splits the submitted list into one chunk per requirement.
//! This file covers exact counts, and variable counts are in phase5_multi_target_word_variable_count.rs. CR 115.3 and 601.2c dedup applies per word, so one object may be chosen for two words, and the spell fizzles only if every target is illegal (CR 608.2b).

use std::collections::HashMap;

use stackfish_engine::engine::ActionError;
use stackfish_engine::commands::Rejected;
use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, hand_battlefield_graveyard_zones, two_player_state as base_two_player_state};

fn creature_definition(definition_id: u64, name: &str, colors: ColorSet) -> CardDefinition {
    CardDefinition {
        definition_id,
        name: name.to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: Some(colors),
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
        buyback_cost: None,
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
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn nonblack_creature_word() -> TargetRequirement {
    TargetRequirement {
        filter: TargetFilter::Object { mana_value: None,
            card_types: Some(CardTypeSet::CREATURE),
            excluded_card_types: None,
            subtypes: None,
            excluded_subtypes: None,
            supertypes: None,
            excluded_supertypes: None,
            controller: None,
            zone: None,
        },
        count: 1,
        up_to: false,
        at_least: false,
        exclude_source: false,
    }
}

fn land_word() -> TargetRequirement {
    TargetRequirement {
        filter: TargetFilter::Object { mana_value: None,
            card_types: Some(CardTypeSet::LAND),
            excluded_card_types: None,
            subtypes: None,
            excluded_subtypes: None,
            supertypes: None,
            excluded_supertypes: None,
            controller: None,
            zone: None,
        },
        count: 1,
        up_to: false,
        at_least: false,
        exclude_source: false,
    }
}

fn plague_spores_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 20,
        name: "Test Plague Spores".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::SORCERY,
        subtypes: Vec::new(),
        oracle_text: "Destroy target nonblack creature and target land.".to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_plague_spores".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: Some(TargetSpec::Multiple(vec![nonblack_creature_word(), land_word()])),
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

fn put_on_battlefield(state: &mut GameState, object: GameObject) {
    let id = object.object_id;
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(id);
    state.objects.insert(id, object);
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

#[test]
fn both_words_legal_destroys_both_targets() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let creature = ObjectId(0);
    let land = ObjectId(1);
    let card = ObjectId(2);
    let mut state = two_player_state();
    put_on_battlefield(&mut state, card_object(creature, creature_definition(1, "Bear", ColorSet::GREEN), p1, ZoneKind::Battlefield));
    put_on_battlefield(&mut state, card_object(land, land_definition(2, "Forest"), p1, ZoneKind::Battlefield));
    state.objects.insert(card, card_object(card, plague_spores_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(creature), Target::Object(land)]);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    assert!(engine.state().object(creature).is_none(), "the creature was destroyed");
    assert!(engine.state().object(land).is_none(), "the land was destroyed");
    assert_eq!(engine.state().zone(ZoneKind::Graveyard, Some(p1)).unwrap().contents.len(), 2);
}

#[test]
fn the_same_permanent_can_be_chosen_for_two_different_words() {
    // An animated land is a legal choice for both "target nonblack creature" and "target land", since CR 601.2c dedup applies within one word.
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let mut manland = creature_definition(1, "Manland", ColorSet::GREEN);
    manland.card_types |= CardTypeSet::LAND;
    let dual = ObjectId(0);
    let card = ObjectId(2);
    let mut state = two_player_state();
    put_on_battlefield(&mut state, card_object(dual, manland, p1, ZoneKind::Battlefield));
    state.objects.insert(card, card_object(card, plague_spores_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(dual), Target::Object(dual)]);
    let status = drain(&mut engine);

    assert!(!matches!(status, StepStatus::Error(_)), "the same permanent is a legal choice for both words, got {status:?}");
}

#[test]
fn one_word_going_illegal_before_resolution_only_skips_that_word() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let creature = ObjectId(0);
    let land = ObjectId(1);
    let card = ObjectId(2);
    let mut state = two_player_state();
    put_on_battlefield(&mut state, card_object(creature, creature_definition(1, "Bear", ColorSet::GREEN), p1, ZoneKind::Battlefield));
    put_on_battlefield(&mut state, card_object(land, land_definition(2, "Forest"), p1, ZoneKind::Battlefield));
    state.objects.insert(card, card_object(card, plague_spores_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(creature), Target::Object(land)]);
    pay_free_and_drain(&mut engine);
    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(&mut engine);

    // The creature leaves before resolution, so its word is illegal and the land word is not.
    engine.state_mut().zone_mut(ZoneKind::Battlefield, None).unwrap().remove(creature);
    engine.state_mut().objects.remove(&creature);

    engine.submit_action(PlayerAction { player: p1, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(&mut engine);

    assert!(engine.state().object(land).is_none(), "CR 608.2b: the still-legal word's target is still affected");
}

#[test]
fn both_words_illegal_fizzles_the_whole_spell() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let creature = ObjectId(0);
    let land = ObjectId(1);
    let card = ObjectId(2);
    let mut state = two_player_state();
    put_on_battlefield(&mut state, card_object(creature, creature_definition(1, "Bear", ColorSet::GREEN), p1, ZoneKind::Battlefield));
    put_on_battlefield(&mut state, card_object(land, land_definition(2, "Forest"), p1, ZoneKind::Battlefield));
    state.objects.insert(card, card_object(card, plague_spores_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(creature), Target::Object(land)]);
    pay_free_and_drain(&mut engine);
    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(&mut engine);

    // Both targets leave the battlefield before resolution.
    for id in [creature, land] {
        engine.state_mut().zone_mut(ZoneKind::Battlefield, None).unwrap().remove(id);
        engine.state_mut().objects.remove(&id);
    }

    engine.submit_action(PlayerAction { player: p1, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(&mut engine);

    assert!(
        engine.state().ledger.iter().any(|e| matches!(e.kind, GameEventKind::Fizzled { .. })),
        "CR 608.2b: every target across every word is illegal - the whole spell fizzles"
    );
}

#[test]
fn rejects_a_wrong_count_submission() {
    let p0 = PlayerId(0);
    let creature = ObjectId(0);
    let land = ObjectId(1);
    let card = ObjectId(2);
    let mut state = two_player_state();
    put_on_battlefield(&mut state, card_object(creature, creature_definition(1, "Bear", ColorSet::GREEN), PlayerId(1), ZoneKind::Battlefield));
    put_on_battlefield(&mut state, card_object(land, land_definition(2, "Forest"), PlayerId(1), ZoneKind::Battlefield));
    state.objects.insert(card, card_object(card, plague_spores_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    // Only one target submitted for a two-word spell.
    let err = try_cast_spell(&mut engine, p0, card, vec![Target::Object(creature)]).unwrap_err();

    assert!(matches!(err, ActionError::Rejected(Rejected::InvalidTarget)));
    assert_eq!(engine.state().object(card).unwrap().zone, ZoneKind::Hand, "state untouched");
}

#[test]
fn rejects_targets_that_dont_match_their_own_word() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let creature = ObjectId(0);
    let land = ObjectId(1);
    let card = ObjectId(2);
    let mut state = two_player_state();
    put_on_battlefield(&mut state, card_object(creature, creature_definition(1, "Bear", ColorSet::GREEN), p1, ZoneKind::Battlefield));
    put_on_battlefield(&mut state, card_object(land, land_definition(2, "Forest"), p1, ZoneKind::Battlefield));
    state.objects.insert(card, card_object(card, plague_spores_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    // Swapped - the land submitted for the creature word, and vice versa.
    let err = try_cast_spell(&mut engine, p0, card, vec![Target::Object(land), Target::Object(creature)]).unwrap_err();

    assert!(matches!(err, ActionError::Rejected(Rejected::InvalidTarget)));
    assert_eq!(engine.state().object(card).unwrap().zone, ZoneKind::Hand, "state untouched");
}
