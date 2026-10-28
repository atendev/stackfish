//! CR 701.19b: `AbilityEffect::Shuffle` (`Game.Ops.Shuffle()`) shuffles the resolving controller's own library, through `Engine::shuffle_library` (Fisher-Yates over `state.rng`, logging `LibraryShuffled` and `RngDrawn`).
// TODO: `Target::Player` ("target player shuffles their library")

use std::collections::HashMap;

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, two_player_state as base_two_player_state, zones};

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
        toughness: Some(PTValue::Fixed(2)),
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

fn land_definition(definition_id: u64, name: &str) -> CardDefinition {
    CardDefinition {
        definition_id,
        name: name.to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::BASIC,
        card_types: CardTypeSet::LAND,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("forest".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn shuffle_spell_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 20,
        name: "Test Shuffle".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::INSTANT,
        subtypes: Vec::new(),
        oracle_text: "Shuffle your library.".to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_shuffle".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: Some(AbilityEffect::Shuffle),
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn draw_spell_definition(definition_id: u64) -> CardDefinition {
    CardDefinition {
        definition_id,
        name: "Test Draw".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::INSTANT,
        subtypes: Vec::new(),
        oracle_text: "Draw a card.".to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_draw".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: Some(AbilityEffect::DrawCards(1)),
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
        ..base_two_player_state(zones(p0, p1, &[ZoneKind::Hand, ZoneKind::Library, ZoneKind::Battlefield, ZoneKind::Graveyard]))
    }
}

fn stock_library(state: &mut GameState, p0: PlayerId, names: &[&str]) -> Vec<ObjectId> {
    let mut ids = Vec::new();
    for (i, name) in names.iter().enumerate() {
        let id = ObjectId(100 + i as u64);
        state.objects.insert(id, card_object(id, land_definition(100 + i as u64, name), p0, ZoneKind::Library));
        state.zone_mut(ZoneKind::Library, Some(p0)).unwrap().push_bottom(id);
        ids.push(id);
    }
    ids
}

fn library_names(engine: &Engine, p0: PlayerId) -> Vec<String> {
    engine
        .state()
        .zone(ZoneKind::Library, Some(p0))
        .unwrap()
        .contents
        .iter()
        .map(|id| engine.state().object(*id).unwrap().definition.name.clone())
        .collect()
}

fn cast_spell(engine: &mut Engine, player: PlayerId, card: ObjectId) {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::CastSpell {
                card,
                modes: Vec::new(),
                targets: Vec::new(),
                x_value: None,
                alternative_cost: None, pay_buyback: false,
            },
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

// Builds a fresh state so the determinism test can compare two independent GameStates from one fixed seed, since GameState has no Clone.
fn shuffle_test_state() -> GameState {
    let p0 = PlayerId(0);
    let mut state = two_player_state();
    stock_library(&mut state, p0, &["First", "Second", "Third", "Fourth", "Fifth"]);
    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, shuffle_spell_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);
    state
}

fn run_shuffle_spell(state: GameState, card: ObjectId, p0: PlayerId, p1: PlayerId) -> Engine {
    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);
    engine
}

#[test]
fn shuffle_spell_reorders_the_library_deterministically_and_logs_the_event() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(2);

    // The same fixed seed run twice asserts determinism.
    let first_run = run_shuffle_spell(shuffle_test_state(), card, p0, p1);
    let second_run = run_shuffle_spell(shuffle_test_state(), card, p0, p1);
    let first_names = library_names(&first_run, p0);
    let second_names = library_names(&second_run, p0);

    assert_eq!(first_names, second_names, "same seed, same shuffle result");
    assert_ne!(first_names, vec!["First", "Second", "Third", "Fourth", "Fifth"], "library order actually changed");
    let mut sorted = first_names.clone();
    sorted.sort();
    assert_eq!(sorted, vec!["Fifth", "First", "Fourth", "Second", "Third"], "shuffle is a permutation, no cards lost or duplicated");
    assert!(
        first_run.state().ledger.iter().any(|e| matches!(e.kind, GameEventKind::LibraryShuffled { player } if player == p0)),
        "LibraryShuffled logged for p0"
    );
}

#[test]
fn shuffle_ability_works_the_same_as_a_shuffle_spell() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let source = ObjectId(0);
    stock_library(&mut state, p0, &["First", "Second", "Third", "Fourth", "Fifth"]);
    state.objects.insert(source, card_object(source, creature_definition(1, "Scribe"), p0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(source);
    state.abilities.push(RegisteredAbility {
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
        id: AbilityId(0),
        source,
        kind: AbilityKind::Activated,
        condition: None,
        cost: Some(CostSpec { mana: None, additional: vec![AdditionalCost::TapSelf], alternative: Vec::new() }),
        target: None,
        operation: ScriptHandle(0),
        effect: Some(AbilityEffect::Shuffle),
        intervening_if: None,
    });

    let mut engine = Engine::new(state);
    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::ActivateAbility {
                source,
                ability_index: 0,
                targets: Vec::new(),
                chosen_colors: Vec::new(),
                x_value: None,
            },
        })
        .expect("player holds priority");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }));
    engine.submit_payment(PaymentPlan { payments: vec![CostPayment::Tap(source)] }).expect("Tap pays TapSelf");
    drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    assert_ne!(
        library_names(&engine, p0),
        vec!["First", "Second", "Third", "Fourth", "Fifth"],
        "activated Shuffle reordered the library same as a Shuffle spell would"
    );
    assert!(
        engine.state().ledger.iter().any(|e| matches!(e.kind, GameEventKind::LibraryShuffled { player } if player == p0)),
        "LibraryShuffled logged for p0"
    );
}

