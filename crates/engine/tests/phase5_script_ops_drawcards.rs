//! Phase 5 vertical slice: `AbilityEffect::DrawCards` (backlog item 8,
//! fourth Script API primitive) — `Game.Ops.DrawCards(n)`, e.g. "Draw two
//! cards." No target - always the resolving `StackObject`'s controller,
//! same "controller only" shape `GainLife` already uses (real card text is
//! overwhelmingly "you draw" rather than "target player draws").
//!
//! Cheapest of the four `Game.Ops` primitives so far: a `ZoneChange` always
//! describes a move that already happened (see `Destroy`'s own doc
//! comment), so `ResolveStackTop` mutates state immediately via the shared
//! `Engine::draw_one_card` helper (extracted from `draw_step`'s own body)
//! and just logs the resulting real event(s) - no `CommitEvents`
//! special-case needed, same convention `draw_step` already established.
//! This also makes it the first non-turn-structure way to draw, so it
//! exercises the already-existing `DrawsCard`/`EachDrawsCard` trigger
//! conditions from a new source for the first time.
//!
//! Drawing from an empty library silently stops rather than looping past it
//! - CR 704.5b's empty-library-loss SBA isn't modeled (same TODO
//! `draw_step` itself already carries).

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

/// "Test Draw: Draw N cards."
fn drawcards_spell_definition(amount: u32) -> CardDefinition {
    CardDefinition {
        definition_id: 20,
        name: "Test Draw".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::INSTANT,
        subtypes: Vec::new(),
        oracle_text: "Draw cards.".to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_draw".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: Some(AbilityEffect::DrawCards(amount)),
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn draws_card_trigger_ability(source: ObjectId) -> RegisteredAbility {
    RegisteredAbility {
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
        id: AbilityId(0),
        source,
        kind: AbilityKind::Triggered,
        condition: Some(ConditionSpec::Primitive(ConditionPrimitive::DrawsCard)),
        cost: None,
        target: None,
        operation: ScriptHandle(0),
        effect: Some(AbilityEffect::GainLife(1)),
        intervening_if: None,
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

#[test]
fn drawcards_spell_puts_two_cards_in_casters_hand() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let lib_a = ObjectId(0);
    let lib_b = ObjectId(1);
    state.objects.insert(lib_a, card_object(lib_a, land_definition(1, "Forest"), p0, ZoneKind::Library));
    state.zone_mut(ZoneKind::Library, Some(p0)).unwrap().push_bottom(lib_a);
    state.objects.insert(lib_b, card_object(lib_b, land_definition(2, "Island"), p0, ZoneKind::Library));
    state.zone_mut(ZoneKind::Library, Some(p0)).unwrap().push_bottom(lib_b);
    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, drawcards_spell_definition(2), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    let status = pass_both_and_drain(&mut engine, p0, p1);

    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert_eq!(engine.state().zone(ZoneKind::Hand, Some(p0)).unwrap().contents.len(), 2, "the spell left, two new cards arrived");
    assert!(engine.state().zone(ZoneKind::Library, Some(p0)).unwrap().contents.is_empty());
}

#[test]
fn scripted_draw_fires_an_already_existing_draws_card_trigger() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let permanent = ObjectId(0);
    state.objects.insert(permanent, card_object(permanent, creature_definition(1, "Bear"), p0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(permanent);
    state.abilities.push(draws_card_trigger_ability(permanent));
    let lib_a = ObjectId(3);
    state.objects.insert(lib_a, card_object(lib_a, land_definition(2, "Forest"), p0, ZoneKind::Library));
    state.zone_mut(ZoneKind::Library, Some(p0)).unwrap().push_bottom(lib_a);
    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, drawcards_spell_definition(1), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    let status = pass_both_and_drain(&mut engine, p0, p1);

    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert_eq!(engine.state().stack.len(), 1, "the DrawsCard trigger was collected off the scripted draw's own ZoneChange");

    let status = pass_both_and_drain(&mut engine, p0, p1);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert!(engine.state().stack.is_empty());
    assert_eq!(engine.state().player(p0).unwrap().life, 21, "GainLife(1) applied from the DrawsCard trigger");
}

#[test]
fn drawing_more_than_the_library_holds_stops_without_crashing() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let lib_a = ObjectId(0);
    state.objects.insert(lib_a, card_object(lib_a, land_definition(1, "Forest"), p0, ZoneKind::Library));
    state.zone_mut(ZoneKind::Library, Some(p0)).unwrap().push_bottom(lib_a);
    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, drawcards_spell_definition(2), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    let status = pass_both_and_drain(&mut engine, p0, p1);

    // CR 120.3/104.3c: the one available card is drawn, then the second
    // attempt (from an empty library) marks p0 for the loss SBA - in this
    // two-player game that immediately ends it (CR 104.2a), backlog item
    // 11b wiring GameOver up to actually fire once elimination is real.
    assert!(matches!(status, StepStatus::GameOver { winner: Some(w) } if w == p1));
    assert!(engine.state().player(p0).unwrap().eliminated);
}

#[test]
fn drawcards_ability_works_the_same_as_a_drawcards_spell() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let source = ObjectId(0);
    let lib_a = ObjectId(1);
    state.objects.insert(source, card_object(source, creature_definition(1, "Scribe"), p0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(source);
    state.objects.insert(lib_a, card_object(lib_a, land_definition(2, "Forest"), p0, ZoneKind::Library));
    state.zone_mut(ZoneKind::Library, Some(p0)).unwrap().push_bottom(lib_a);
    state.abilities.push(RegisteredAbility {
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
        id: AbilityId(1),
        source,
        kind: AbilityKind::Activated,
        condition: None,
        cost: Some(CostSpec { mana: None, additional: vec![AdditionalCost::TapSelf], alternative: Vec::new() }),
        target: None,
        operation: ScriptHandle(0),
        effect: Some(AbilityEffect::DrawCards(1)),
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
            },
        })
        .expect("player holds priority");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }));
    engine.submit_payment(PaymentPlan { payments: vec![CostPayment::Tap(source)] }).expect("Tap pays TapSelf");
    drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    assert_eq!(engine.state().zone(ZoneKind::Hand, Some(p0)).unwrap().contents.len(), 1);
    assert!(engine.state().zone(ZoneKind::Library, Some(p0)).unwrap().contents.is_empty());
}

