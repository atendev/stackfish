//! Phase 5 vertical slice: CR 700.2a modal spell mode-selection (backlog
//! item 9a-i) — the controller chooses exactly one of a spell's modes as
//! part of casting, before targets are chosen (601.2b precedes 601.2c).
//! This file covers only the "choose exactly one" (`ModeChoice::Exactly(1)`)
//! case; choosing two or more modes (CR 700.2d) is real now too — see
//! `phase5_modal_multi_mode.rs`. Another player choosing (700.2e) and a mode
//! carrying its own additional cost (700.2h) are still deferred — see
//! `CardDefinition::modes`'s own doc comment.
//!
//! Unlike `Game.SelectTargets`/`Game.MayChoose` (backlog items 9/9a, mid-
//! resolution Lua coroutine yields), mode-choice is a cast-time decision:
//! `PlayerActionKind::CastSpell::modes` already flowed end-to-end before
//! this slice (all the way onto `StackObject.chosen_modes`) without ever
//! being read - this slice is what actually consults it, choosing which of
//! `CardDefinition.modes`'s `SpellMode`s supplies the target requirement
//! and effect for the rest of casting/resolution.

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
        mode_choice: ModeChoice::Exactly(1),
    }
}

/// "Choose one — Mode 0: You gain 3 life. Mode 1: Test Modal Spell deals 2
/// damage to any target." Mode 0 has no target requirement at all (CR
/// 601.2c: a spell is cast as though it didn't require targets its chosen
/// mode doesn't ask for); mode 1 requires exactly one `AnyTarget`.
fn modal_spell_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 30,
        name: "Test Modal Spell".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::INSTANT,
        subtypes: Vec::new(),
        oracle_text: "Choose one - \
            * You gain 3 life. \
            * Test Modal Spell deals 2 damage to any target."
            .to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_modal_spell".to_string()),
        keywords: KeywordSet::empty(),
        // Ignored - a modal card's per-mode fields (below) are used instead.
        target_requirement: None,
        alternative_costs: Vec::new(),
        effect: None,
        modes: vec![
            SpellMode { target_requirement: None, effect: Some(AbilityEffect::GainLife(3)), additional_cost: None },
            SpellMode {
                target_requirement: Some(TargetSpec::Primitive(TargetRequirement {
                    filter: TargetFilter::AnyTarget,
                    count: 1,
                    up_to: false, at_least: false,
                    exclude_source: false,
                })),
                effect: Some(AbilityEffect::Damage(2)),
                additional_cost: None,
            },
        ],
        mode_choice: ModeChoice::Exactly(1),
    }
}

/// A plain, non-modal instant - regression guard that `modes: Vec::new()`
/// on `CardDefinition` still behaves exactly as before this slice.
fn shock_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 20,
        name: "Test Shock".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::INSTANT,
        subtypes: Vec::new(),
        oracle_text: "Test Shock deals 2 damage to any target.".to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_shock".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: Some(TargetSpec::Primitive(TargetRequirement {
            filter: TargetFilter::AnyTarget,
            count: 1,
            up_to: false, at_least: false,
            exclude_source: false,
        })),
        alternative_costs: Vec::new(),
        effect: Some(AbilityEffect::Damage(2)),
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

fn cast_spell(engine: &mut Engine, player: PlayerId, card: ObjectId, modes: Vec<u32>, targets: Vec<Target>) {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::CastSpell { card, modes, targets, x_value: None, alternative_cost: None },
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
fn choosing_mode_zero_gains_life_and_never_touches_mode_ones_target_requirement() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, modal_spell_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    // No targets supplied - legal only because mode 0 has no target
    // requirement at all. If mode 1's AnyTarget requirement leaked in here,
    // this cast would be illegally rejected for a missing target instead.
    cast_spell(&mut engine, p0, card, vec![0], Vec::new());
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    assert_eq!(engine.state().player(p0).unwrap().life, 23, "mode 0's GainLife(3) resolved");
    assert_eq!(engine.state().player(p1).unwrap().life, 20, "mode 1's Damage never ran");
}

#[test]
fn choosing_mode_one_deals_damage_and_validates_its_own_target_requirement() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let target = ObjectId(0);
    state.objects.insert(target, card_object(target, creature_definition(1, "Bear"), p1, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(target);
    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, modal_spell_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![1], vec![Target::Object(target)]);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    assert_eq!(engine.state().object(target).unwrap().damage_marked, 2, "mode 1's Damage(2) resolved");
    assert_eq!(engine.state().player(p0).unwrap().life, 20, "mode 0's GainLife never ran");
}

#[test]
fn modal_spell_rejects_a_target_illegal_for_the_chosen_modes_own_requirement() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, modal_spell_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    // Mode 1 requires exactly one AnyTarget - supplying none is illegal,
    // same as any non-modal spell's own target_requirement would reject.
    cast_spell(&mut engine, p0, card, vec![1], Vec::new());
    let status = drain(&mut engine);

    assert!(matches!(status, StepStatus::Error(CommandError::InvalidTarget)));
    assert_eq!(engine.state().object(card).unwrap().zone, ZoneKind::Hand, "state untouched");
}

#[test]
fn modal_spell_rejects_casting_with_no_mode_chosen() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, modal_spell_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, Vec::new(), Vec::new());
    let status = drain(&mut engine);

    assert!(matches!(status, StepStatus::Error(CommandError::IllegalAction(_))));
    assert_eq!(engine.state().object(card).unwrap().zone, ZoneKind::Hand, "state untouched");
    assert_eq!(engine.state().ledger.len(), 0);
}

#[test]
fn modal_spell_rejects_choosing_two_modes() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, modal_spell_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    // CR 700.2d ("choose two"/repeated modes) is out of scope this slice -
    // narrowed to exactly one.
    cast_spell(&mut engine, p0, card, vec![0, 1], Vec::new());
    let status = drain(&mut engine);

    assert!(matches!(status, StepStatus::Error(CommandError::IllegalAction(_))));
    assert_eq!(engine.state().object(card).unwrap().zone, ZoneKind::Hand, "state untouched");
}

#[test]
fn modal_spell_rejects_an_out_of_range_mode_index() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, modal_spell_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![7], Vec::new());
    let status = drain(&mut engine);

    assert!(matches!(status, StepStatus::Error(CommandError::IllegalAction(_))));
    assert_eq!(engine.state().object(card).unwrap().zone, ZoneKind::Hand, "state untouched");
}

#[test]
fn non_modal_spell_rejects_a_mode_choice() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let target = ObjectId(0);
    state.objects.insert(target, card_object(target, creature_definition(1, "Bear"), p0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(target);
    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, shock_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    // shock_definition() has no modes - announcing one is illegal.
    cast_spell(&mut engine, p0, card, vec![0], vec![Target::Object(target)]);
    let status = drain(&mut engine);

    assert!(matches!(status, StepStatus::Error(CommandError::IllegalAction(_))));
    assert_eq!(engine.state().object(card).unwrap().zone, ZoneKind::Hand, "state untouched");
}

#[test]
fn non_modal_spell_still_casts_normally_with_no_modes_announced() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let target = ObjectId(0);
    state.objects.insert(target, card_object(target, creature_definition(1, "Bear"), p1, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(target);
    let card = ObjectId(2);
    state.objects.insert(card, card_object(card, shock_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, Vec::new(), vec![Target::Object(target)]);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    assert_eq!(engine.state().object(target).unwrap().damage_marked, 2, "regression guard: unaffected by this slice");
}
