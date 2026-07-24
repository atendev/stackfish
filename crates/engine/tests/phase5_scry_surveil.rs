//! Phase 5 vertical slice: `AbilityEffect::Scry`/`Surveil` (backlog item
//! 14a) — `Game.Ops.Scry(n)`/`Game.Ops.Surveil(n)` (CR 701.22a/701.25a).
//!
//! Unlike every prior `Game.Ops.*` primitive, these can't resolve in one
//! step: `ResolveStackTop` peeks the controller's top `n` library ids
//! (read-only) and pauses on a new `ProcessState::AwaitingScryChoice`/
//! `AwaitingSurveilChoice` rather than mutating anything itself -
//! `Engine::submit_scry_choice`/`submit_surveil_choice` carry out the actual
//! reorder (Scry) or library -> graveyard move (Surveil, CR 400.7 new
//! `ObjectId` per card) once the controller answers. CR 701.22b/701.25c:
//! `n == 0` (or an empty library) is a no-op - no pause, no event.
//!
//! CR 701.22d/701.25d ("whenever a player scries/surveils" trigger) is out
//! of scope this slice - the resulting `CardsScried`/`CardsSurveiled` event
//! already reaches `CollectTriggers`, but no `ConditionPrimitive` exists yet
//! to match it (see `AbilityEffect::Scry`'s own doc comment).

use std::collections::{HashMap, VecDeque};

use stackfish_engine::engine::{
    Engine, ScryChoiceError, StepStatus, SurveilChoiceError,
};
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, hand_battlefield_graveyard_zones, two_player_state as base_two_player_state};