#[test]
fn drawing_two_preserves_library_order() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let first = ObjectId(0);
    let second = ObjectId(1);
    let third = ObjectId(2);
    state.objects.insert(first, card_object(first, land_definition(1, "First"), p0, ZoneKind::Library));
    state.zone_mut(ZoneKind::Library, Some(p0)).unwrap().push_bottom(first);
    state.objects.insert(second, card_object(second, land_definition(2, "Second"), p0, ZoneKind::Library));
    state.zone_mut(ZoneKind::Library, Some(p0)).unwrap().push_bottom(second);
    state.objects.insert(third, card_object(third, land_definition(3, "Third"), p0, ZoneKind::Library));
    state.zone_mut(ZoneKind::Library, Some(p0)).unwrap().push_bottom(third);
    let card = ObjectId(3);
    state.objects.insert(card, card_object(card, drawcards_spell_definition(2), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    let hand = &engine.state().zone(ZoneKind::Hand, Some(p0)).unwrap().contents;
    let hand_names: Vec<&str> =
        hand.iter().map(|id| engine.state().object(*id).unwrap().definition.name.as_str()).collect();
    assert_eq!(hand_names, vec!["First", "Second"], "top two library cards drawn in order");

    let library = &engine.state().zone(ZoneKind::Library, Some(p0)).unwrap().contents;
    let library_names: Vec<&str> =
        library.iter().map(|id| engine.state().object(*id).unwrap().definition.name.as_str()).collect();
    assert_eq!(library_names, vec!["Third"], "third card remains on top of the library");
}