#[test]
fn shuffling_an_empty_or_single_card_library_is_a_harmless_no_op() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);

    // Empty library: no crash, no swaps, still logs LibraryShuffled.
    let mut empty_state = two_player_state();
    let empty_card = ObjectId(2);
    empty_state.objects.insert(empty_card, card_object(empty_card, shuffle_spell_definition(), p0, ZoneKind::Hand));
    empty_state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(empty_card);
    let mut engine = Engine::new(empty_state);
    cast_spell(&mut engine, p0, empty_card);
    pay_free_and_drain(&mut engine);
    let status = pass_both_and_drain(&mut engine, p0, p1);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert!(engine.state().zone(ZoneKind::Library, Some(p0)).unwrap().contents.is_empty());
    assert!(
        !engine.state().ledger.iter().any(|e| matches!(e.kind, GameEventKind::RngDrawn { .. })),
        "an empty library needs zero swaps, so no RngDrawn draws happen"
    );

    // Single-card library: no swaps possible, order trivially preserved.
    let mut single_state = two_player_state();
    stock_library(&mut single_state, p0, &["Only"]);
    let single_card = ObjectId(2);
    single_state.objects.insert(single_card, card_object(single_card, shuffle_spell_definition(), p0, ZoneKind::Hand));
    single_state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(single_card);
    let mut engine = Engine::new(single_state);
    cast_spell(&mut engine, p0, single_card);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);
    assert_eq!(library_names(&engine, p0), vec!["Only"]);
    assert!(
        !engine.state().ledger.iter().any(|e| matches!(e.kind, GameEventKind::RngDrawn { .. })),
        "a single-card library needs zero swaps, so no RngDrawn draws happen"
    );
}

#[test]
fn a_subsequent_draw_actually_consumes_the_shuffled_order() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    stock_library(&mut state, p0, &["First", "Second", "Third", "Fourth", "Fifth"]);
    let shuffle_card = ObjectId(2);
    state.objects.insert(shuffle_card, card_object(shuffle_card, shuffle_spell_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(shuffle_card);
    let draw_card = ObjectId(3);
    state.objects.insert(draw_card, card_object(draw_card, draw_spell_definition(21), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(draw_card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, shuffle_card);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);
    let expected_top = library_names(&engine, p0)[0].clone();

    cast_spell(&mut engine, p0, draw_card);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    let hand_names: Vec<String> = engine
        .state()
        .zone(ZoneKind::Hand, Some(p0))
        .unwrap()
        .contents
        .iter()
        .map(|id| engine.state().object(*id).unwrap().definition.name.clone())
        .collect();
    assert_eq!(hand_names, vec![expected_top], "the card drawn is exactly the one the shuffle put on top");
}