fn library_card_definition(definition_id: u64, name: &str) -> CardDefinition {
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
        script_ref: ScriptReference::Inline(String::new()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn scry_spell_definition(n: u32) -> CardDefinition {
    CardDefinition {
        definition_id: 30,
        name: "Test Scry Spell".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::INSTANT,
        subtypes: Vec::new(),
        oracle_text: format!("Scry {n}."),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_scry".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        effect: Some(AbilityEffect::Scry(n)),
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn surveil_spell_definition(n: u32) -> CardDefinition {
    CardDefinition {
        definition_id: 31,
        name: "Test Surveil Spell".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::INSTANT,
        subtypes: Vec::new(),
        oracle_text: format!("Surveil {n}."),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_surveil".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        effect: Some(AbilityEffect::Surveil(n)),
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

/// Same shape as every other Phase 5 test's `two_player_state`, plus a
/// Library zone for `p0` (absent from the shared precedent since most
/// slices never touch it) seeded with `library` in top-to-bottom order
/// (`contents`'s front is the top - `Zone::top`/`push_top` already establish
/// this). Each id in `library` must also be inserted into `objects`
/// separately by the caller, same as every other zone here.
fn two_player_state(library: Vec<ObjectId>) -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let mut zones = hand_battlefield_graveyard_zones(p0, p1);
    zones.push(Zone { kind: ZoneKind::Library, owner: Some(p0), contents: VecDeque::from(library) });
    zones.push(Zone::new(ZoneKind::Library, Some(p1)));
    GameState {
        step: Step::PreCombatMain,
        last_active_turn_number: HashMap::from([(p0, 1), (p1, 1)]),
        ..base_two_player_state(zones)
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
                alternative_cost: None,
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

/// Both players pass priority (CR 117.4) so the cast spell actually resolves.
fn pass_both_and_drain(engine: &mut Engine, p0: PlayerId, p1: PlayerId) -> StepStatus {
    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine);
    engine.submit_action(PlayerAction { player: p1, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine)
}

fn library_order(engine: &Engine, player: PlayerId) -> Vec<ObjectId> {
    engine.state().zone(ZoneKind::Library, Some(player)).unwrap().contents.iter().copied().collect()
}

fn scried_events(engine: &Engine) -> Vec<(PlayerId, Vec<ObjectId>, Vec<ObjectId>)> {
    engine
        .state()
        .ledger
        .iter()
        .filter_map(|e| match &e.kind {
            GameEventKind::CardsScried { player, kept_on_top, put_bottom } => {
                Some((*player, kept_on_top.clone(), put_bottom.clone()))
            }
            _ => None,
        })
        .collect()
}

fn surveiled_events(engine: &Engine) -> Vec<(PlayerId, Vec<ObjectId>, Vec<ObjectId>)> {
    engine
        .state()
        .ledger
        .iter()
        .filter_map(|e| match &e.kind {
            GameEventKind::CardsSurveiled { player, kept_on_top, put_graveyard } => {
                Some((*player, kept_on_top.clone(), put_graveyard.clone()))
            }
            _ => None,
        })
        .collect()
}

// ---------------------------------------------------------------------
// Scry
// ---------------------------------------------------------------------

#[test]
fn scry_two_keep_both_on_top_reorders_in_chosen_order() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let lib0 = ObjectId(10);
    let lib1 = ObjectId(11);
    let lib2 = ObjectId(12);
    let card = ObjectId(2);
    let mut state = two_player_state(vec![lib0, lib1, lib2]);
    for (id, name) in [(lib0, "Lib0"), (lib1, "Lib1"), (lib2, "Lib2")] {
        state.objects.insert(id, card_object(id, library_card_definition(id.0, name), p0, ZoneKind::Library));
    }
    state.objects.insert(card, card_object(card, scry_spell_definition(2), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    let status = pass_both_and_drain(&mut engine, p0, p1);
    let StepStatus::AwaitingScryChoice { player, cards } = status else {
        panic!("expected a pause for Scry(2), got {status:?}")
    };
    assert_eq!(player, p0);
    assert_eq!(cards, vec![lib0, lib1], "top 2 cards, front = top");

    // Reverse the offered order to prove the chosen order is respected, not
    // just the offered one.
    engine.submit_scry_choice(vec![lib1, lib0], Vec::new()).expect("kept_on_top+put_bottom together are exactly cards");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));

    assert_eq!(library_order(&engine, p0), vec![lib1, lib0, lib2], "reordered top 2, untouched rest below");
    assert_eq!(scried_events(&engine), vec![(p0, vec![lib1, lib0], Vec::new())]);
}

#[test]
fn scry_two_put_both_to_bottom_in_chosen_order() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let lib0 = ObjectId(10);
    let lib1 = ObjectId(11);
    let lib2 = ObjectId(12);
    let card = ObjectId(2);
    let mut state = two_player_state(vec![lib0, lib1, lib2]);
    for (id, name) in [(lib0, "Lib0"), (lib1, "Lib1"), (lib2, "Lib2")] {
        state.objects.insert(id, card_object(id, library_card_definition(id.0, name), p0, ZoneKind::Library));
    }
    state.objects.insert(card, card_object(card, scry_spell_definition(2), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    engine.submit_scry_choice(Vec::new(), vec![lib1, lib0]).expect("both to bottom");
    drain(&mut engine);

    assert_eq!(library_order(&engine, p0), vec![lib2, lib1, lib0], "lib2 was never touched, stays on top; bottom pile in chosen order");
    assert_eq!(scried_events(&engine), vec![(p0, Vec::new(), vec![lib1, lib0])]);
}

#[test]
fn scry_three_splits_between_top_and_bottom() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let lib0 = ObjectId(10);
    let lib1 = ObjectId(11);
    let lib2 = ObjectId(12);
    let card = ObjectId(2);
    let mut state = two_player_state(vec![lib0, lib1, lib2]);
    for (id, name) in [(lib0, "Lib0"), (lib1, "Lib1"), (lib2, "Lib2")] {
        state.objects.insert(id, card_object(id, library_card_definition(id.0, name), p0, ZoneKind::Library));
    }
    state.objects.insert(card, card_object(card, scry_spell_definition(3), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    let status = pass_both_and_drain(&mut engine, p0, p1);
    let StepStatus::AwaitingScryChoice { cards, .. } = status else {
        panic!("expected a pause for Scry(3), got {status:?}")
    };
    assert_eq!(cards, vec![lib0, lib1, lib2]);

    engine.submit_scry_choice(vec![lib0], vec![lib1, lib2]).expect("1 top / 2 bottom, exactly partitions cards");
    drain(&mut engine);

    assert_eq!(library_order(&engine, p0), vec![lib0, lib1, lib2]);
}

#[test]
fn scry_zero_resolves_synchronously_with_no_pause_or_event() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let lib0 = ObjectId(10);
    let card = ObjectId(2);
    let mut state = two_player_state(vec![lib0]);
    state.objects.insert(lib0, card_object(lib0, library_card_definition(lib0.0, "Lib0"), p0, ZoneKind::Library));
    state.objects.insert(card, card_object(card, scry_spell_definition(0), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    let status = pass_both_and_drain(&mut engine, p0, p1);

    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0), "CR 701.22b: Scry 0 is a no-op, no pause");
    assert_eq!(library_order(&engine, p0), vec![lib0], "untouched");
    assert!(scried_events(&engine).is_empty(), "CR 701.22b: no scry event occurs");
}

#[test]
fn scry_n_greater_than_library_size_takes_however_many_exist() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let lib0 = ObjectId(10);
    let lib1 = ObjectId(11);
    let card = ObjectId(2);
    let mut state = two_player_state(vec![lib0, lib1]);
    for (id, name) in [(lib0, "Lib0"), (lib1, "Lib1")] {
        state.objects.insert(id, card_object(id, library_card_definition(id.0, name), p0, ZoneKind::Library));
    }
    state.objects.insert(card, card_object(card, scry_spell_definition(5), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    let status = pass_both_and_drain(&mut engine, p0, p1);
    let StepStatus::AwaitingScryChoice { cards, .. } = status else {
        panic!("expected a pause for Scry(5) with only 2 library cards, got {status:?}")
    };
    assert_eq!(cards, vec![lib0, lib1], "only as many as exist, no panic");

    engine.submit_scry_choice(cards, Vec::new()).expect("offering back the exact offered cards");
    drain(&mut engine);
    assert_eq!(library_order(&engine, p0), vec![lib0, lib1]);
}

#[test]
fn submit_scry_choice_rejects_wrong_cards_and_leaves_the_pause_open() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let lib0 = ObjectId(10);
    let lib1 = ObjectId(11);
    let card = ObjectId(2);
    let mut state = two_player_state(vec![lib0, lib1]);
    for (id, name) in [(lib0, "Lib0"), (lib1, "Lib1")] {
        state.objects.insert(id, card_object(id, library_card_definition(id.0, name), p0, ZoneKind::Library));
    }
    state.objects.insert(card, card_object(card, scry_spell_definition(2), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    // Missing lib1 entirely - not a valid partition of the offered cards.
    let err = engine.submit_scry_choice(vec![lib0], Vec::new()).unwrap_err();
    assert!(matches!(err, ScryChoiceError::WrongCards));

    // The pause is still open - a subsequent correct answer still works.
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingScryChoice { .. }), "a rejected answer must leave the pause untouched");
    engine.submit_scry_choice(vec![lib0, lib1], Vec::new()).expect("a correct answer still resumes normally");
    drain(&mut engine);
    assert_eq!(library_order(&engine, p0), vec![lib0, lib1]);
}

#[test]
fn submit_surveil_choice_rejects_a_pending_scry_pause() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let lib0 = ObjectId(10);
    let card = ObjectId(2);
    let mut state = two_player_state(vec![lib0]);
    state.objects.insert(lib0, card_object(lib0, library_card_definition(lib0.0, "Lib0"), p0, ZoneKind::Library));
    state.objects.insert(card, card_object(card, scry_spell_definition(1), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    let status = pass_both_and_drain(&mut engine, p0, p1);
    assert!(matches!(status, StepStatus::AwaitingScryChoice { .. }));

    let err = engine.submit_surveil_choice(vec![lib0], Vec::new()).unwrap_err();
    assert!(matches!(err, SurveilChoiceError::NotAwaitingChoice), "a Scry pause isn't a Surveil pause");
}

// ---------------------------------------------------------------------
// Surveil
// ---------------------------------------------------------------------

#[test]
fn surveil_two_keep_both_on_top_is_a_pure_reorder_no_graveyard_move() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let lib0 = ObjectId(10);
    let lib1 = ObjectId(11);
    let card = ObjectId(2);
    let mut state = two_player_state(vec![lib0, lib1]);
    for (id, name) in [(lib0, "Lib0"), (lib1, "Lib1")] {
        state.objects.insert(id, card_object(id, library_card_definition(id.0, name), p0, ZoneKind::Library));
    }
    state.objects.insert(card, card_object(card, surveil_spell_definition(2), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    let status = pass_both_and_drain(&mut engine, p0, p1);
    let StepStatus::AwaitingSurveilChoice { player, cards } = status else {
        panic!("expected a pause for Surveil(2), got {status:?}")
    };
    assert_eq!(player, p0);
    assert_eq!(cards, vec![lib0, lib1]);

    engine.submit_surveil_choice(vec![lib1, lib0], Vec::new()).expect("both kept on top, reversed order");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));

    assert_eq!(library_order(&engine, p0), vec![lib1, lib0], "reordered, still in the library");
    // The graveyard isn't necessarily empty - the resolved instant itself
    // (CR 608.2m) lands there too - but neither surveiled library card does.
    let graveyard = engine.state().zone(ZoneKind::Graveyard, Some(p0)).unwrap();
    assert!(!graveyard.contents.contains(&lib0) && !graveyard.contents.contains(&lib1), "neither kept-on-top card left the library");
    assert_eq!(surveiled_events(&engine), vec![(p0, vec![lib1, lib0], Vec::new())]);
}

#[test]
fn surveil_two_to_graveyard_is_a_real_zone_change_with_new_ids() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let lib0 = ObjectId(10);
    let lib1 = ObjectId(11);
    let card = ObjectId(2);
    let mut state = two_player_state(vec![lib0, lib1]);
    for (id, name) in [(lib0, "Lib0"), (lib1, "Lib1")] {
        state.objects.insert(id, card_object(id, library_card_definition(id.0, name), p0, ZoneKind::Library));
    }
    state.objects.insert(card, card_object(card, surveil_spell_definition(2), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    engine.submit_surveil_choice(Vec::new(), vec![lib0, lib1]).expect("both to graveyard");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));

    assert!(library_order(&engine, p0).is_empty(), "both cards left the library");
    assert!(engine.state().object(lib0).is_none(), "old id retired (CR 400.7)");
    assert!(engine.state().object(lib1).is_none(), "old id retired (CR 400.7)");

    // 3, not 2: the resolved instant itself (CR 608.2m) also lands in the
    // owner's graveyard, alongside the two surveiled library cards.
    let graveyard = engine.state().zone(ZoneKind::Graveyard, Some(p0)).unwrap();
    assert_eq!(graveyard.contents.len(), 3, "both surveiled cards, plus the resolved instant itself");

    let zone_changes: Vec<_> = engine
        .state()
        .ledger
        .iter()
        .filter_map(|e| match e.kind {
            GameEventKind::ZoneChange { object, from: ZoneKind::Library, to: ZoneKind::Graveyard, .. } => Some(object),
            _ => None,
        })
        .collect();
    assert_eq!(zone_changes.len(), 2, "one real ZoneChange per surveiled-to-graveyard card");

    let surveiled = surveiled_events(&engine);
    assert_eq!(surveiled.len(), 1);
    let (player, kept_on_top, put_graveyard) = &surveiled[0];
    assert_eq!(*player, p0);
    assert!(kept_on_top.is_empty());
    assert_eq!(put_graveyard, &zone_changes, "the summary event's put_graveyard carries the new ids, not the old ones");
}

// ---------------------------------------------------------------------
// Game.Ops.Scry(n)/Game.Ops.Surveil(n) Lua bindings decode to the right
// AbilityEffect primitive.
// ---------------------------------------------------------------------

#[test]
fn game_ops_scry_decodes_to_the_ability_effect_primitive() {
    let mut runtime = ScriptRuntime::new();
    let (effect, handle) = runtime.load_operation("return Game.Ops.Scry(3)").expect("primitive loads");
    assert!(matches!(effect, Some(AbilityEffect::Scry(3))));
    assert_eq!(handle, ScriptHandle(0), "the operation handle is an unused placeholder when effect is Some");
}

#[test]
fn game_ops_surveil_decodes_to_the_ability_effect_primitive() {
    let mut runtime = ScriptRuntime::new();
    let (effect, handle) = runtime.load_operation("return Game.Ops.Surveil(2)").expect("primitive loads");
    assert!(matches!(effect, Some(AbilityEffect::Surveil(2))));
    assert_eq!(handle, ScriptHandle(0));
}
